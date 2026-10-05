// ytm-bridge: transmite un dispositivo de entrada (el cable virtual donde sale Pocket Bard) a un
// canal de voz de Discord como bot. Reemplazo liviano de Kenku FM; los jugadores regulan el
// volumen del bot como siempre. Voz con DAVE (E2EE) via songbird.
//
// Lo lanza la card (`ytm-bridge serve`) y hablan por lineas JSON: ver serve.rs.
// Token y ultima seleccion en %LOCALAPPDATA%\ytm-float\bridge.json.
mod capture;
mod serve;

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
    serve::serve().await;
}
