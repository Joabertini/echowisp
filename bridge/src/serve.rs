// Modo `serve`: la card lanza el puente como hijo y hablan por líneas JSON.
//   card → puente: invite · token · unlink · join(guild,channel) · leave · apps · route(pid,on) · vol(pid,v)
//   puente → card: config · guilds · state · level · apps(available,mode,list) · mix_error
// Si la card se cierra (stdin EOF) el puente sale del canal y termina.
use crate::{audio_mix::{Live, Shared}, bot_id, config_path, mixer::Command};
use base64::Engine as _;
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
use windows::Win32::{
    Foundation::{LocalFree, HLOCAL},
    Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN},
};

pub fn emit(v: Value) {
    if !matches!(v["ev"].as_str(), Some("level" | "apps" | "guilds")) { log(&v.to_string()); }
    println!("{v}"); // stdout es LineWriter: cada linea sale entera
}

/// Keep the original message for bridge.log and old cards; the card renders known codes.
pub fn mix_error(msg: String, pid: u32) {
    let code = if msg.ends_with("está silenciada en Windows") { "muted_source" }
        else if msg.starts_with("la app ") && msg.contains("ya no tiene sesión") { "no_session" }
        else if msg.starts_with("timeout de activación") { "capture_timeout" }
        else if msg.starts_with("GetBuffer devolvió datos nulos") { "capture_buffer" }
        else if msg == "la captura terminó" { "capture_ended" }
        else { "" };
    let app = if code == "muted_source" { msg.strip_suffix(" está silenciada en Windows").unwrap_or("") } else { "" };
    emit(json!({ "ev": "mix_error", "code": code, "app": app, "pid": pid, "msg": msg }));
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
    let code = if msg.starts_with("no pude guardar bridge.json") { "config_save" }
        else if msg == "token con formato inválido" || msg == "token inválido" { "invalid_token" }
        else if msg.starts_with("no pude cifrar el token") { "encrypt_token" }
        else if msg.starts_with("no pude entrar") { "join_failed" }
        else if msg.starts_with("no pude migrar el token") { "token_migrate" }
        else if msg.starts_with("no pude descifrar el token") { "token_decrypt" }
        else if msg.starts_with("Discord cerró la conexión") { "discord_disconnected" }
        else if msg == "desconectado de Discord" { "discord_disconnected" }
        else if msg.starts_with("el audio no arrancó") { "audio_start_failed" }
        else if msg == "el mezclador terminó" { "mixer_stopped" }
        else if msg == "timeout al activar las fuentes" { "sources_timeout" }
        else if msg.starts_with("la app ") && msg.contains("ya no tiene sesión") { "no_session" }
        else { "" };
    let pid = if code == "no_session" { msg.split_whitespace().nth(2).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) } else { 0 };
    emit(json!({ "ev": "state", "s": s, "code": code, "pid": pid, "msg": msg }));
}

/// DPAPI usa la cuenta de Windows actual. La memoria devuelta por Crypt32 pertenece a LocalFree.
fn crypt(data: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    let mut input = CRYPT_INTEGER_BLOB { cbData: data.len().try_into().map_err(|_| "token demasiado largo")?, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let result = unsafe {
        if protect {
            CryptProtectData(&mut input, windows::core::w!("Echowisp bot token"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        } else {
            CryptUnprotectData(&mut input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        }
    };
    if let Err(e) = result { return Err(format!("DPAPI: {e}")); }
    let bytes = if output.cbData == 0 { Vec::new() } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
    };
    if !output.pbData.is_null() { unsafe { LocalFree(HLOCAL(output.pbData.cast())); } }
    Ok(bytes)
}

fn encrypt(token: &str) -> Result<String, String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(crypt(token.as_bytes(), true)?))
}

fn decrypt(encoded: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| format!("base64: {e}"))?;
    String::from_utf8(crypt(&bytes, false)?).map_err(|e| format!("UTF-8: {e}"))
}

/// Siempre escribe un archivo completo antes de reemplazar bridge.json.
fn save_at(path: &std::path::Path, cfg: &Value) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().ok_or("ruta de configuración inválida")?).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

fn save(cfg: &Value) -> Result<(), String> { save_at(&config_path(), cfg) }

