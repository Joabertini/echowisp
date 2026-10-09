// "Reportar un problema": el texto del usuario (y, si lo deja, un resumen tecnico) se manda por HTTPS a
// reportes.bertinilabs.xyz, que lo guarda y avisa al desarrollador. WinHTTP del sistema: sin dependencias.
use crate::brave::{self, wide};
use serde_json::{json, Value};
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Networking::WinHttp::*,
    System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const WM_REPORT: u32 = WM_APP + 4;
const HOST: &str = "reportes.bertinilabs.xyz";
const PATH: &str = "/v1/reporte";
pub const MAX_TEXT: usize = 4000;
const LOG_LINES: usize = 80;

#[derive(Clone, Copy, PartialEq)]
pub enum Field {
    Text,
    Contact,
}

/// Lo que se va escribiendo en la card de costado.
pub struct Form {
    pub text: String,
    pub contact: String,
    pub focus: Option<Field>,
    pub attach: bool, // adjuntar version de Windows y el final de engine.log
    pub sending: bool,
    pub error: String,
}

impl Form {
    pub fn new() -> Form {
        Form { text: String::new(), contact: String::new(), focus: None, attach: true, sending: false, error: String::new() }
    }

    pub fn ready(&self) -> bool {
        !self.sending && self.text.trim().chars().count() >= 3
    }

    /// Caracter tecleado en el campo con foco. Tab pasa al otro campo, Esc suelta el foco.
    pub fn type_char(&mut self, c: u16, paste: impl FnOnce() -> String) {
        let Some(f) = self.focus else { return };
        let max = if f == Field::Text { MAX_TEXT } else { 200 };
        let buf = if f == Field::Text { &mut self.text } else { &mut self.contact };
        match c {
            8 => {
                buf.pop();
            }
            9 => self.focus = Some(if f == Field::Text { Field::Contact } else { Field::Text }),
            27 => self.focus = None,
            13 if f == Field::Text => buf.push('\n'),
            13 => self.focus = None,
            22 => {
                let p: String = paste().replace("\r\n", "\n").chars().filter(|&c| c == '\n' && f == Field::Text || !c.is_control()).collect();
                buf.extend(p.chars().take(max.saturating_sub(buf.chars().count())));
            }
            c if c >= 32 && !(0xD800..0xE000).contains(&c) => {
                if buf.chars().count() < max {
                    buf.extend(char::from_u32(c as u32));
                }
            }
            _ => {}
        }
        self.error.clear();
    }
}

fn reg_str(sub: &str, name: &str) -> String {
    let (sub, name) = (wide(sub), wide(name));
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, sub.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut len) };
    if r != 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)])
}

/// "Windows 10 Pro 22H2 (build 19045)". ProductName dice 10 tambien en 11: el build lo aclara.
fn windows() -> String {
    let k = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    format!("{} {} (build {})", reg_str(k, "ProductName"), reg_str(k, "DisplayVersion"), reg_str(k, "CurrentBuild"))
}

/// Ultimas lineas de la sesion actual; si son pocas, completa con el final de la anterior (por si se cerro
/// por el problema y la reabrieron para reportar). Sin la carpeta del usuario (puede llevar su nombre).
fn log_tail() -> String {
    let leer = |f: &str| std::fs::read_to_string(brave::data_dir().join(f)).unwrap_or_default();
    let (actual, previa) = (leer("engine.log"), leer("engine.prev.log"));
    let actual: Vec<&str> = actual.lines().collect();
    let previa: Vec<&str> = previa.lines().collect();
    let falta = LOG_LINES.saturating_sub(actual.len());
    let mut lines = Vec::new();
    if falta > 0 && !previa.is_empty() {
        lines.push("--- sesión anterior ---");
        lines.extend(&previa[previa.len().saturating_sub(falta)..]);
        lines.push("--- sesión actual ---");
    }
    lines.extend(&actual[actual.len().saturating_sub(LOG_LINES)..]);
    let mut t = lines.join("\n");
    for v in ["USERPROFILE", "USERNAME"] {
        if let Some(p) = std::env::var(v).ok().filter(|p| p.len() > 2) {
            t = t.replace(&p, &format!("%{v}%"));
        }
    }
    t
}

