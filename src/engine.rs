// Mantiene Brave headless + la sesion CDP. Todo llega a la UI por PostMessage:
// estado (push desde la pagina), respuestas a comandos y mensajes de estado.
// Opcional: una 2a pestaña oculta con Discord web, adjuntada como sesion "flatten" en la misma
// conexion (con --single-process una 2a conexion CDP no ve los contextos).
use crate::{brave, cdp};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering::SeqCst},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_ENGINE: u32 = WM_APP + 1;
const PAGE_JS: &str = include_str!("page.js");
const DISCORD_JS: &str = include_str!("discord.js");
pub const YTM_LOGIN: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/";
pub const DISCORD_LOGIN: &str = "https://discord.com/login";

pub enum Ev {
    State(Value),
    Reply(u32, Value),
    Status(String),
    /// Estado de voz de Discord (push de discord.js); {"off":true} si se desactivo.
    Discord(Value),
}

pub fn log(m: &str) {
    use std::io::Write;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(brave::data_dir().join("engine.log")) {
        let _ = writeln!(f, "{t} {m}");
    }
}

fn discord_flag() -> PathBuf {
    brave::data_dir().join("discord.on")
}

/// Pestaña de Discord: ids de los pedidos en vuelo y la sesion una vez adjuntada.
#[derive(Default)]
struct Dsc {
    create_id: u32,
    attach_id: u32,
    target: String,
    session: String,
}

pub struct Engine {
    hwnd: isize,
    ws: Mutex<Option<Arc<cdp::Ws>>>,
    ctx: AtomicI64,
    next: AtomicU32,
    tags: Mutex<HashMap<u32, u32>>,
    proc_: Mutex<Option<brave::Proc>>,
    login: Mutex<Option<String>>,
    exiting: AtomicBool,
    ua: Mutex<String>,
    dsc_on: AtomicBool,
    dsc: Mutex<Dsc>,
}

pub fn start(hwnd: isize) -> Arc<Engine> {
    let e = Arc::new(Engine {
        hwnd,
        ws: Mutex::new(None),
        ctx: AtomicI64::new(0),
        next: AtomicU32::new(1),
        tags: Mutex::new(HashMap::new()),
        proc_: Mutex::new(None),
        login: Mutex::new(None),
        exiting: AtomicBool::new(false),
        ua: Mutex::new(String::new()),
        dsc_on: AtomicBool::new(discord_flag().exists()),
        dsc: Mutex::new(Dsc::default()),
    });
    let e2 = e.clone();
    thread::spawn(move || e2.run());
    e
}

impl Engine {
    fn post(&self, ev: Ev) {
        let p = Box::into_raw(Box::new(ev));
        if unsafe { PostMessageW(self.hwnd as _, WM_ENGINE, 0, p as isize) } == 0 {
            drop(unsafe { Box::from_raw(p) });
        }
    }

    fn send(&self, id: u32, method: &str, params: Value, session: Option<&str>) {
        let ws = self.ws.lock().unwrap().clone();
        if let Some(ws) = ws {
            let mut m = json!({ "id": id, "method": method, "params": params });
            if let Some(s) = session {
                m["sessionId"] = json!(s);
            }
            let _ = ws.send(&m.to_string());
        }
    }

    fn raw(&self, method: &str, params: Value) {
        self.send(self.next.fetch_add(1, SeqCst), method, params, None);
    }

    fn raw_s(&self, method: &str, params: Value, session: &str) {
        self.send(self.next.fetch_add(1, SeqCst), method, params, Some(session));
    }

    /// Ejecuta window.__ytm(cmd, arg) en la pagina; la respuesta vuelve como Ev::Reply(tag, ..).
    pub fn call(&self, cmd: &str, arg: Value, tag: u32) {
        let ctx = self.ctx.load(SeqCst);
        if ctx == 0 {
            self.post(Ev::Reply(tag, json!({ "ok": false, "error": "Todavía cargando…" })));
            return;
        }
        let id = self.next.fetch_add(1, SeqCst);
        self.tags.lock().unwrap().insert(id, tag);
        let expr = format!("window.__ytm({},{})", json!(cmd), arg);
        self.send(id, "Runtime.evaluate", json!({
            "expression": expr, "contextId": ctx, "awaitPromise": true, "returnByValue": true,
        }), None);
    }

