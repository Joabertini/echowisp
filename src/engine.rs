// Mantiene Brave headless + la sesion CDP. Todo llega a la UI por PostMessage:
// estado (push desde la pagina), respuestas a comandos y mensajes de estado.
use crate::{brave, cdp};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io,
    path::Path,
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

pub enum Ev {
    State(Value),
    Reply(u32, Value),
    Status(String),
}

fn log(m: &str) {
    use std::io::Write;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(brave::data_dir().join("engine.log")) {
        let _ = writeln!(f, "{t} {m}");
    }
}

pub struct Engine {
    hwnd: isize,
    ws: Mutex<Option<Arc<cdp::Ws>>>,
    ctx: AtomicI64,
    next: AtomicU32,
    tags: Mutex<HashMap<u32, u32>>,
    proc_: Mutex<Option<brave::Proc>>,
    login: AtomicBool,
    exiting: AtomicBool,
}

pub fn start(hwnd: isize) -> Arc<Engine> {
    let e = Arc::new(Engine {
        hwnd,
        ws: Mutex::new(None),
        ctx: AtomicI64::new(0),
        next: AtomicU32::new(1),
        tags: Mutex::new(HashMap::new()),
        proc_: Mutex::new(None),
        login: AtomicBool::new(false),
        exiting: AtomicBool::new(false),
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

    fn send(&self, id: u32, method: &str, params: Value) {
        let ws = self.ws.lock().unwrap().clone();
        if let Some(ws) = ws {
            let _ = ws.send(&json!({ "id": id, "method": method, "params": params }).to_string());
        }
    }

    fn raw(&self, method: &str, params: Value) {
        self.send(self.next.fetch_add(1, SeqCst), method, params);
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
        }));
    }

    pub fn login(&self) {
        self.login.store(true, SeqCst);
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
            if self.login.load(SeqCst) {
                self.post(Ev::Status("Iniciá sesión en la ventana de Brave y cerrala al terminar".into()));
                match brave::launch_login(&exe) {
                    Ok(p) => {
                        p.wait(u32::MAX);
                    }
                    Err(e) => self.post(Ev::Status(format!("Error al abrir Brave: {e}"))),
                }
                self.login.store(false, SeqCst);
                continue;
            }
            self.post(Ev::Status("Iniciando YouTube Music…".into()));
            if let Err(e) = self.session(&exe) {
                log(&format!("sesión: {e}"));
                if !self.exiting.load(SeqCst) && !self.login.load(SeqCst) {
                    self.post(Ev::Status(format!("Reconectando ({e})")));
                    thread::sleep(Duration::from_secs(2));
                }
            }
            self.ctx.store(0, SeqCst);
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
        self.raw("Network.setUserAgentOverride", json!({ "userAgent": ua }));
        self.raw("Inspector.enable", json!({}));
        self.raw("Runtime.enable", json!({}));
        self.raw("Runtime.addBinding", json!({ "name": "__ytmEvt" }));
        self.raw("Page.addScriptToEvaluateOnNewDocument", json!({ "source": PAGE_JS }));
        self.raw("Page.navigate", json!({ "url": "https://music.youtube.com/" }));

        loop {
            let msg = cdp::read_msg(&mut rd, &ws)?;
            let Ok(v) = serde_json::from_str::<Value>(&msg) else { continue };
            if let Some(m) = v["method"].as_str() {
                match m {
                    // Con --single-process el script de addScriptToEvaluateOnNewDocument no siempre
                    // corre: se inyecta a mano en cada contexto nuevo del frame principal (es idempotente).
                    "Runtime.executionContextCreated" => {
                        let c = &v["params"]["context"];
                        if c["auxData"]["isDefault"] == true && c["auxData"]["frameId"] == target.as_str() {
                            let id = c["id"].as_i64().unwrap_or(0);
                            self.ctx.store(id, SeqCst);
                            self.raw("Runtime.evaluate", json!({ "contextId": id, "expression": PAGE_JS }));
                        }
                    }
                    "Runtime.bindingCalled" if v["params"]["name"] == "__ytmEvt" => {
                        if let Ok(s) = serde_json::from_str(v["params"]["payload"].as_str().unwrap_or("")) {
                            self.post(Ev::State(s));
                        }
                    }
                    "Inspector.targetCrashed" => return Err(io::Error::other("pestaña caída")),
                    _ => {}
                }
            } else if let Some(id) = v["id"].as_u64() {
                if let Some(tag) = self.tags.lock().unwrap().remove(&(id as u32)) {
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
