// ytm-bridge: transmite un dispositivo de entrada (el cable virtual donde sale Pocket Bard) a un
// canal de voz de Discord como bot. Reemplazo liviano de Kenku FM; los jugadores regulan el
// volumen del bot como siempre. Voz con DAVE (E2EE) via songbird. Prototipo.
//
//   ytm-bridge devices          lista entradas y salidas de audio
//   ytm-bridge test [entrada]   captura 3 s y muestra el nivel
//   ytm-bridge loop             tono en "CABLE Input" + captura de "CABLE Output" (no suena afuera)
//   ytm-bridge                  bot: usa %LOCALAPPDATA%\ytm-float\bridge.json
//                               {"token": "...", "guild": 123, "channel": 456, "device": "CABLE Output"}
mod capture;

use base64::Engine as _;
use serde::Deserialize;
use songbird::{input::RawAdapter, shards::TwilightMap, Songbird};
use std::{
    collections::HashMap,
    num::NonZeroU64,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_model::id::{marker::UserMarker, Id};

#[derive(Deserialize)]
struct Config {
    token: String,
    guild: NonZeroU64,
    channel: NonZeroU64,
    #[serde(default = "default_device")]
    device: String,
}

fn default_device() -> String {
    "CABLE Output".into()
}

fn config_path() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("ytm-float").join("bridge.json")
}

/// El id del bot es la primera parte del token, en base64: evita una llamada HTTP.
fn bot_id(token: &str) -> Option<NonZeroU64> {
    let part = token.split('.').next()?.trim_end_matches('=');
    let raw = base64::engine::general_purpose::STANDARD_NO_PAD.decode(part).ok()?;
    String::from_utf8(raw).ok()?.parse().ok()
}

/// Pico y RMS de lo capturado durante `secs`.
fn level(device: &str, secs: u64) -> Result<(), String> {
    let acc = Arc::new(Mutex::new((0f32, 0f64, 0u64)));
    let a = acc.clone();
    let cap = capture::open(device, move |d| {
        let mut g = a.lock().unwrap();
        for &s in d {
            g.0 = g.0.max(s.abs());
            g.1 += (s as f64) * (s as f64);
            g.2 += 1;
        }
    })?;
    std::thread::sleep(Duration::from_secs(secs));
    let (peak, sq, n) = *acc.lock().unwrap();
    let rms = if n > 0 { (sq / n as f64).sqrt() } else { 0.0 };
    println!("{device}: {} Hz, {} canales, {n} muestras, pico {peak:.3}, rms {rms:.3}", cap.rate, cap.channels);
    Ok(())
}

async fn run(cfg: Config) -> Result<(), String> {
    let user: Id<UserMarker> = bot_id(&cfg.token).ok_or("token con formato inválido")?.into();
    let mut shard = Shard::new(ShardId::ONE, cfg.token.clone(), Intents::GUILDS | Intents::GUILD_VOICE_STATES);
    let senders = TwilightMap::new(HashMap::from([(shard.id().number(), shard.sender())]));
    let songbird = Arc::new(Songbird::twilight(Arc::new(senders), user));

    // La captura arranca ya; songbird la consume cuando entra al canal.
    let (tx, live) = capture::live();
    let cap = capture::open(&cfg.device, move |d| capture::push(&tx, d))?;
    println!("capturando \"{}\" ({} Hz, {} canales)", cfg.device, cap.rate, cap.channels);
    let mut source = Some(RawAdapter::new(live, cap.rate, cap.channels as u32));

    let (guild, channel) = (cfg.guild, cfg.channel);
    let sb = songbird.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sb.remove(guild).await;
        std::process::exit(0);
    });

    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        let Ok(event) = item else { continue };
        songbird.process(&event).await;
        if let Event::Ready(_) = event {
            let Some(src) = source.take() else { continue };
            let sb = songbird.clone();
            tokio::spawn(async move {
                match sb.join(guild, channel).await {
                    Ok(call) => {
                        let mut c = call.lock().await;
                        let _ = c.deafen(true).await; // no escucha: solo transmite
                        c.play_only_input(src.into());
                        println!("en el canal: transmitiendo");
                    }
                    Err(e) => eprintln!("no pude entrar al canal: {e}"),
                }
            });
        }
    }
    drop(cap);
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("devices") => {
            println!("Entradas:");
            capture::input_devices().iter().for_each(|d| println!("  {d}"));
            println!("Salidas:");
            capture::output_devices().iter().for_each(|d| println!("  {d}"));
            Ok(())
        }
        Some("test") => level(args.get(1).map(String::as_str).unwrap_or("CABLE Output"), 3),
        Some("loop") => capture::tone("CABLE Input", 440.0).and_then(|_tone| level("CABLE Output", 3)),
        _ => match std::fs::read_to_string(config_path()) {
            Ok(s) => match serde_json::from_str::<Config>(&s) {
                Ok(cfg) => run(cfg).await,
                Err(e) => Err(format!("bridge.json inválido: {e}")),
            },
            Err(_) => Err(format!("falta {}", config_path().display())),
        },
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
