// "Actualizar": consulta el manifiesto del sitio, baja el instalador nuevo y lo corre en silencio.
// Bajado por la app (WinHTTP) no lleva la marca de "descargado de internet", asi que SmartScreen no lo frena
// como cuando el usuario lo baja con el navegador. Se verifica el SHA-256 del manifiesto antes de correrlo.
use crate::brave::wide;
use serde_json::Value;
use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Networking::WinHttp::*,
    Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const WM_UPDATE: u32 = WM_APP + 5;
// Las 0.4.x leen /ytm-float/version.json: al publicar, actualizar los dos (apuntan al mismo instalador).
const MANIFEST: &str = "https://www.bertinilabs.xyz/echowisp/version.json";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Lo que llega a la card por WM_UPDATE (lparam = Box<Msg>).
pub enum Msg {
    Available(Info),
    Progress(u32), // porcentaje
    Failed(String),
    Launched, // el instalador quedo corriendo: la card tiene que cerrarse
    UpToDate,
    Offline, // no se pudo leer el manifiesto (sin conexion o sitio caido)
}

#[derive(Clone)]
pub struct Info {
    pub version: String,
    url: String,
    sha256: String,
}

/// Estado de la fila de version en Configuracion.
pub enum State {
    None, // consultando el manifiesto
    Current,
    Unknown, // sin conexion: tocar reintenta
    Ready(Info),
    Busy(u32, Info),
    Error(String, Info),
}

fn post(hwnd: isize, m: Msg) {
    let p = Box::into_raw(Box::new(m));
    if unsafe { PostMessageW(hwnd as _, WM_UPDATE, 0, p as isize) } == 0 {
        drop(unsafe { Box::from_raw(p) });
    }
}

/// "0.3.10" > "0.3.9": por componentes numericos.
fn newer(remote: &str, local: &str) -> bool {
    let n = |s: &str| s.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    n(remote) > n(local)
}

fn parse(manifest: &Value) -> Option<Info> {
    let s = |k: &str| manifest[k].as_str().map(str::to_string);
    let info = Info { version: s("version")?, url: s("url")?, sha256: s("sha256")?.to_lowercase() };
    (info.url.starts_with("https://") && info.sha256.len() == 64).then_some(info)
}

/// Al arrancar, en un hilo: avisa si hay version nueva, si esta al dia o si no se pudo consultar.
pub fn check(hwnd: isize) {
    std::thread::spawn(move || {
        let Ok(body) = (unsafe { get(MANIFEST, &mut |_, _| {}) }) else { return post(hwnd, Msg::Offline) };
        let Some(info) = serde_json::from_slice::<Value>(&body).ok().as_ref().and_then(parse) else { return post(hwnd, Msg::Offline) };
        post(hwnd, if newer(&info.version, env!("CARGO_PKG_VERSION")) { Msg::Available(info) } else { Msg::UpToDate });
    });
}

/// Baja, verifica y lanza el instalador. `browser` = navegador actual (el instalador no lo cambia).
pub fn install(hwnd: isize, info: Info, browser: PathBuf) {
    std::thread::spawn(move || {
        let mut last = u32::MAX;
        let data = match unsafe {
            get(&info.url, &mut |got, total| {
                let pct = if total > 0 { (got * 100 / total) as u32 } else { 0 };
                if pct != last {
                    last = pct;
                    post(hwnd, Msg::Progress(pct));
                }
            })
        } {
            Ok(d) => d,
            Err(e) => return post(hwnd, Msg::Failed(e)),
        };
        if sha256_hex(&data).as_deref() != Some(info.sha256.as_str()) {
            return post(hwnd, Msg::Failed("La descarga llegó dañada: probá de nuevo".into()));
        }
        let setup = std::env::temp_dir().join(format!("echowisp-setup-{}.exe", info.version));
        if std::fs::write(&setup, &data).is_err() {
            return post(hwnd, Msg::Failed("No pude guardar el instalador".into()));
        }
        // La card tiene que cerrarse antes de que el instalador arranque (AppMutex): cmd espera ~2 s.
        let cmd = format!(
            "/S /C \"ping -n 3 127.0.0.1 >nul & \"{}\" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /RELANZAR=1 \"/NAVEGADOR={}\"\"",
            setup.display(),
            browser.display()
        );
        match std::process::Command::new("cmd.exe").raw_arg(cmd).creation_flags(CREATE_NO_WINDOW).spawn() {
            Ok(_) => post(hwnd, Msg::Launched),
            Err(e) => post(hwnd, Msg::Failed(format!("No pude abrir el instalador: {e}"))),
        }
    });
}

fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, null(), 0, data.as_ptr(), data.len() as u32, out.as_mut_ptr(), 32) };
    (st == 0).then(|| out.iter().map(|b| format!("{b:02x}")).collect())
}

/// GET https; `progress(bajado, total)`. Error en castellano para mostrar en la card.
unsafe fn get(url: &str, progress: &mut dyn FnMut(usize, usize)) -> Result<Vec<u8>, String> {
    struct H(*mut core::ffi::c_void);
    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    const OFFLINE: &str = "Sin conexión: revisá internet y probá de nuevo";
    let ok = |h: *mut core::ffi::c_void| if h.is_null() { Err(OFFLINE.to_string()) } else { Ok(H(h)) };
    let rest = url.strip_prefix("https://").ok_or("URL inválida")?;
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let agent = wide(concat!("echowisp/", env!("CARGO_PKG_VERSION")));
    let ses = ok(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
    WinHttpSetTimeouts(ses.0, 10_000, 10_000, 15_000, 30_000);
    let con = ok(WinHttpConnect(ses.0, wide(host).as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
    let req = ok(WinHttpOpenRequest(con.0, wide("GET").as_ptr(), wide(&path).as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE))?;
    if WinHttpSendRequest(req.0, null(), 0, null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, null_mut()) == 0 {
        return Err(OFFLINE.into());
    }
    let num = |q: u32| {
        let mut v = 0u32;
        let mut len = 4u32;
        let r = WinHttpQueryHeaders(req.0, q | WINHTTP_QUERY_FLAG_NUMBER, null(), (&mut v as *mut u32).cast(), &mut len, null_mut());
        if r == 0 { 0 } else { v }
    };
    let status = num(WINHTTP_QUERY_STATUS_CODE);
    if status != 200 {
        return Err(format!("El servidor respondió {status}: probá más tarde"));
    }
    let total = num(WINHTTP_QUERY_CONTENT_LENGTH) as usize;
    let mut out = Vec::with_capacity(total);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let mut n = 0u32;
        if WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut n) == 0 {
            return Err(OFFLINE.into());
        }
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
        progress(out.len(), total);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versiones() {
        assert!(newer("0.3.10", "0.3.9"));
        assert!(newer("0.4.0", "0.3.1"));
        assert!(!newer("0.3.1", "0.3.1"));
        assert!(!newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn manifiesto_valido() {
        let ok = serde_json::json!({ "version": "0.3.2", "url": "https://x.y/a.exe", "sha256": "A".repeat(64) });
        assert_eq!(parse(&ok).unwrap().sha256, "a".repeat(64));
        let http = serde_json::json!({ "version": "0.3.2", "url": "http://x.y/a.exe", "sha256": "a".repeat(64) });
        assert!(parse(&http).is_none());
        assert!(parse(&serde_json::json!({ "version": "0.3.2" })).is_none());
    }

    /// Baja un instalador publicado de verdad: `cargo test -- --ignored descarga_real`.
    #[test]
    #[ignore]
    fn descarga_real() {
        let mut ultimo = (0, 0);
        let d = unsafe { get("https://www.bertinilabs.xyz/ytm-float/ytm-float-setup-0.2.0.exe", &mut |a, b| ultimo = (a, b)) }.unwrap();
        assert_eq!(ultimo, (d.len(), d.len()));
        assert_eq!(sha256_hex(&d).unwrap(), "679ebdb160a2198f95bcd782e1fe2fe94bb98e2dec6ffa94e882ec1e95f727f4");
    }

    #[test]
    fn sha256_conocido() {
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
