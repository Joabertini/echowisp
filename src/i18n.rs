//! User-facing text for the card. Protocol identifiers and log messages stay untranslated.
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang { Es, En }

static LANG: AtomicU8 = AtomicU8::new(0);

#[link(name = "kernel32")]
unsafe extern "system" { fn GetUserDefaultUILanguage() -> u16; }

pub fn from_windows_langid(id: u16) -> Lang {
    if id & 0x03ff == 0x000a { Lang::Es } else { Lang::En }
}

pub fn system_lang() -> Lang { from_windows_langid(unsafe { GetUserDefaultUILanguage() }) }

pub fn current() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::Es,
        2 => Lang::En,
        _ => {
            let saved = std::fs::read_to_string(crate::brave::data_dir().join("card.json"))
                .ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v["idioma"].as_str().map(str::to_owned));
            let lang = match saved.as_deref() { Some("es") => Lang::Es, Some("en") => Lang::En, _ => system_lang() };
            set(lang);
            lang
        }
    }
}

pub fn set(lang: Lang) { LANG.store(if lang == Lang::Es { 1 } else { 2 }, Ordering::Relaxed); }

// Key, Spanish, English. Keep English short: the cards have fixed widths.
pub const TEXTS: &[(&str, &str, &str)] = &[
    ("no_music", "Nada sonando", "Nothing playing"),
    ("search_lists", "Buscá o abrí tus listas", "Search or open playlists"),
    ("no_session", "Sin sesión · tocá el ícono", "Sign in · tap the icon"),
    ("loading", "Cargando…", "Loading…"),
    ("bot_no_server", "Bot sin servidor: invitalo desde ⚙", "Invite the bot from Settings"),
    ("server_no_voice", "El servidor no tiene canales de voz", "No voice channels"),
    ("choose_channel", "Elegí servidor y canal", "Choose a server and channel"),
    ("discord_open_failed", "No pude abrir Discord: ", "Could not open Discord: "),
    ("paste_token", "Copiá el token del bot y tocá de nuevo", "Copy the bot token and tap again"),
    ("saving_token", "Guardando token…", "Saving token…"),
    ("bridge", "Puente", "Bridge"),
    ("mixer", "Mezclador", "Mixer"),
    ("queue_shuffled", "Cola mezclada", "Queue shuffled"),
    ("no_playlists", "Sin listas (¿sesión iniciada?)", "No playlists (signed in?)"),
    ("no_section", "No encontré esa sección", "Section not found"),
    ("no_results", "Sin resultados", "No results"),
    ("reloading", "Recargando YouTube Music…", "Reloading YouTube Music…"),
    ("search_song", "Buscar canción…", "Search songs…"),
    ("lists", "Listas", "Playlists"),
    ("again", "Escuchar otra vez", "Listen again"),
    ("quick", "Selección rápida", "Quick picks"),
    ("bridge_setup", "Puente · configurar", "Bridge · set up"),
    ("live", "al aire", "live"),
    ("joining", "Entrando a", "Joining"),
    ("bridge_connecting", "Puente · conectando…", "Bridge · connecting…"),
    ("invite_bot", "Invitá el bot al servidor", "Invite the bot"),
    ("bridge_channel", "Puente · elegí canal", "Bridge · choose channel"),
    ("discord_close", "Discord · abierto (clic cierra)", "Discord · click to close"),
    ("discord_open", "Discord · abrir ventana", "Discord · open window"),
    ("radio", "Radio", "Radio"),
    ("ad", "Anuncio", "Ad"),
    ("unknown_command", "comando desconocido", "unknown command"),
    ("app_capture_unavailable", "Captura por app no disponible", "App capture unavailable"),
    ("no_shared_output", "Sin salida compartida activa", "No shared audio output"),
    ("app_audio_note", "Audio de apps · sin cable virtual", "App audio · no virtual cable"),
    ("report_note", "Contanos qué pasó. Le llega directo a quien hace la app.", "Tell us what happened. It goes to the developer."),
    ("apps_title", "Apps a Discord", "Apps to Discord"),
    ("report_title", "Reportar un problema", "Report a problem"),
    ("settings", "Configuración", "Settings"),
    ("section_card", "EN LA CARD", "ON THE CARD"),
    ("section_appearance", "APARIENCIA", "APPEARANCE"),
    ("section_bot", "BOT DE DISCORD", "DISCORD BOT"),
    ("section_capture", "CAPTURA POR APP", "APP CAPTURE"),
    ("section_browser", "NAVEGADOR", "BROWSER"),
    ("section_hotkeys", "ATAJOS", "SHORTCUTS"),
    ("section_language", "IDIOMA", "LANGUAGE"),
    ("spanish", "Español", "Español"),
    ("english", "English", "English"),
    ("echowisp_music", "Música (Echowisp)", "Music (Echowisp)"),
    ("accent", "Acento", "Accent"),
    ("background", "Fondo", "Background"),
    ("discord_bridge", "Puente a Discord", "Discord bridge"),
    ("discord_window", "Ventana de Discord", "Discord window"),
    ("token_saved", "Token guardado · pegar otro", "Token saved · paste another"),
    ("token_prompt", "Pegar token del bot (copialo y tocá)", "Paste bot token (copy, then tap)"),
    ("invite_server", "Invitar el bot a un servidor", "Invite bot to a server"),
    ("unlink_bot", "Desvincular bot", "Unlink bot"),
    ("update_to", "Actualizar a", "Update to"),
    ("version", "Versión", "Version"),
    ("checking", "buscando novedades…", "checking…"),
    ("up_to_date", "al día", "up to date"),
    ("retry_offline", "sin conexión (reintentar)", "offline (retry)"),
    ("installing", "Instalando…", "Installing…"),
    ("downloading", "Bajando…", "Downloading…"),
    ("retry", "reintentar", "retry"),
    ("report_hint", "¿Qué pasó? ¿Qué estabas haciendo?", "What happened? What were you doing?"),
    ("contact_hint", "Tu mail, si querés respuesta", "Your email for a reply (optional)"),
    ("attach_technical", "Adjuntar datos técnicos", "Attach technical details"),
    ("sending", "Enviando…", "Sending…"),
    ("send", "Enviar", "Send"),
    ("press_shortcut", "presioná la combinación…", "press shortcut…"),
    ("space", "Espacio", "Space"),
    ("key", "tecla", "key"),
    ("play_pause", "Play / pausa", "Play / pause"),
    ("next", "Siguiente", "Next"),
    ("previous", "Anterior", "Previous"),
    ("show_hide", "Mostrar / ocultar", "Show / hide"),
    ("collapse", "Colapsar", "Collapse"),
    ("search", "Buscar", "Search"),
    ("opening_invite", "Abriendo la invitación en el navegador", "Opening invite in browser"),
    ("browser_changed", "Navegador cambiado: se usa al reabrir la card", "Browser set for next launch"),
    ("browser_save_failed", "No pude guardar el navegador", "Could not save browser"),
    ("report_empty", "Escribí qué pasó antes de enviar", "Describe the issue before sending"),
    ("report_sent", "Reporte enviado. ¡Gracias!", "Report sent. Thanks!"),
    ("new_version", "Hay una versión nueva", "New version"),
    ("shortcut_mod", "Usá Ctrl o Alt en la combinación", "Use Ctrl or Alt"),
    ("shortcut_taken", "Esa combinación la usa otra app", "Another app uses that shortcut"),
    ("starting", "Iniciando…", "Starting…"),
    ("still_loading", "Todavía cargando…", "Still loading…"),
    ("browser_missing", "No encontré el navegador (reinstalá y elegí uno)", "Browser not found"),
    ("login_prompt", "Iniciá sesión en la ventana de Brave y cerrala al terminar", "Sign in, then close Brave"),
    ("brave_launch_failed", "Error al abrir Brave: ", "Could not open Brave: "),
    ("starting_ytm", "Iniciando YouTube Music…", "Starting YouTube Music…"),
    ("reconnecting", "Reconectando", "Reconnecting"),
    ("brave_closed", "Brave se cerró al arrancar", "Brave closed during startup"),
    ("brave_unresponsive", "Brave no respondió", "Brave did not respond"),
    ("tab_missing", "sin pestaña", "no tab"),
    ("tab_crashed", "pestaña caída", "tab crashed"),
    ("download_corrupt", "La descarga llegó dañada: probá de nuevo", "Download damaged. Try again"),
    ("installer_save_failed", "No pude guardar el instalador", "Could not save installer"),
    ("installer_open_failed", "No pude abrir el instalador: ", "Could not open installer: "),
    ("offline", "Sin conexión: revisá internet y probá de nuevo", "Offline. Check internet"),
    ("invalid_url", "URL inválida", "Invalid URL"),
    ("server_replied", "El servidor respondió", "Server replied"),
    ("try_later", "probá más tarde", "try later"),
    ("report_rate", "Mandaste varios seguidos: probá en un rato", "Too many reports. Try later"),
];

