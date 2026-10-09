//! Catálogo de sesiones, selección de procesos y ducking reversible.
//! El único hilo COM que cambia volumen conserva el estado anterior en ruteo.json.
use crate::{audio_mix, process_capture, serve::emit};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, ffi::c_void, path::PathBuf, sync::{mpsc, Arc}, time::{Duration, Instant}};
use windows::{core::{h, implement, interface, GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, Interface, PCWSTR, PWSTR}, Win32::{Foundation::*, Media::Audio::*, System::{Com::*, Diagnostics::ToolHelp::*, Threading::*, WinRT::RoGetActivationFactory}}};

const DUCK: f32 = 0.0001;
// Revisión de apps abiertas/cerradas. El audio no pasa por este hilo.
const SCAN: Duration = Duration::from_secs(1);
const EPSILON: f32 = 0.000001;
const CONTEXT: GUID = GUID::from_u128(0x33a0c180_4b67_4333_86c0_0cbdf5f11b87);

// Se conserva Policy solo para reparar ruteos pendientes de versiones anteriores.
macro_rules! policy {
    ($name:ident, $iid:literal) => {
        #[interface($iid)]
        unsafe trait $name: IUnknown {
            unsafe fn _get_iids(&self) -> HRESULT; unsafe fn _get_runtime_class_name(&self) -> HRESULT;
            unsafe fn _get_trust_level(&self) -> HRESULT;
            unsafe fn _m0(&self) -> HRESULT; unsafe fn _m1(&self) -> HRESULT;
            unsafe fn _m2(&self) -> HRESULT; unsafe fn _m3(&self) -> HRESULT;
            unsafe fn _m4(&self) -> HRESULT; unsafe fn _m5(&self) -> HRESULT;
            unsafe fn _m6(&self) -> HRESULT; unsafe fn _m7(&self) -> HRESULT;
            unsafe fn _m8(&self) -> HRESULT; unsafe fn _m9(&self) -> HRESULT;
            unsafe fn _m10(&self) -> HRESULT; unsafe fn _m11(&self) -> HRESULT;
            unsafe fn _m12(&self) -> HRESULT; unsafe fn _m13(&self) -> HRESULT;
            unsafe fn _m14(&self) -> HRESULT; unsafe fn _m15(&self) -> HRESULT;
            unsafe fn _m16(&self) -> HRESULT; unsafe fn _m17(&self) -> HRESULT;
            unsafe fn _m18(&self) -> HRESULT;
            unsafe fn set(&self, pid: u32, flow: EDataFlow, role: ERole, id: *mut c_void) -> HRESULT;
        }
    };
}
policy!(IPolicyNew, "ab3d4648-e242-459f-b02f-541c70306324");
policy!(IPolicyOld, "2a59116d-6c4f-45e0-a74f-707e3fef9258");
enum Policy { New(IPolicyNew), Old(IPolicyOld) }
impl Policy {
    fn new() -> Option<Self> {
        unsafe { RoGetActivationFactory::<IPolicyNew>(h!("Windows.Media.Internal.AudioPolicyConfig"))
            .map(Self::New).or_else(|_| RoGetActivationFactory::<IPolicyOld>(h!("Windows.Media.Internal.AudioPolicyConfig")).map(Self::Old)).ok() }
    }
    fn restore(&self, pid: u32, dev: &str) -> windows::core::Result<()> {
        const TOKEN: &str = r"\\?\SWD#MMDEVAPI#";
        const RENDER: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";
        let h = (!dev.is_empty()).then(|| HSTRING::from(format!("{TOKEN}{dev}{RENDER}")));
        let raw: *mut c_void = h.as_ref().map_or(std::ptr::null_mut(), |h| unsafe { std::mem::transmute_copy(h) });
        for role in [eMultimedia, eConsole] {
            unsafe { match self { Policy::New(p) => p.set(pid, eRender, role, raw), Policy::Old(p) => p.set(pid, eRender, role, raw) }.ok()?; }
        }
        Ok(())
    }
}

fn data_path() -> PathBuf {
    crate::data_dir().join("ruteo.json")
}

