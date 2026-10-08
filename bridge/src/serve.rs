// Modo `serve`: la card lanza el puente como hijo y hablan por líneas JSON.
//   card → puente: invite · token · join(guild,channel) · leave · apps · route(pid,on) · vol(pid,v)
//   puente → card: config · guilds · state · level · apps(available,mode,list) · mix_error
// Si la card se cierra (stdin EOF) el puente sale del canal y termina.
use crate::{audio_mix::{Live, Shared}, bot_id, config_path, mixer::Command};
use serde_json::{json, Value};
use songbird::{input::RawAdapter, shards::TwilightMap, Songbird};
use std::{
    collections::{BTreeMap, HashMap},
    num::NonZeroU64,
    sync::{
        atomic::{AtomicU32, Ordering::Relaxed},
        mpsc as smpsc, Arc, Mutex,
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, ShardState, StreamExt as _};
use twilight_model::{channel::ChannelType, gateway::payload::incoming::GuildCreate, id::Id};

pub fn emit(v: Value) {
    if !matches!(v["ev"].as_str(), Some("level" | "apps" | "guilds")) { log(&v.to_string()); }
    println!("{v}"); // stdout es LineWriter: cada linea sale entera
}

/// bridge.log (sesion actual; la anterior queda en bridge.prev.log): estados y errores, sin token.
pub fn log(line: &str) {
    use std::io::Write;
    static FILE: std::sync::OnceLock<Option<Mutex<std::fs::File>>> = std::sync::OnceLock::new();
    let file = FILE.get_or_init(|| {
        let path = config_path().with_file_name("bridge.log");
        let _ = std::fs::rename(&path, path.with_file_name("bridge.prev.log"));
        std::fs::File::create(path).ok().map(Mutex::new)
    });
    if let Some(f) = file {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let _ = writeln!(f.lock().unwrap(), "{t} {line}");
    }
}

fn state(s: &str, msg: &str) {
    emit(json!({ "ev": "state", "s": s, "msg": msg }));
}

/// Config persistida: token y ultima seleccion (bridge.json).
fn load() -> Value {
    std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}))
}

fn save(v: &Value) {
    let p = config_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(p, serde_json::to_string_pretty(v).unwrap_or_default());
}

fn id(v: &Value) -> Option<NonZeroU64> {
    v.as_str()?.parse().ok()
}

type Guilds = Arc<Mutex<BTreeMap<u64, (String, Vec<(u64, String)>)>>>;

fn emit_guilds(g: &Guilds) {
    let list: Vec<Value> = g
        .lock()
        .unwrap()
        .iter()
        .map(|(id, (name, chans))| {
            json!({ "id": id.to_string(), "name": name,
                "channels": chans.iter().map(|(c, n)| json!({ "id": c.to_string(), "name": n })).collect::<Vec<_>>() })
        })
        .collect();
    emit(json!({ "ev": "guilds", "list": list }));
}

/// Captura en un hilo propio (el Stream de cpal no es Send); muere al soltar `stop`.
struct Tx {
    guild: NonZeroU64,
    join_id: u64,
}

fn start_capture(mix: &smpsc::Sender<Command>, audio: &Arc<Shared>) -> Result<RawAdapter<Live>, String> {
    let live = audio.open();
    let (reply, answer) = smpsc::channel();
    mix.send(Command::Active(true, reply)).map_err(|_| "el mezclador terminó".to_string())?;
    answer.recv_timeout(Duration::from_secs(15)).map_err(|_| "timeout al activar las fuentes".to_string())??;
    Ok(RawAdapter::new(live, crate::audio_mix::RATE, crate::audio_mix::CHANNELS as u32))
}