pub fn t(key: &str) -> &'static str {
    let (_, es, en) = TEXTS.iter().find(|row| row.0 == key).unwrap_or_else(|| panic!("missing i18n key: {key}"));
    if current() == Lang::Es { es } else { en }
}

pub fn relocalize(message: &str) -> String {
    for &(key, es, en) in TEXTS {
        if message == es || message == en { return t(key).into(); }
        if matches!(key, "reconnecting" | "brave_launch_failed" | "discord_open_failed" | "installer_open_failed") {
            if let Some(rest) = message.strip_prefix(es).or_else(|| message.strip_prefix(en)) {
                return format!("{}{rest}", t(key));
            }
        }
    }
    message.into()
}

pub fn runtime_error(message: &str) -> &str {
    let key = match message {
        "Brave se cerró al arrancar" => "brave_closed",
        "Brave no respondió" => "brave_unresponsive",
        "sin pestaña" => "tab_missing",
        "pestaña caída" => "tab_crashed",
        _ => return message,
    };
    t(key)
}

pub fn mix_error(code: &str, app: &str, pid: u64, fallback: &str) -> String {
    let (es, en) = match code {
        "route_save" => ("No pude guardar ruteo.json", "Could not save ruteo.json"),
        "no_shared_output" => ("La app no tiene salida compartida activa", "No shared audio output"),
        "discord_source" => ("Discord no puede ser fuente del bot", "Discord cannot be a source"),
        "overlap" => ("La app se solapa con otra fuente", "App overlaps another source"),
        "no_session" => ("La app ya no tiene sesión de audio", "App has no audio session"),
        "waiting_session" => ("Esperando audio compartido", "Waiting for shared audio"),
        "output_closed" => ("La app no reabrió su audio", "App did not reopen audio"),
        "volume_changed" => ("Cambió el volumen en Windows; envío detenido", "Windows volume changed; stopped"),
        "capture_ended" => ("La captura terminó", "Capture ended"),
        "com_failed" => ("COM de audio no inició", "Audio COM failed"),
        "catalog_failed" => ("No pude abrir sesiones de Windows", "Could not open Windows sessions"),
        "last_source_stopped" => ("La última fuente de audio se detuvo", "Last audio source stopped"),
        "config_save" => ("No pude guardar bridge.json", "Could not save bridge.json"),
        "invalid_token" => ("Token inválido", "Invalid token"),
        "encrypt_token" => ("No pude cifrar el token", "Could not encrypt token"),
        "join_failed" => ("No pude entrar al canal", "Could not join channel"),
        "muted_source" => ("Silenciada en Windows", "Muted in Windows"),
        "capture_timeout" => ("La captura tardó demasiado", "Audio capture timed out"),
        "capture_buffer" => ("La captura devolvió datos inválidos", "Audio capture returned invalid data"),
        "token_migrate" => ("No pude migrar el token", "Could not migrate token"),
        "token_decrypt" => ("No pude descifrar el token", "Could not decrypt token"),
        "discord_disconnected" => ("Discord se desconectó", "Discord disconnected"),
        "audio_start_failed" => ("El audio no arrancó", "Audio did not start"),
        "mixer_stopped" => ("El mezclador se detuvo", "Mixer stopped"),
        "sources_timeout" => ("Las fuentes tardaron demasiado", "Audio sources timed out"),
        _ => return fallback.to_string(),
    };
    let base = if current() == Lang::Es { es } else { en };
    if !app.is_empty() { format!("{app}: {base}") } else if pid != 0 { format!("{pid}: {base}") } else { base.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn language_detection() {
        assert_eq!(from_windows_langid(0x0c0a), Lang::Es);
        assert_eq!(from_windows_langid(0x040a), Lang::Es);
        assert_eq!(from_windows_langid(0x0409), Lang::En);
        assert_eq!(from_windows_langid(0x040c), Lang::En);
    }
    #[test] fn all_keys_have_both_languages() {
        let mut seen = std::collections::HashSet::new();
        for &(key, es, en) in TEXTS {
            assert!(seen.insert(key), "duplicate key: {key}");
            assert!(!es.is_empty() && !en.is_empty(), "empty translation: {key}");
        }
    }
    #[test] fn literal_key_uses_exist() {
        for source in [include_str!("main.rs"), include_str!("panel.rs"), include_str!("engine.rs"), include_str!("update.rs"), include_str!("report.rs")] {
            for (at, _) in source.match_indices("t(\"") {
                if source[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') { continue; }
                let key = source[at + 3..].split('"').next().unwrap();
                assert!(TEXTS.iter().any(|row| row.0 == key), "missing translation: {key}");
            }
        }
    }
    #[test] fn accented_literals_are_not_untranslated_ui() {
        let sources = [
            ("main.rs", include_str!("main.rs")), ("panel.rs", include_str!("panel.rs")),
            ("engine.rs", include_str!("engine.rs")), ("update.rs", include_str!("update.rs")),
            ("report.rs", include_str!("report.rs")), ("brave.rs", include_str!("brave.rs")),
            ("bridge.rs", include_str!("bridge.rs")), ("cdp.rs", include_str!("cdp.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("bridge/mixer.rs", include_str!("../bridge/src/mixer.rs")),
            ("bridge/serve.rs", include_str!("../bridge/src/serve.rs")),
            ("bridge/main.rs", include_str!("../bridge/src/main.rs")),
            ("bridge/audio_mix.rs", include_str!("../bridge/src/audio_mix.rs")),
            ("bridge/audio_probe.rs", include_str!("../bridge/src/audio_probe.rs")),
            ("bridge/process_capture.rs", include_str!("../bridge/src/process_capture.rs")),
            ("bridge/sim.rs", include_str!("../bridge/src/sim.rs")),
        ];
        // Explicit exceptions: protocol/log messages, test fixtures and diagnostic CLI text.
        const ALLOWED: &[&str] = &[
            "Brave se cerró al arrancar", "Brave no respondió", "sesión: {e}", "sin pestaña", "pestaña caída",
            "--- sesión anterior ---", "--- sesión actual ---", "ñandú", "no suena\\nñand", "Prueba automática del envío desde la app",
            "Música (Echowisp)", "está silenciada en Windows", "ya no tiene sesión", "notificación de volumen", "la app {pid} ya no tiene sesión de audio activa",
            "la app no tiene salida de audio compartida activa", "el árbol de procesos se solapa", "la app {pid} no tiene sesión compartida",
            "la app {pid} no reabrió su salida", "cambió volumen en Windows", "la captura terminó", "la última fuente de audio se detuvo",
            "COM de audio no inició", "no pude abrir el catálogo de sesiones", "ruta de configuración inválida", "el mezclador terminó",
            "token con formato inválido", "token inválido", "Discord cerró la conexión", "el audio no arrancó", "test-token.áéí.123",
            "callback sin operación", "activación", "GetBuffer devolvió datos nulos", "timeout de activación",
            "La audibilidad local se verifica", "songbird no parseó", "la pista terminaría acá", "timeout de activación de process loopback",
            "no pude guardar ruteo.json", "no pude guardar bridge.json", "no pude cifrar el token", "no pude entrar",
        ];
        for (file, source) in sources {
            for (line_no, line) in source.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if !code.contains('"') || !code.chars().any(|c| "áéíóúñÁÉÍÓÚÑ".contains(c)) { continue; }
                assert!(ALLOWED.iter().any(|s| code.contains(s)), "untranslated accented literal at {file}:{}: {code}", line_no + 1);
            }
        }
    }
}