pub fn body(f: &Form) -> Value {
    let mut b = json!({
        "app": "echowisp",
        "version": env!("CARGO_PKG_VERSION"),
        "texto": f.text.trim(),
        "contacto": f.contact.trim(),
    });
    if f.attach {
        b["sistema"] = json!(windows());
        b["log"] = json!(log_tail());
    }
    b
}

/// POST en un hilo aparte; la respuesta vuelve a la card como WM_REPORT (wparam 1 = ok, lparam = Box<String> con el error).
pub fn send(hwnd: isize, body: Value) {
    std::thread::spawn(move || {
        let r = unsafe { post(&body.to_string()) };
        let (ok, msg) = match r {
            Ok(200) => (1, String::new()),
            Ok(429) => (0, crate::i18n::t("report_rate").to_string()),
            Ok(s) => (0, format!("{} {s}: {}", crate::i18n::t("server_replied"), crate::i18n::t("try_later"))),
            Err(()) => (0, crate::i18n::t("offline").to_string()),
        };
        let p = Box::into_raw(Box::new(msg));
        if unsafe { PostMessageW(hwnd as _, WM_REPORT, ok, p as isize) } == 0 {
            drop(unsafe { Box::from_raw(p) });
        }
    });
}

/// Codigo HTTP de la respuesta, o Err si no hubo conexion.
unsafe fn post(body: &str) -> Result<u32, ()> {
    struct H(*mut core::ffi::c_void);
    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    let ok = |h: *mut core::ffi::c_void| if h.is_null() { Err(()) } else { Ok(H(h)) };
    let agent = wide(concat!("echowisp/", env!("CARGO_PKG_VERSION")));
    let ses = ok(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
    WinHttpSetTimeouts(ses.0, 10_000, 10_000, 15_000, 15_000);
    let con = ok(WinHttpConnect(ses.0, wide(HOST).as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
    let req = ok(WinHttpOpenRequest(con.0, wide("POST").as_ptr(), wide(PATH).as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE))?;
    let hdr = wide("Content-Type: application/json; charset=utf-8\r\n");
    let b = body.as_bytes();
    if WinHttpSendRequest(req.0, hdr.as_ptr(), u32::MAX, b.as_ptr().cast(), b.len() as u32, b.len() as u32, 0) == 0 || WinHttpReceiveResponse(req.0, null_mut()) == 0 {
        return Err(());
    }
    let mut status = 0u32;
    let mut len = 4u32;
    WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, null(), (&mut status as *mut u32).cast(), &mut len, null_mut());
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(f: &mut Form, s: &str) {
        for c in s.encode_utf16() {
            f.type_char(c, String::new);
        }
    }

    #[test]
    fn escribe_borra_y_cambia_de_campo() {
        let mut f = Form::new();
        assert!(!f.ready());
        f.focus = Some(Field::Text);
        typed(&mut f, "no suena\r");
        typed(&mut f, "ñandú");
        f.type_char(8, String::new);
        assert_eq!(f.text, "no suena\nñand");
        f.type_char(9, String::new);
        typed(&mut f, "a@b.uy\r");
        assert_eq!(f.contact, "a@b.uy");
        assert!(f.focus.is_none());
        assert!(f.ready());
    }

    #[test]
    fn pegar_respeta_el_tope_y_los_saltos() {
        let mut f = Form::new();
        f.focus = Some(Field::Contact);
        f.type_char(22, || "x@y.com\r\notra".into());
        assert_eq!(f.contact, "x@y.comotra");
        f.focus = Some(Field::Text);
        f.type_char(22, || "a\r\nb\t".into());
        assert_eq!(f.text, "a\nb");
        f.type_char(22, || "z".repeat(MAX_TEXT * 2));
        assert_eq!(f.text.chars().count(), MAX_TEXT);
    }

    #[test]
    fn el_log_no_lleva_la_carpeta_del_usuario() {
        let home = std::env::var("USERPROFILE").unwrap();
        assert!(!log_tail().contains(&home));
    }

    /// Manda un reporte de verdad: `cargo test -- --ignored envio_real`.
    #[test]
    #[ignore]
    fn envio_real() {
        let mut f = Form::new();
        f.text = "Prueba automática del envío desde la app".into();
        assert_eq!(unsafe { post(&body(&f).to_string()) }, Ok(200));
    }
}