pub async fn serve() {
    let mut cfg = load();
    if cfg["version"] != 2 {
        if let Some(object) = cfg.as_object_mut() { object.remove("device"); }
        cfg["version"] = json!(2);
        save(&cfg);
    }
    emit(json!({ "ev": "config", "token": cfg["token"].is_string(),
        "guild": cfg["guild"], "channel": cfg["channel"], "mode": "process_loopback" }));

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if let Ok(v) = serde_json::from_str::<Value>(&l) {
                let _ = cmd_tx.send(v);
            }
        }
        // stdin cerrado: la card ya no esta
    });

    let level = Arc::new(AtomicU32::new(0));
    let audio = Shared::new(level.clone());
    let (mix, mix_thread) = crate::mixer::start(audio.clone());
    let guilds: Guilds = Default::default();
    let mut songbird: Option<Arc<Songbird>> = None;
    let mut gateway: Option<tokio::task::JoinHandle<()>> = None;
    let mut tx: Option<Tx> = None;
    let mut next_join_id = 0u64;
    let (fail_tx, mut fail_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, String)>();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let mut ticks = 0u64;

    // Conecta el bot (gateway) con el token guardado; los servidores llegan solos por GUILD_CREATE.
    let connect = |token: String, guilds: Guilds| -> Option<(Arc<Songbird>, tokio::task::JoinHandle<()>)> {
        let Some(user) = bot_id(&token) else {
            state("error", "token con formato inválido");
            return None;
        };
        let mut shard = Shard::new(ShardId::ONE, token, Intents::GUILDS | Intents::GUILD_VOICE_STATES);
        let senders = TwilightMap::new(HashMap::from([(shard.id().number(), shard.sender())]));
        let sb = Arc::new(Songbird::twilight(Arc::new(senders), Id::from(user)));
        let sb2 = sb.clone();
        let gateway_fail = fail_tx.clone();
        state("conectando", "");
        let h = tokio::spawn(async move {
            while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
                let ev = match item {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = gateway_fail.send((0, format!("gateway: {e}")));
                        return;
                    }
                };
                sb2.process(&ev).await;
                match ev {
                    Event::Ready(_) => state("listo", ""),
                    Event::GuildCreate(gc) => {
                        if let GuildCreate::Available(g) = *gc {
                            let mut chans: Vec<(i32, u64, String)> = g
                                .channels
                                .iter()
                                .filter(|c| matches!(c.kind, ChannelType::GuildVoice | ChannelType::GuildStageVoice))
                                .map(|c| (c.position.unwrap_or(0), c.id.get(), c.name.clone().unwrap_or_default()))
                                .collect();
                            chans.sort();
                            guilds.lock().unwrap().insert(g.id.get(), (g.name.clone(), chans.into_iter().map(|(_, i, n)| (i, n)).collect()));
                            emit_guilds(&guilds);
                        }
                    }
                    Event::GuildDelete(g) => {
                        guilds.lock().unwrap().remove(&g.id.get());
                        emit_guilds(&guilds);
                    }
                    // Cierre fatal (token invalido, intents): sin esto la card quedaba en "conectando…".
                    Event::GatewayClose(f) if shard.state() == ShardState::FatallyClosed => {
                        match f.map(|f| f.code).unwrap_or(0) {
                            4004 => { let _ = gateway_fail.send((0, "token inválido".into())); }
                            c => { let _ = gateway_fail.send((0, format!("Discord cerró la conexión ({c})"))); }
                        }
                        return;
                    }
                    _ => {}
                }
            }
            let _ = gateway_fail.send((0, "desconectado de Discord".into()));
        });
        Some((sb, h))
    };

    if let Some(t) = cfg["token"].as_str() {
        if let Some((sb, h)) = connect(t.to_string(), guilds.clone()) {
            songbird = Some(sb);
            gateway = Some(h);
        }
    } else {
        state("sin_token", "");
    }

    loop {
        tokio::select! {
            c = cmd_rx.recv() => {
                let Some(c) = c else { break };
                match c["cmd"].as_str() {
                    Some("apps" | "route" | "vol") => {
                        let _ = mix.send(Command::Json(c));
                    }
                    Some("token") => {
                        let Some(t) = c["token"].as_str().map(|s| s.trim().to_string()) else { continue };
                        if let (Some(old), Some(sb)) = (tx.take(), songbird.as_ref()) { let _ = sb.remove(old.guild).await; }
                        audio.close(); let (reply, _) = smpsc::channel(); let _ = mix.send(Command::Active(false, reply));
                        cfg["token"] = json!(t);
                        save(&cfg);
                        if let Some(h) = gateway.take() { h.abort(); }
                        guilds.lock().unwrap().clear();
                        if let Some((sb, h)) = connect(t, guilds.clone()) {
                            songbird = Some(sb);
                            gateway = Some(h);
                        }
                    }
                    Some("invite") => {
                        // Link para agregar el bot a un servidor: ver canales, conectarse y hablar.
                        if let Some(id) = cfg["token"].as_str().and_then(crate::bot_id) {
                            emit(json!({ "ev": "invite", "url": format!(
                                "https://discord.com/oauth2/authorize?client_id={id}&scope=bot&permissions=3146752") }));
                        }
                    }
                    Some("join") => {
                        let (Some(g), Some(ch)) = (id(&c["guild"]), id(&c["channel"])) else { continue };
                        let Some(sb) = songbird.clone() else { state("sin_token", ""); continue };
                        if let Some(t) = tx.take() {
                            let _ = sb.remove(t.guild).await;
                            audio.close(); let (reply, _) = smpsc::channel(); let _ = mix.send(Command::Active(false, reply));
                        }
                        cfg["guild"] = json!(g.to_string());
                        cfg["channel"] = json!(ch.to_string());
                        cfg.as_object_mut().map(|v| v.remove("device"));
                        save(&cfg);
                        let src = match start_capture(&mix, &audio) {
                            Ok(x) => x,
                            Err(e) => { state("error", &e); continue; }
                        };
                        state("entrando", "");
                        match sb.join(g, ch).await {
                            Ok(call) => {
                                let mut call = call.lock().await;
                                let _ = call.deafen(true).await; // solo transmite
                                let pista = call.play_only_input(src.into());
                                next_join_id += 1;
                                tx = Some(Tx { guild: g, join_id: next_join_id });
                                state("transmitiendo", "");
                                // Si la pista no arranca (codec, formato) songbird no avisa: se revisa a los 2 s.
                                let track_fail = fail_tx.clone();
                                let join_id = next_join_id;
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                    if let Ok(info) = pista.get_info().await {
                                        if let songbird::tracks::PlayMode::Errored(e) = info.playing {
                                            let _ = track_fail.send((join_id, format!("el audio no arrancó: {e:?}")));
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                audio.close(); let (reply, _) = smpsc::channel(); let _ = mix.send(Command::Active(false, reply));
                                state("error", &format!("no pude entrar: {e}"));
                            }
                        }
                    }
                    Some("leave") => {
                        if let (Some(t), Some(sb)) = (tx.take(), songbird.as_ref()) {
                            let _ = sb.remove(t.guild).await;
                            audio.close(); let (reply, _) = smpsc::channel(); let _ = mix.send(Command::Active(false, reply));
                        }
                        level.store(0, Relaxed);
                        state(if songbird.is_some() { "listo" } else { "sin_token" }, "");
                    }
                    _ => {}
                }
            }
            _ = tick.tick() => {
                if tx.is_some() {
                    emit(json!({ "ev": "level", "v": (f32::from_bits(level.load(Relaxed)) * 1000.0).round() / 1000.0 }));
                    ticks += 1;
                    if ticks % 50 == 0 { log(&format!("audio {:?}", audio.stats())); } // cada 10 s
                }
            }
            failure = fail_rx.recv() => {
                if let Some((join_id, message)) = failure {
                    if join_id != 0 && tx.as_ref().is_none_or(|t| t.join_id != join_id) { continue; }
                    if let (Some(t), Some(sb)) = (tx.take(), songbird.as_ref()) {
                        let _ = sb.remove(t.guild).await;
                    }
                    audio.close();
                    let (reply, _) = smpsc::channel();
                    let _ = mix.send(Command::Active(false, reply));
                    level.store(0, Relaxed);
                    if join_id == 0 {
                        if let Some(h) = gateway.take() { h.abort(); }
                        songbird = None;
                    }
                    state("error", &message);
                }
            }
        }
    }

    // La card se fue: devolver las salidas de las apps y salir del canal antes de terminar.
    if let (Some(t), Some(sb)) = (tx.take(), songbird.as_ref()) {
        let _ = sb.remove(t.guild).await;
    }
    drop(mix);
    let _ = mix_thread.join();
}
