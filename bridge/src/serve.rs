// Modo `serve`: la card lanza el puente como hijo y hablan por lineas JSON.
//   card → puente: {"cmd":"token","token":"…"} · {"cmd":"join","guild":"…","channel":"…","device":"…"} · {"cmd":"leave"}
//   puente → card: {"ev":"devices","list":[…]} · {"ev":"config",…} · {"ev":"guilds","list":[…]}
//                  {"ev":"state","s":"…","msg":"…"} · {"ev":"level","v":0.0}
// Si la card se cierra (stdin EOF) el puente sale del canal y termina.
use crate::{bot_id, capture, config_path};
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

fn emit(v: Value) {
    println!("{v}"); // stdout es LineWriter: cada linea sale entera
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
    stop: smpsc::Sender<()>,
    guild: NonZeroU64,
}

fn start_capture(device: &str, level: Arc<AtomicU32>) -> Result<(smpsc::Sender<()>, RawAdapter<capture::Live>), String> {
    let (tx, live) = capture::live();
    let (ready_tx, ready_rx) = smpsc::channel();
    let (stop_tx, stop_rx) = smpsc::channel::<()>();
    let dev = device.to_string();
    std::thread::spawn(move || {
        let r = capture::open(&dev, move |d| {
            let peak = d.iter().fold(0f32, |m, s| m.max(s.abs()));
            // pico decreciente: el medidor de la card cae suave
            let old = f32::from_bits(level.load(Relaxed));
            level.store(peak.max(old * 0.9).to_bits(), Relaxed);
            let _ = tx.try_send(d.to_vec()); // cola llena: el bloque se pierde
        });
        match r {
            Ok(cap) => {
                let _ = ready_tx.send(Ok((cap.rate, cap.channels)));
                let _ = stop_rx.recv(); // espera a que la suelten
                drop(cap);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    });
    let (rate, ch) = ready_rx.recv().map_err(|_| "la captura no arrancó".to_string())??;
    Ok((stop_tx, RawAdapter::new(live, rate, ch as u32)))
}

pub async fn serve() {
    let mut cfg = load();
    emit(json!({ "ev": "devices", "list": capture::input_devices() }));
    emit(json!({ "ev": "config", "token": cfg["token"].is_string(),
        "guild": cfg["guild"], "channel": cfg["channel"], "device": cfg["device"].as_str().unwrap_or("CABLE Output") }));

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

    let guilds: Guilds = Default::default();
    let level = Arc::new(AtomicU32::new(0));
    let mut songbird: Option<Arc<Songbird>> = None;
    let mut gateway: Option<tokio::task::JoinHandle<()>> = None;
    let mut tx: Option<Tx> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(200));

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
        state("conectando", "");
        let h = tokio::spawn(async move {
            while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
                let ev = match item {
                    Ok(e) => e,
                    Err(e) => {
                        state("error", &format!("gateway: {e}"));
                        continue;
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
                            4004 => state("error", "token inválido"),
                            c => state("error", &format!("Discord cerró la conexión ({c})")),
                        }
                        return;
                    }
                    _ => {}
                }
            }
            state("error", "desconectado de Discord");
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
                    Some("token") => {
                        let Some(t) = c["token"].as_str().map(|s| s.trim().to_string()) else { continue };
                        cfg["token"] = json!(t);
                        save(&cfg);
                        if let Some(h) = gateway.take() { h.abort(); }
                        guilds.lock().unwrap().clear();
                        if let Some((sb, h)) = connect(t, guilds.clone()) {
                            songbird = Some(sb);
                            gateway = Some(h);
                        }
                    }
                    Some("join") => {
                        let (Some(g), Some(ch)) = (id(&c["guild"]), id(&c["channel"])) else { continue };
                        let Some(sb) = songbird.clone() else { state("sin_token", ""); continue };
                        let device = c["device"].as_str().unwrap_or("CABLE Output").to_string();
                        if let Some(t) = tx.take() {
                            let _ = sb.remove(t.guild).await;
                            let _ = t.stop.send(());
                        }
                        cfg["guild"] = json!(g.to_string());
                        cfg["channel"] = json!(ch.to_string());
                        cfg["device"] = json!(device);
                        save(&cfg);
                        let (stop, src) = match start_capture(&device, level.clone()) {
                            Ok(x) => x,
                            Err(e) => { state("error", &e); continue; }
                        };
                        state("entrando", "");
                        match sb.join(g, ch).await {
                            Ok(call) => {
                                let mut call = call.lock().await;
                                let _ = call.deafen(true).await; // solo transmite
                                call.play_only_input(src.into());
                                tx = Some(Tx { stop, guild: g });
                                state("transmitiendo", "");
                            }
                            Err(e) => {
                                let _ = stop.send(());
                                state("error", &format!("no pude entrar: {e}"));
                            }
                        }
                    }
                    Some("leave") => {
                        if let (Some(t), Some(sb)) = (tx.take(), songbird.as_ref()) {
                            let _ = sb.remove(t.guild).await;
                            let _ = t.stop.send(());
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
                }
            }
        }
    }

    // La card se fue: salir del canal antes de terminar.
    if let (Some(t), Some(sb)) = (tx.take(), songbird.as_ref()) {
        let _ = sb.remove(t.guild).await;
        let _ = t.stop.send(());
    }
}