    /// Igual que `call` pero contra window.__dsc en la pestaña de Discord.
    pub fn call_dsc(&self, cmd: &str, arg: Value, tag: u32) {
        let session = self.dsc.lock().unwrap().session.clone();
        if session.is_empty() {
            self.post(Ev::Reply(tag, json!({ "ok": false, "error": "Discord todavía no cargó" })));
            return;
        }
        let id = self.next.fetch_add(1, SeqCst);
        self.tags.lock().unwrap().insert(id, tag);
        let expr = format!("window.__dsc ? window.__dsc({},{}) : ({{ok:false,error:'Discord todavía no cargó'}})", json!(cmd), arg);
        self.send(id, "Runtime.evaluate", json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }), Some(&session));
    }

    pub fn discord_on(&self) -> bool {
        self.dsc_on.load(SeqCst)
    }

    /// Activa/desactiva la pestaña de Discord (persistido). Desactivar no devuelve la RAM hasta
    /// reiniciar: Brave en un solo proceso no la suelta al cerrar la pestaña.
    pub fn set_discord(&self, on: bool) {
        self.dsc_on.store(on, SeqCst);
        if on {
            let _ = std::fs::create_dir_all(brave::data_dir());
            let _ = std::fs::write(discord_flag(), "");
            self.open_discord();
        } else {
            let _ = std::fs::remove_file(discord_flag());
            let target = std::mem::take(&mut *self.dsc.lock().unwrap()).target;
            if !target.is_empty() {
                self.raw("Target.closeTarget", json!({ "targetId": target }));
            }
            self.post(Ev::Discord(json!({ "off": true })));
        }
    }

    /// Crea la pestaña oculta; el resto (adjuntar, navegar, inyectar) sigue en el bucle al llegar las respuestas.
    fn open_discord(&self) {
        if !self.dsc_on.load(SeqCst) || self.ws.lock().unwrap().is_none() {
            return;
        }
        let mut d = self.dsc.lock().unwrap();
        if d.create_id != 0 || !d.target.is_empty() {
            return; // ya abierta o en camino
        }
        let id = self.next.fetch_add(1, SeqCst);
        d.create_id = id;
        drop(d);
        self.send(id, "Target.createTarget", json!({ "url": "about:blank", "background": true }), None);
    }

    /// Respuestas de la secuencia de apertura de Discord; true si la consumio.
    fn on_dsc_reply(&self, id: u32, v: &Value) -> bool {
        let mut d = self.dsc.lock().unwrap();
        if id == d.create_id && d.create_id != 0 {
            d.create_id = 0;
            let Some(t) = v["result"]["targetId"].as_str() else {
                log(&format!("discord createTarget: {}", v["error"]));
                return true;
            };
            d.target = t.to_string();
            let aid = self.next.fetch_add(1, SeqCst);
            d.attach_id = aid;
            let t = d.target.clone();
            drop(d);
            self.send(aid, "Target.attachToTarget", json!({ "targetId": t, "flatten": true }), None);
            return true;
        }
        if id == d.attach_id && d.attach_id != 0 {
            d.attach_id = 0;
            let Some(s) = v["result"]["sessionId"].as_str() else {
                log(&format!("discord attach: {}", v["error"]));
                return true;
            };
            d.session = s.to_string();
            drop(d);
            let ua = self.ua.lock().unwrap().clone();
            self.raw_s("Network.setUserAgentOverride", json!({ "userAgent": ua }), s);
            self.raw_s("Runtime.enable", json!({}), s);
            self.raw_s("Runtime.addBinding", json!({ "name": "__dscEvt" }), s);
            // Microfono solo para discord.com, sin preguntar (headless no tiene donde preguntar).
            self.raw("Browser.grantPermissions", json!({ "origin": "https://discord.com", "permissions": ["audioCapture"] }));
            self.raw_s("Page.navigate", json!({ "url": "https://discord.com/channels/@me" }), s);
            return true;
        }
        false
    }

    pub fn login(&self, url: &str) {
        *self.login.lock().unwrap() = Some(url.to_string());
        if let Some(p) = self.proc_.lock().unwrap().take() {
            p.kill();
            p.wait(3000);
        }
    }

    pub fn shutdown(&self) {
        self.exiting.store(true, SeqCst);
        if let Some(p) = self.proc_.lock().unwrap().take() {
            self.raw("Browser.close", json!({})); // cierre limpio: guarda cookies
            if !p.wait(1500) {
                p.kill();
            }
        }
    }

    fn run(self: Arc<Self>) {
        let Some(exe) = brave::find_brave() else {
            self.post(Ev::Status("No encontré Brave instalado".into()));
            return;
        };
        while !self.exiting.load(SeqCst) {
            let login = self.login.lock().unwrap().clone();
            if let Some(url) = login {
                self.post(Ev::Status("Iniciá sesión en la ventana de Brave y cerrala al terminar".into()));
                match brave::launch_login(&exe, &url) {
                    Ok(p) => {
                        p.wait(u32::MAX);
                    }
                    Err(e) => self.post(Ev::Status(format!("Error al abrir Brave: {e}"))),
                }
                *self.login.lock().unwrap() = None;
                continue;
            }
            self.post(Ev::Status("Iniciando YouTube Music…".into()));
            if let Err(e) = self.session(&exe) {
                log(&format!("sesión: {e}"));
                if !self.exiting.load(SeqCst) && self.login.lock().unwrap().is_none() {
                    self.post(Ev::Status(format!("Reconectando ({e})")));
                    thread::sleep(Duration::from_secs(2));
                }
            }
            self.ctx.store(0, SeqCst);
            *self.dsc.lock().unwrap() = Dsc::default();
            *self.ws.lock().unwrap() = None;
            if let Some(p) = self.proc_.lock().unwrap().take() {
                p.kill();
                p.wait(3000);
            }
        }
    }

    fn session(&self, exe: &Path) -> io::Result<()> {
        let (proc_, port) = brave::launch_headless(exe)?;
        *self.proc_.lock().unwrap() = Some(proc_);

        let targets: Value = serde_json::from_str(&brave::http_get(port, "/json/list")?)?;
        let page = targets
            .as_array()
            .and_then(|a| a.iter().find(|t| t["type"] == "page"))
            .ok_or_else(|| io::Error::other("sin pestaña"))?;
        let target = page["id"].as_str().unwrap_or_default().to_string();
        let url = page["webSocketDebuggerUrl"].as_str().unwrap_or_default();
        let path = url.find("/devtools/").map(|i| &url[i..]).unwrap_or_default();
        let (ws, mut rd) = cdp::connect(port, path)?;
        let ws = Arc::new(ws);
        *self.ws.lock().unwrap() = Some(ws.clone());

        // Headless se anuncia como "HeadlessChrome" y YT Music lo rechaza: se presenta como Brave normal.
        let ver: Value = serde_json::from_str(&brave::http_get(port, "/json/version")?)?;
        let ua = ver["User-Agent"].as_str().unwrap_or_default().replace("HeadlessChrome", "Chrome");
        *self.ua.lock().unwrap() = ua.clone();
        self.raw("Network.setUserAgentOverride", json!({ "userAgent": ua }));
        self.raw("Inspector.enable", json!({}));
        self.raw("Runtime.enable", json!({}));
        self.raw("Runtime.addBinding", json!({ "name": "__ytmEvt" }));
        self.raw("Page.addScriptToEvaluateOnNewDocument", json!({ "source": PAGE_JS }));
        self.raw("Page.navigate", json!({ "url": "https://music.youtube.com/" }));
        self.open_discord();

        loop {
            let msg = cdp::read_msg(&mut rd, &ws)?;
            let Ok(v) = serde_json::from_str::<Value>(&msg) else { continue };
            let session = v["sessionId"].as_str();
            if let Some(m) = v["method"].as_str() {
                match (m, session) {
                    // Con --single-process el script de addScriptToEvaluateOnNewDocument no siempre
                    // corre: se inyecta a mano en cada contexto nuevo del frame principal (es idempotente).
                    ("Runtime.executionContextCreated", None) => {
                        let c = &v["params"]["context"];
                        if c["auxData"]["isDefault"] == true && c["auxData"]["frameId"] == target.as_str() {
                            let id = c["id"].as_i64().unwrap_or(0);
                            self.ctx.store(id, SeqCst);
                            self.raw("Runtime.evaluate", json!({ "contextId": id, "expression": PAGE_JS }));
                        }
                    }
                    // Pestaña de Discord: mismo criterio, script propio.
                    ("Runtime.executionContextCreated", Some(s)) => {
                        let c = &v["params"]["context"];
                        let d = self.dsc.lock().unwrap();
                        if s == d.session && c["auxData"]["isDefault"] == true && c["auxData"]["frameId"] == d.target.as_str() {
                            drop(d);
                            let id = c["id"].as_i64().unwrap_or(0);
                            self.raw_s("Runtime.evaluate", json!({ "contextId": id, "expression": DISCORD_JS }), s);
                        }
                    }
                    ("Runtime.bindingCalled", None) if v["params"]["name"] == "__ytmEvt" => {
                        if let Ok(s) = serde_json::from_str(v["params"]["payload"].as_str().unwrap_or("")) {
                            self.post(Ev::State(s));
                        }
                    }
                    ("Runtime.bindingCalled", Some(_)) if v["params"]["name"] == "__dscEvt" => {
                        if let Ok(s) = serde_json::from_str(v["params"]["payload"].as_str().unwrap_or("")) {
                            self.post(Ev::Discord(s));
                        }
                    }
                    // La pestaña de Discord se cerro o murio: se vuelve a abrir si sigue activado.
                    ("Target.detachedFromTarget", _) => {
                        let gone = v["params"]["sessionId"].as_str() == Some(self.dsc.lock().unwrap().session.as_str());
                        if gone {
                            *self.dsc.lock().unwrap() = Dsc::default();
                            log("discord: pestaña desconectada");
                            self.open_discord();
                        }
                    }
                    ("Inspector.targetCrashed", None) => return Err(io::Error::other("pestaña caída")),
                    _ => {}
                }
            } else if let Some(id) = v["id"].as_u64() {
                let id = id as u32;
                if self.on_dsc_reply(id, &v) {
                    continue;
                }
                if let Some(tag) = self.tags.lock().unwrap().remove(&id) {
                    let r = &v["result"]["result"]["value"];
                    let out = if r.is_object() {
                        r.clone()
                    } else {
                        let e = v["error"]["message"].as_str()
                            .or(v["result"]["exceptionDetails"]["text"].as_str())
                            .unwrap_or("sin respuesta");
                        json!({ "ok": false, "error": e })
                    };
                    self.post(Ev::Reply(tag, out));
                }
            }
        }
    }
}