fn pid_file(name: &str) -> u32 {
    std::fs::read_to_string(data_path().with_file_name(name)).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let result = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).ok()
            .map(|_| String::from_utf16_lossy(&buf[..n as usize]));
        let _ = CloseHandle(h);
        result
    }
}

fn process_birth(pid: u32) -> Option<u64> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user).ok()
            .map(|_| ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64);
        let _ = CloseHandle(h);
        result
    }
}

fn parent_map() -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut item = PROCESSENTRY32W::default();
        item.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut item).is_ok() {
            loop {
                out.insert(item.th32ProcessID, item.th32ParentProcessID);
                if Process32NextW(snapshot, &mut item).is_err() { break; }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

fn descendant(mut child: u32, ancestor: u32, parents: &HashMap<u32, u32>) -> bool {
    for _ in 0..64 {
        if child == ancestor { return true; }
        let Some(&parent) = parents.get(&child) else { return false };
        if parent == child || parent == 0 { return false; }
        child = parent;
    }
    false
}

#[implement(IAudioSessionEvents)]
struct SessionEvents { id: String, external: mpsc::Sender<String> }

impl IAudioSessionEvents_Impl for SessionEvents {
    fn OnDisplayNameChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> { Ok(()) }
    fn OnIconPathChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> { Ok(()) }
    fn OnSimpleVolumeChanged(&self, _: f32, _: BOOL, context: *const GUID) -> windows::core::Result<()> {
        if context.is_null() || unsafe { *context != CONTEXT } { let _ = self.external.send(self.id.clone()); }
        Ok(())
    }
    fn OnChannelVolumeChanged(&self, _: u32, _: *const f32, _: u32, _: *const GUID) -> windows::core::Result<()> { Ok(()) }
    fn OnGroupingParamChanged(&self, _: *const GUID, _: *const GUID) -> windows::core::Result<()> { Ok(()) }
    fn OnStateChanged(&self, _: AudioSessionState) -> windows::core::Result<()> { Ok(()) }
    fn OnSessionDisconnected(&self, _: AudioSessionDisconnectReason) -> windows::core::Result<()> {
        let _ = self.external.send(self.id.clone());
        Ok(())
    }
}

struct Subscription { pid: u32, control: IAudioSessionControl, sink: IAudioSessionEvents }
impl Drop for Subscription {
    fn drop(&mut self) { let _ = unsafe { self.control.UnregisterAudioSessionNotification(&self.sink) }; }
}

struct Session { id: String, vol: ISimpleAudioVolume, control: IAudioSessionControl }
struct App { pid: u32, birth: u64, exe: String, name: String, sessions: Vec<Session> }

fn apps(en: &IMMDeviceEnumerator) -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    let (ytm, discord, me) = (pid_file("brave.pid"), pid_file("discord.pid"), std::process::id());
    // Chromium suena desde un proceso hijo (servicio de audio): se reconoce por árbol, no por PID.
    let parents = if ytm != 0 || discord != 0 { parent_map() } else { HashMap::new() };
    unsafe {
        let Ok(devs) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..devs.GetCount().unwrap_or(0) {
            let Ok(d) = devs.Item(i) else { continue };
            let Ok(sm) = d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(ss) = sm.GetSessionEnumerator() else { continue };
            for j in 0..ss.GetCount().unwrap_or(0) {
                let Ok(c2) = ss.GetSession(j).and_then(|c| c.cast::<IAudioSessionControl2>()) else { continue };
                if c2.IsSystemSoundsSession() == S_OK || c2.GetState().map_or(true, |s| s == AudioSessionStateExpired) { continue; }
                let pid = c2.GetProcessId().unwrap_or(0);
                if pid == 0 || pid == me || (discord != 0 && descendant(pid, discord, &parents)) { continue; }
                let (Some(exe), Some(birth)) = (exe_path(pid), process_birth(pid)) else { continue };
                let stem = std::path::Path::new(&exe).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if stem.to_lowercase().contains("discord") { continue; }
                let Ok(vol) = c2.cast::<ISimpleAudioVolume>() else { continue };
                let Ok(control) = c2.cast::<IAudioSessionControl>() else { continue };
                let Ok(ptr) = c2.GetSessionInstanceIdentifier() else { continue };
                let id = ptr.to_string().unwrap_or_default();
                CoTaskMemFree(Some(ptr.0 as _));
                match out.iter_mut().find(|a| a.pid == pid) {
                    Some(a) => a.sessions.push(Session { id, vol, control }),
                    None => out.push(App { pid, birth, exe, name: if ytm != 0 && descendant(pid, ytm, &parents) { "Música (Echowisp)".into() } else { stem }, sessions: vec![Session { id, vol, control }] }),
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[derive(Clone)]
struct Saved { pid: u32, exe: String, original: f32 }
#[derive(Default)]
struct Persist { ducked: HashMap<String, Saved>, legacy: HashMap<String, String> }

impl Persist {
    fn load() -> Self {
        let v: Value = std::fs::read_to_string(data_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
        let mut p = Self::default();
        if v["version"] == 2 {
            if let Some(entries) = v["ducked"].as_object() {
                for (id, x) in entries {
                    if let (Some(pid), Some(exe), Some(original)) = (x["pid"].as_u64(), x["exe"].as_str(), x["original"].as_f64()) {
                        p.ducked.insert(id.clone(), Saved { pid: pid as u32, exe: exe.into(), original: original as f32 });
                    }
                }
            }
            if let Some(entries) = v["legacy"].as_object() {
                for (exe, dev) in entries { if let Some(dev) = dev.as_str() { p.legacy.insert(exe.clone(), dev.into()); } }
            }
        } else if let Some(entries) = v.as_object() {
            for (exe, dev) in entries { if let Some(dev) = dev.as_str() { p.legacy.insert(exe.clone(), dev.into()); } }
        }
        p
    }
    fn save(&self) -> Result<(), String> {
        let path = data_path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let ducked: serde_json::Map<String, Value> = self.ducked.iter().map(|(id, x)| (id.clone(), json!({ "pid": x.pid, "exe": x.exe, "original": x.original }))).collect();
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&json!({ "version": 2, "ducked": ducked, "legacy": self.legacy })).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

trait VolumeBackend {
    fn get(&self) -> Result<f32, String>;
    fn muted(&self) -> Result<bool, String>;
    fn set(&self, value: f32) -> Result<(), String>;
}

impl VolumeBackend for ISimpleAudioVolume {
    fn get(&self) -> Result<f32, String> { unsafe { self.GetMasterVolume().map_err(|e| e.to_string()) } }
    fn muted(&self) -> Result<bool, String> { unsafe { self.GetMute().map(|v| v.as_bool()).map_err(|e| e.to_string()) } }
    fn set(&self, value: f32) -> Result<(), String> { unsafe { self.SetMasterVolume(value, &CONTEXT).map_err(|e| e.to_string()) } }
}

fn still_ducked(vol: &impl VolumeBackend) -> bool {
    vol.get().is_ok_and(|v| (v - DUCK).abs() <= EPSILON)
}

fn restore_volume(p: &mut Persist, id: &str, vol: &impl VolumeBackend) {
    if let Some(saved) = p.ducked.get(id) {
        match vol.get() {
            Ok(value) if (value - DUCK).abs() > EPSILON => { p.ducked.remove(id); }
            Ok(_) => { if vol.set(saved.original).is_ok() { p.ducked.remove(id); } }
            Err(_) => {} // conservar recuperación pendiente si el dispositivo falló
        }
    }
}

fn restore_session(p: &mut Persist, s: &Session) { restore_volume(p, &s.id, &s.vol); }

fn duck_volume(p: &mut Persist, id: &str, pid: u32, exe: &str, name: &str, vol: &impl VolumeBackend, persist: bool) -> Result<(), String> {
    if p.ducked.contains_key(id) { return Ok(()); }
    let original = vol.get()?;
    if vol.muted()? { return Err(format!("{name} está silenciada en Windows")); }
    if original <= 0.0 { return Err(format!("{name} tiene volumen cero en Windows")); }
    p.ducked.insert(id.into(), Saved { pid, exe: exe.into(), original });
    if persist {
        if let Err(e) = p.save() { p.ducked.remove(id); return Err(e); }
    } // write-ahead: un cierre forzado podrá restaurar
    if let Err(e) = vol.set(DUCK) {
        p.ducked.remove(id);
        if persist { let _ = p.save(); }
        return Err(format!("no pude bajar el volumen de {name}: {e}"));
    }
    Ok(())
}

fn duck_session(p: &mut Persist, app: &App, s: &Session) -> Result<(), String> {
    duck_volume(p, &s.id, app.pid, &app.exe, &app.name, &s.vol, true)
}

pub enum Command { Json(Value), Active(bool, mpsc::Sender<Result<(), String>>) }

struct Selection { exe: String, birth: u64, gain: f32, capture: Option<process_capture::Capture>, reconnect_deadline: Option<Instant> }
struct Mixer {
    en: IMMDeviceEnumerator,
    saved: Persist,
    selected: HashMap<u32, Selection>,
    subscriptions: HashMap<String, Subscription>,
    external_tx: mpsc::Sender<String>,
    external_rx: mpsc::Receiver<String>,
    external_changes: HashSet<String>,
    audio: Arc<audio_mix::Shared>,
    active: bool,
    last_scan: Instant,
    last_list: String,
}

impl Mixer {
    fn subscribe(&mut self, pid: u32, session: &Session) -> Result<(), String> {
        if self.subscriptions.contains_key(&session.id) { return Ok(()); }
        let sink: IAudioSessionEvents = SessionEvents { id: session.id.clone(), external: self.external_tx.clone() }.into();
        unsafe { session.control.RegisterAudioSessionNotification(&sink) }.map_err(|e| format!("notificación de volumen: {e}"))?;
        self.subscriptions.insert(session.id.clone(), Subscription { pid, control: session.control.clone(), sink });
        Ok(())
    }

    fn recover(&mut self, current: &[App]) {
        for app in current {
            for s in &app.sessions {
                if self.saved.ducked.get(&s.id).is_some_and(|v| v.exe == app.exe && v.pid == app.pid) {
                    restore_session(&mut self.saved, s);
                }
            }
        }
        if !self.saved.legacy.is_empty() {
            if let Some(policy) = Policy::new() {
                for app in current {
                    if let Some(dev) = self.saved.legacy.get(&app.exe) {
                        if policy.restore(app.pid, dev).is_ok() { self.saved.legacy.remove(&app.exe); }
                    }
                }
            }
        }
        let _ = self.saved.save();
    }

    /// Emite la lista solo si cambió (o si la card la pide con `force`).
    fn list_if(&mut self, force: bool) {
        let current = apps(&self.en);
        self.list_from(&current, force);
    }

    fn list_from(&mut self, current: &[App], force: bool) {
        let list: Vec<Value> = current.iter().map(|a| json!({
            "pid": a.pid, "name": a.name, "on": self.selected.contains_key(&a.pid),
            "vol": (self.selected.get(&a.pid).map_or(1.0, |s| s.gain) * 100.0).round(),
        })).collect();
        let ev = json!({ "ev": "apps", "available": true, "mode": "process_loopback", "list": list });
        let text = ev.to_string();
        if force || text != self.last_list { emit(ev); self.last_list = text; }
    }

    fn list(&mut self) { self.list_if(false) }

    fn start_one(&mut self, pid: u32, app: &App) -> Result<(), String> {
        let cap = process_capture::open(pid, self.audio.clone(), 1.0 / DUCK)?;
        for s in &app.sessions {
            if let Err(e) = self.subscribe(pid, s).and_then(|_| duck_session(&mut self.saved, app, s)) {
                for done in &app.sessions { restore_session(&mut self.saved, done); }
                self.subscriptions.retain(|_, subscription| subscription.pid != pid);
                let _ = self.saved.save();
                return Err(e);
            }
        }
        let selection = self.selected.get_mut(&pid).unwrap();
        selection.capture = Some(cap);
        selection.reconnect_deadline = None;
        let gain = self.selected[&pid].gain;
        self.audio.with(|m| m.add(pid, gain));
        Ok(())
    }

    fn stop_one(&mut self, pid: u32) {
        if let Some(s) = self.selected.get_mut(&pid) { s.capture.take(); }
        while let Ok(id) = self.external_rx.try_recv() { self.external_changes.insert(id); }
        self.subscriptions.retain(|_, subscription| subscription.pid != pid);
        self.audio.with(|m| m.remove(pid));
        for app in apps(&self.en).iter().filter(|a| a.pid == pid) {
            for sess in &app.sessions {
                if self.external_changes.contains(&sess.id) && !sess.vol.muted().unwrap_or(false) {
                    self.saved.ducked.remove(&sess.id);
                }
                restore_session(&mut self.saved, sess);
            }
        }
        let _ = self.saved.save();
    }

    fn active(&mut self, on: bool) -> Result<(), String> {
        if !on {
            let pids: Vec<u32> = self.selected.keys().copied().collect();
            for pid in pids { self.stop_one(pid); }
            self.active = false;
            return Ok(());
        }
        self.active = true;
        let current = apps(&self.en);
        for pid in self.selected.keys().copied().collect::<Vec<_>>() {
            let Some(app) = current.iter().find(|a| a.pid == pid && a.exe == self.selected[&pid].exe && a.birth == self.selected[&pid].birth) else {
                self.active(false)?;
                return Err(format!("la app {pid} ya no tiene sesión de audio activa"));
            };
            if let Err(e) = self.start_one(pid, app) { self.active(false)?; return Err(e); }
        }
        Ok(())
    }

    fn route(&mut self, pid: u32, on: bool) {
        if !on {
            self.stop_one(pid);
            self.selected.remove(&pid);
            self.list();
            return;
        }
        let current = apps(&self.en);
        let Some(app) = current.iter().find(|a| a.pid == pid) else {
            emit(json!({ "ev": "mix_error", "msg": "la app no tiene salida de audio compartida activa (quizá usa modo exclusivo)" }));
            self.list(); return;
        };
        if app.exe.to_lowercase().contains("discord") {
            emit(json!({ "ev": "mix_error", "msg": "Discord no puede ser una fuente del bot" })); self.list(); return;
        }
        let parents = parent_map();
        let discord = pid_file("discord.pid");
        let discord_tree = discord != 0 && (descendant(pid, discord, &parents) || descendant(discord, pid, &parents));
        let overlap = self.selected.keys().any(|&other| descendant(pid, other, &parents) || descendant(other, pid, &parents));
        if discord_tree || overlap {
            emit(json!({ "ev": "mix_error", "msg": "el árbol de procesos se solapa con Discord u otra fuente elegida" })); self.list(); return;
        }
        self.selected.entry(pid).or_insert_with(|| Selection { exe: app.exe.clone(), birth: app.birth, gain: 1.0, capture: None, reconnect_deadline: None });
        if self.active {
            if let Err(e) = self.start_one(pid, app) {
                self.selected.remove(&pid);
                emit(json!({ "ev": "mix_error", "msg": e }));
            }
        }
        self.list();
    }

    fn tick(&mut self) {
        let had_source = self.active && !self.selected.is_empty();
        while let Ok(id) = self.external_rx.try_recv() { self.external_changes.insert(id); }
        if self.last_scan.elapsed() >= SCAN {
            self.last_scan = Instant::now();
            let current = apps(&self.en);
            let pids: Vec<u32> = self.selected.keys().copied().collect();
            for pid in pids {
                let Some(app) = current.iter().find(|a| a.pid == pid && a.exe == self.selected[&pid].exe && a.birth == self.selected[&pid].birth) else {
                    // Un reinicio conserva la elección solo si hay un único sucesor nuevo del mismo exe.
                    if self.selected[&pid].reconnect_deadline.is_none() {
                        self.stop_one(pid);
                        self.selected.get_mut(&pid).unwrap().reconnect_deadline = Some(Instant::now() + Duration::from_secs(10));
                        emit(json!({ "ev": "mix_error", "msg": format!("la app {pid} no tiene sesión compartida (cerrada o en modo exclusivo); esperando reapertura") }));
                    }
                    let old = &self.selected[&pid];
                    let parents = parent_map();
                    let discord = pid_file("discord.pid");
                    let candidates: Vec<&App> = current.iter().filter(|a| a.exe == old.exe && a.birth > old.birth
                        && (a.pid == pid || !self.selected.contains_key(&a.pid))
                        && (discord == 0 || (!descendant(a.pid, discord, &parents) && !descendant(discord, a.pid, &parents)))
                        && self.selected.keys().filter(|&&other| other != pid).all(|&other|
                            !descendant(a.pid, other, &parents) && !descendant(other, a.pid, &parents))).collect();
                    let successor = (candidates.len() == 1).then(|| (candidates[0].pid, candidates[0].birth));
                    if let Some((new_pid, birth)) = successor {
                        let mut selection = self.selected.remove(&pid).unwrap();
                        selection.birth = birth;
                        selection.capture = None;
                        selection.reconnect_deadline = None;
                        self.selected.insert(new_pid, selection);
                        if self.active {
                            if let Some(app) = current.iter().find(|a| a.pid == new_pid) {
                                if let Err(e) = self.start_one(new_pid, app) {
                                    self.selected.remove(&new_pid);
                                    emit(json!({ "ev": "mix_error", "msg": e }));
                                }
                            }
                        }
                    } else if self.selected[&pid].reconnect_deadline.is_some_and(|t| Instant::now() >= t) {
                        self.selected.remove(&pid);
                        emit(json!({ "ev": "mix_error", "msg": format!("la app {pid} no reabrió su salida de audio") }));
                    }
                    continue;
                };
                if self.active {
                    let changed = app.sessions.iter().any(|s| self.saved.ducked.contains_key(&s.id)
                        && (self.external_changes.contains(&s.id) || !still_ducked(&s.vol) || s.vol.muted().unwrap_or(false)));
                    if changed {
                        for session in &app.sessions {
                            if self.external_changes.contains(&session.id) && !session.vol.muted().unwrap_or(false) {
                                self.saved.ducked.remove(&session.id);
                            }
                        }
                        let _ = self.saved.save();
                        self.stop_one(pid); self.selected.remove(&pid);
                        emit(json!({ "ev": "mix_error", "msg": format!("{} cambió volumen en Windows; se detuvo el envío", app.name) }));
                        continue;
                    }
                    for s in &app.sessions {
                        if !self.saved.ducked.contains_key(&s.id) {
                            if let Err(e) = self.subscribe(pid, s).and_then(|_| duck_session(&mut self.saved, app, s)) {
                                self.stop_one(pid); self.selected.remove(&pid);
                                emit(json!({ "ev": "mix_error", "msg": e }));
                                break;
                            }
                        }
                    }
                }
            }
            self.list_from(&current, false);
            self.external_changes.retain(|id| self.subscriptions.contains_key(id));
        }
        if self.active {
            let mut failed = Vec::new();
            for (&pid, source) in &self.selected {
                if let Some(cap) = &source.capture {
                    if let Ok(result) = cap.ended.try_recv() {
                        failed.push((pid, result.err().unwrap_or_else(|| "la captura terminó".into())));
                        continue;
                    }
                }
            }
            for (pid, error) in failed {
                self.stop_one(pid);
                self.selected.remove(&pid);
                emit(json!({ "ev": "mix_error", "msg": error }));
            }
            if had_source && self.selected.is_empty() {
                emit(json!({ "ev": "state", "s": "error", "msg": "la última fuente de audio se detuvo" }));
            }
        }
    }
}

pub fn start(audio: Arc<audio_mix::Shared>) -> (mpsc::Sender<Command>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let (external_tx, external_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || unsafe {
        if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            emit(json!({ "ev": "mix_error", "msg": format!("COM de audio no inició: {e}") }));
            return;
        }
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            emit(json!({ "ev": "mix_error", "msg": "no pude abrir el catálogo de sesiones de Windows" }));
            CoUninitialize();
            return;
        };
        let mut mixer = Mixer { en, saved: Persist::load(), selected: HashMap::new(),
            subscriptions: HashMap::new(), external_tx, external_rx, external_changes: HashSet::new(),
            audio, active: false, last_scan: Instant::now(), last_list: String::new() };
        mixer.recover(&apps(&mixer.en));
        mixer.list();
        loop {
            match rx.recv_timeout(SCAN) {
                Ok(Command::Json(c)) => {
                    let pid = c["pid"].as_u64().unwrap_or(0) as u32;
                    match c["cmd"].as_str() {
                        Some("apps") => mixer.list_if(true),
                        Some("route") => mixer.route(pid, c["on"] == true),
                        Some("vol") => {
                            if let Some(s) = mixer.selected.get_mut(&pid) {
                                s.gain = (c["v"].as_f64().unwrap_or(1.0) as f32).clamp(0.0, 1.0);
                                let gain = s.gain;
                                mixer.audio.with(|m| m.gain(pid, gain));
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Command::Active(on, reply)) => { let _ = reply.send(mixer.active(on)); mixer.list(); }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            mixer.tick();
        }
        let _ = mixer.active(false);
        let _ = mixer.saved.save();
        CoUninitialize();
    });
    (tx, thread)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_service_child_belongs_to_its_browser() {
        // 5328 = Brave de Discord, 11812 = su servicio de audio; 11456 = Brave del usuario.
        let parents = HashMap::from([(11812, 5328), (5328, 6956), (8152, 11456), (11456, 6576)]);
        assert!(descendant(11812, 5328, &parents));
        assert!(descendant(5328, 5328, &parents));
        assert!(!descendant(8152, 5328, &parents));
    }
    use std::cell::Cell;
    struct Fake { value: Cell<f32>, muted: bool, fails: Cell<bool> }
    impl VolumeBackend for Fake {
        fn get(&self) -> Result<f32, String> { Ok(self.value.get()) }
        fn muted(&self) -> Result<bool, String> { Ok(self.muted) }
        fn set(&self, value: f32) -> Result<(), String> {
            if self.fails.get() { Err("fallo".into()) } else { self.value.set(value); Ok(()) }
        }
    }
    #[test]
    fn duck_and_restore_with_fake_backend() {
        let mut p = Persist::default();
        let volume = Fake { value: Cell::new(0.42), muted: false, fails: Cell::new(false) };
        duck_volume(&mut p, "session-1", 7, "app.exe", "app", &volume, false).unwrap();
        assert_eq!(volume.value.get(), DUCK);
        restore_volume(&mut p, "session-1", &volume);
        assert_eq!(volume.value.get(), 0.42);
        assert!(p.ducked.is_empty());
    }
    #[test]
    fn external_change_is_preserved() {
        let mut p = Persist::default();
        let volume = Fake { value: Cell::new(0.7), muted: false, fails: Cell::new(false) };
        duck_volume(&mut p, "session-1", 7, "app.exe", "app", &volume, false).unwrap();
        volume.value.set(0.25);
        restore_volume(&mut p, "session-1", &volume);
        assert_eq!(volume.value.get(), 0.25);
        assert!(p.ducked.is_empty());
    }
    #[test]
    fn muted_or_failed_session_never_remains_ducked() {
        let mut p = Persist::default();
        let muted = Fake { value: Cell::new(0.7), muted: true, fails: Cell::new(false) };
        assert!(duck_volume(&mut p, "m", 7, "app.exe", "app", &muted, false).is_err());
        let failed = Fake { value: Cell::new(0.7), muted: false, fails: Cell::new(true) };
        assert!(duck_volume(&mut p, "f", 7, "app.exe", "app", &failed, false).is_err());
        assert!(p.ducked.is_empty());
    }
    #[test]
    fn failed_restore_remains_pending_for_recovery() {
        let mut p = Persist::default();
        let volume = Fake { value: Cell::new(0.6), muted: false, fails: Cell::new(false) };
        duck_volume(&mut p, "session-1", 7, "app.exe", "app", &volume, false).unwrap();
        volume.fails.set(true);
        restore_volume(&mut p, "session-1", &volume);
        assert!(p.ducked.contains_key("session-1"));
        volume.fails.set(false);
        restore_volume(&mut p, "session-1", &volume);
        assert_eq!(volume.value.get(), 0.6);
    }
    #[test]
    fn event_context_distinguishes_external_volume_change() {
        let (tx, rx) = mpsc::channel();
        let sink = SessionEvents { id: "session-1".into(), external: tx };
        sink.OnSimpleVolumeChanged(DUCK, BOOL(0), &CONTEXT).unwrap();
        assert!(rx.try_recv().is_err());
        let other = GUID::from_u128(0x2d777355_838a_4c33_88c1_564557f1c127);
        sink.OnSimpleVolumeChanged(DUCK, BOOL(0), &other).unwrap();
        assert_eq!(rx.try_recv().unwrap(), "session-1");
    }
}
