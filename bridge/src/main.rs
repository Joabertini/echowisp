// ytm-bridge: captura apps elegidas por process loopback y mezcla su audio para transmitirlo a
// un canal de voz de Discord como bot. Voz con DAVE (E2EE) via songbird.
//
// Lo lanza la card (`ytm-bridge serve`) y hablan por lineas JSON: ver serve.rs.
// Token y ultima seleccion en %LOCALAPPDATA%\ytm-float\bridge.json.
mod audio_probe;
mod audio_mix;
mod mixer;
mod process_capture;
mod serve;
mod sim;

use base64::Engine as _;
use std::{num::NonZeroU64, path::PathBuf};

fn config_path() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("ytm-float").join("bridge.json")
}

/// El id del bot es la primera parte del token, en base64: evita una llamada HTTP.
fn bot_id(token: &str) -> Option<NonZeroU64> {
    let part = token.split('.').next()?.trim_end_matches('=');
    let raw = base64::engine::general_purpose::STANDARD_NO_PAD.decode(part).ok()?;
    String::from_utf8(raw).ok()?.parse().ok()
}

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("audio-probe") {
        if let Err(e) = audio_probe::run(std::env::args().skip(2)) {
            eprintln!("audio-probe: {e}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("simular") {
        if let Err(e) = sim::run(std::env::args().skip(2)) {
            eprintln!("simular: {e}");
            std::process::exit(1);
        }
        return;
    }
    serve::serve().await;
}