/// Migra el token viejo antes de usarlo; nunca lo devuelve si no se pudo persistir cifrado.
fn load_at(path: &std::path::Path) -> (Value, Option<String>, Option<String>) {
    let mut cfg: Value = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
    if let Some(plain) = cfg["token"].as_str().map(str::to_owned) {
        cfg.as_object_mut().map(|o| o.remove("token"));
        let result = encrypt(&plain).and_then(|encrypted| {
            cfg["token_dpapi"] = json!(encrypted);
            save_at(path, &cfg)
        });
        return match result {
            Ok(()) => (cfg, Some(plain), None),
            Err(e) => (cfg, None, Some(format!("no pude migrar el token del bot: {e}"))),
        };
    }
    let token = match cfg["token_dpapi"].as_str() {
        Some(encoded) => match decrypt(encoded) {
            Ok(token) => Some(token),
            Err(e) => return (cfg, None, Some(format!("no pude descifrar el token del bot: {e}"))),
        },
        None => None,
    };
    (cfg, token, None)
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
    let (mut cfg, mut token, load_error) = load_at(&config_path());
    if load_error.is_none() && cfg["version"] != 2 {
        if let Some(object) = cfg.as_object_mut() { object.remove("device"); }
        cfg["version"] = json!(2);
        if let Err(e) = save(&cfg) { state("error", &format!("no pude guardar bridge.json: {e}")); }
    }
    emit(json!({ "ev": "config", "token": token.is_some(),
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

    if let Some(t) = token.as_ref() {
        if let Some((sb, h)) = connect(t.clone(), guilds.clone()) {
            songbird = Some(sb);
            gateway = Some(h);
        }
    } else if let Some(e) = load_error {
        state("error", &e);
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
                        let encrypted = match encrypt(&t) {
                            Ok(v) => v,
                            Err(e) => { state("error", &format!("no pude cifrar el token: {e}")); continue; }
                        };
                        let mut next = cfg.clone();
                        next.as_object_mut().map(|o| o.remove("token"));
                        next["token_dpapi"] = json!(encrypted);
                        if let Err(e) = save(&next) {
                            state("error", &format!("no pude guardar bridge.json: {e}"));
                            continue;
                        }
                        if let (Some(old), Some(sb)) = (tx.take(), songbird.as_ref()) { let _ = sb.remove(old.guild).await; }
                        audio.close(); let (reply, _) = smpsc::channel(); let _ = mix.send(Command::Active(false, reply));
                        cfg = next;
                        token = Some(t.clone());
                        if let Some(h) = gateway.take() { h.abort(); }
                        guilds.lock().unwrap().clear();
                        songbird = None;
                        emit(json!({ "ev": "config", "token": true, "guild": cfg["guild"], "channel": cfg["channel"], "mode": "process_loopback" }));
                        if let Some((sb, h)) = connect(t, guilds.clone()) {
                            songbird = Some(sb);
                            gateway = Some(h);
                        }
                    }
                    Some("invite") => {
                        // Link para agregar el bot a un servidor: ver canales, conectarse y hablar.
                        if let Some(id) = token.as_deref().and_then(crate::bot_id) {
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
                        if let Err(e) = save(&cfg) { state("error", &format!("no pude guardar bridge.json: {e}")); }
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
                    Some("unlink") => {
                        let mut next = cfg.clone();
                        next.as_object_mut().map(|o| { o.remove("token_dpapi"); o.remove("token"); });
                        if let Err(e) = save(&next) {
                            state("error", &format!("no pude guardar bridge.json: {e}"));
                            continue;
                        }
                        if let (Some(old), Some(sb)) = (tx.take(), songbird.as_ref()) { let _ = sb.remove(old.guild).await; }
                        audio.close(); let (reply, _) = smpsc::channel(); let _ = mix.send(Command::Active(false, reply));
                        level.store(0, Relaxed);
                        if let Some(h) = gateway.take() { h.abort(); }
                        songbird = None;
                        token = None;
                        cfg = next;
                        guilds.lock().unwrap().clear();
                        emit_guilds(&guilds);
                        emit(json!({ "ev": "config", "token": false, "guild": cfg["guild"], "channel": cfg["channel"], "mode": "process_loopback" }));
                        state("sin_token", "");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("echowisp-dpapi-{}-{nonce}", std::process::id())).join("bridge.json")
    }

    #[test]
    fn dpapi_round_trip() {
        let token = "test-token.áéí.123";
        let encrypted = encrypt(token).unwrap();
        assert_ne!(encrypted, token);
        assert_eq!(decrypt(&encrypted).unwrap(), token);
    }

    #[test]
    fn migrates_plain_token() {
        let path = test_path();
        save_at(&path, &json!({ "token": "old.token.value", "guild": "42" })).unwrap();
        let (cfg, token, error) = load_at(&path);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(token.as_deref(), Some("old.token.value"));
        assert!(cfg.get("token").is_none());
        assert!(cfg["token_dpapi"].is_string());
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(!file.contains("old.token.value"));
        assert!(!file.contains("\"token\""));
        assert_eq!(load_at(&path).1.as_deref(), Some("old.token.value"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn corrupt_dpapi_is_no_token() {
        let path = test_path();
        save_at(&path, &json!({ "token_dpapi": base64::engine::general_purpose::STANDARD.encode(b"corrupt blob") })).unwrap();
        let (_, token, error) = load_at(&path);
        assert!(token.is_none());
        assert!(error.unwrap().contains("descifrar"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
