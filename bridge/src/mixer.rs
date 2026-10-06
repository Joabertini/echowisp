// Mezclador: apps con sonido, su volumen y "mandar al cable". Mandar al cable cambia la salida de esa
// app, la misma funcion que Configuracion > Sonido > Volumen de aplicaciones (no documentada; EarTrumpet
// la usa igual). Asi la app no suena local y el usuario la escucha una sola vez, por el bot en Discord.
// Se recuerda la salida previa en ruteo.json: al cerrar se devuelve, y si hubo un cuelgue se repara cuando
// la app vuelve a aparecer.
use crate::serve::emit;
use serde_json::{json, Value};
use std::{collections::HashMap, ffi::c_void, path::PathBuf, sync::mpsc};
use windows::{
    core::*,
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::*,
        Media::Audio::*,
        System::{Com::StructuredStorage::PropVariantToStringAlloc, Com::*, Threading::*, WinRT::RoGetActivationFactory},
    },
};

macro_rules! policy {
    ($name:ident, $iid:literal) => {
        #[interface($iid)]
        // Hereda de IInspectable: sus 3 metodos van a mano (el macro no acepta IInspectable de padre).
        unsafe trait $name: IUnknown {
            unsafe fn _get_iids(&self) -> HRESULT;
            unsafe fn _get_runtime_class_name(&self) -> HRESULT;
            unsafe fn _get_trust_level(&self) -> HRESULT;
            unsafe fn _m0(&self) -> HRESULT;
            unsafe fn _m1(&self) -> HRESULT;
            unsafe fn _m2(&self) -> HRESULT;
            unsafe fn _m3(&self) -> HRESULT;
            unsafe fn _m4(&self) -> HRESULT;
            unsafe fn _m5(&self) -> HRESULT;
            unsafe fn _m6(&self) -> HRESULT;
            unsafe fn _m7(&self) -> HRESULT;
            unsafe fn _m8(&self) -> HRESULT;
            unsafe fn _m9(&self) -> HRESULT;
            unsafe fn _m10(&self) -> HRESULT;
            unsafe fn _m11(&self) -> HRESULT;
            unsafe fn _m12(&self) -> HRESULT;
            unsafe fn _m13(&self) -> HRESULT;
            unsafe fn _m14(&self) -> HRESULT;
            unsafe fn _m15(&self) -> HRESULT;
            unsafe fn _m16(&self) -> HRESULT;
            unsafe fn _m17(&self) -> HRESULT;
            unsafe fn _m18(&self) -> HRESULT;
            unsafe fn set(&self, pid: u32, flow: EDataFlow, role: ERole, id: *mut c_void) -> HRESULT;
            unsafe fn get(&self, pid: u32, flow: EDataFlow, role: ERole, id: *mut HSTRING) -> HRESULT;
        }
    };
}
// Misma tabla, IID distinto segun la version de Windows (21H2 en adelante / anteriores).
policy!(IPolicyNew, "ab3d4648-e242-459f-b02f-541c70306324");
policy!(IPolicyOld, "2a59116d-6c4f-45e0-a74f-707e3fef9258");

enum Policy {
    New(IPolicyNew),
    Old(IPolicyOld),
}

const TOKEN: &str = r"\\?\SWD#MMDEVAPI#";
const RENDER: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";

impl Policy {
    fn new() -> Result<Policy> {
        let name = h!("Windows.Media.Internal.AudioPolicyConfig");
        unsafe { RoGetActivationFactory::<IPolicyNew>(name).map(Policy::New).or_else(|_| RoGetActivationFactory::<IPolicyOld>(name).map(Policy::Old)) }
    }
    /// Salida de `pid`; "" = la predeterminada de Windows.
    fn set(&self, pid: u32, dev: &str) -> Result<()> {
        let h = (!dev.is_empty()).then(|| HSTRING::from(format!("{TOKEN}{dev}{RENDER}")));
        let raw: *mut c_void = h.as_ref().map_or(std::ptr::null_mut(), |h| unsafe { std::mem::transmute_copy(h) });
        for role in [eMultimedia, eConsole] {
            unsafe {
                match self {
                    Policy::New(p) => p.set(pid, eRender, role, raw),
                    Policy::Old(p) => p.set(pid, eRender, role, raw),
                }
                .ok()?
            };
        }
        Ok(())
    }
    fn get(&self, pid: u32) -> String {
        let mut h = HSTRING::new();
        let _ = unsafe {
            match self {
                Policy::New(p) => p.get(pid, eRender, eMultimedia, &mut h),
                Policy::Old(p) => p.get(pid, eRender, eMultimedia, &mut h),
            }
        };
        let s = h.to_string();
        s.trim_start_matches(TOKEN).trim_end_matches(RENDER).to_string()
    }
}

struct App {
    pid: u32,
    exe: String,
    name: String,
    vols: Vec<ISimpleAudioVolume>,
}

fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("ytm-float")
}

fn pid_file(name: &str) -> u32 {
    std::fs::read_to_string(data_dir().join(name)).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

struct Mixer {
    policy: Policy,
    en: IMMDeviceEnumerator,
    /// exe → salida previa ("" = predeterminada). `stale` viene de una corrida anterior que no cerro bien.
    routes: HashMap<String, String>,
    stale: HashMap<String, String>,
}

impl Mixer {
    fn save(&self) {
        let all: HashMap<_, _> = self.stale.iter().chain(self.routes.iter()).collect();
        let _ = std::fs::write(data_dir().join("ruteo.json"), serde_json::to_string_pretty(&all).unwrap_or_default());
    }

    /// Id del cable virtual de salida ("CABLE Input"), si esta instalado.
    fn cable(&self) -> Option<String> {
        unsafe {
            let devs = self.en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).ok()?;
            for i in 0..devs.GetCount().ok()? {
                let d = devs.Item(i).ok()?;
                let name = d.OpenPropertyStore(STGM_READ).ok().and_then(|ps| ps.GetValue(&PKEY_Device_FriendlyName).ok()).and_then(|v| PropVariantToStringAlloc(&v).ok());
                if let Some(n) = name {
                    let s = n.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(n.0 as _));
                    if s.contains("CABLE Input") {
                        let id = d.GetId().ok()?;
                        let r = id.to_string().ok();
                        CoTaskMemFree(Some(id.0 as _));
                        return r;
                    }
                }
            }
            None
        }
    }

    /// Apps con sesion de audio, sin Discord (eco), sin sonidos del sistema ni este proceso.
    fn apps(&self) -> Vec<App> {
        let mut out: Vec<App> = Vec::new();
        let (ytm, dsc, me) = (pid_file("brave.pid"), pid_file("discord.pid"), std::process::id());
        unsafe {
            let Ok(devs) = self.en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
            for i in 0..devs.GetCount().unwrap_or(0) {
                let Ok(d) = devs.Item(i) else { continue };
                let Ok(sm) = d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(ss) = sm.GetSessionEnumerator() else { continue };
                for j in 0..ss.GetCount().unwrap_or(0) {
                    let Ok(c2) = ss.GetSession(j).and_then(|c| c.cast::<IAudioSessionControl2>()) else { continue };
                    if c2.IsSystemSoundsSession() == S_OK || c2.GetState().map_or(true, |s| s == AudioSessionStateExpired) {
                        continue;
                    }
                    let pid = c2.GetProcessId().unwrap_or(0);
                    if pid == 0 || pid == me || pid == dsc {
                        continue;
                    }
                    let Some(exe) = exe_path(pid) else { continue };
                    let stem = std::path::Path::new(&exe).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    if stem.to_lowercase().contains("discord") {
                        continue;
                    }
                    let Ok(vol) = c2.cast::<ISimpleAudioVolume>() else { continue };
                    match out.iter_mut().find(|a| a.pid == pid) {
                        Some(a) => a.vols.push(vol),
                        None => {
                            let name = if pid == ytm { "Música (YTM Float)".to_string() } else { stem };
                            out.push(App { pid, exe, name, vols: vec![vol] });
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out
    }

    fn list(&mut self) {
        let cable = self.cable();
        let apps = self.apps();
        // Reparar lo que quedo mandado al cable por una corrida que se colgo.
        let mut fixed = false;
        for a in &apps {
            if let Some(prev) = self.stale.remove(&a.exe) {
                let _ = self.policy.set(a.pid, &prev);
                fixed = true;
            }
        }
        if fixed {
            self.save();
        }
        let list: Vec<Value> = apps
            .iter()
            .map(|a| {
                let v = a.vols.first().and_then(|v| unsafe { v.GetMasterVolume().ok() }).unwrap_or(1.0);
                // "on" sale de Windows (la salida elegida para esa app), no de nuestra memoria.
                let on = cable.as_deref().is_some_and(|c| self.policy.get(a.pid) == c);
                json!({ "pid": a.pid, "name": a.name, "on": on, "vol": (v * 100.0).round() })
            })
            .collect();
        emit(json!({ "ev": "apps", "cable": cable.is_some(), "list": list }));
    }

    fn route(&mut self, pid: u32, on: bool) {
        let Some(exe) = exe_path(pid) else { return };
        let r = if on {
            let Some(cable) = self.cable() else {
                emit(json!({ "ev": "mix_error", "msg": "no encontré VB-Cable (CABLE Input)" }));
                return;
            };
            let prev = self.routes.get(&exe).cloned().unwrap_or_else(|| self.policy.get(pid));
            self.routes.insert(exe, prev);
            self.policy.set(pid, &cable)
        } else {
            let prev = self.routes.remove(&exe).unwrap_or_default();
            self.policy.set(pid, &prev)
        };
        if let Err(e) = r {
            emit(json!({ "ev": "mix_error", "msg": format!("Windows no dejó cambiar la salida: {}", e.message()) }));
        }
        self.save();
        self.list();
    }

    fn volume(&self, pid: u32, v: f32) {
        if let Some(a) = self.apps().into_iter().find(|a| a.pid == pid) {
            for s in a.vols {
                let _ = unsafe { s.SetMasterVolume(v.clamp(0.0, 1.0), std::ptr::null()) };
            }
        }
    }

    /// Al cerrar: cada app vuelve a la salida que tenia. Las que no estan corriendo quedan en
    /// ruteo.json y se reparan la proxima vez que aparezcan.
    fn restore(&mut self) {
        let running: HashMap<String, u32> = self.apps().into_iter().map(|a| (a.exe, a.pid)).collect();
        for (exe, prev) in std::mem::take(&mut self.routes) {
            match running.get(&exe) {
                Some(&pid) => {
                    let _ = self.policy.set(pid, &prev);
                }
                None => {
                    self.stale.insert(exe, prev);
                }
            }
        }
        self.save();
    }
}

/// Hilo propio (COM no se comparte entre hilos). Ordenes: {"cmd":"apps"} · {"cmd":"route","pid":1,"on":true}
/// · {"cmd":"vol","pid":1,"v":0.5}. Al cerrar el canal devuelve las salidas y termina.
pub fn start() -> (mpsc::Sender<Value>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Value>();
    let h = std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let (Ok(policy), Ok(en)) = (Policy::new(), CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)) else {
            emit(json!({ "ev": "mix_error", "msg": "este Windows no permite cambiar la salida por app" }));
            return;
        };
        let stale = std::fs::read_to_string(data_dir().join("ruteo.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut m = Mixer { policy, en, routes: HashMap::new(), stale };
        while let Ok(c) = rx.recv() {
            let pid = c["pid"].as_u64().unwrap_or(0) as u32;
            match c["cmd"].as_str() {
                Some("apps") => m.list(),
                Some("route") => m.route(pid, c["on"] == true),
                Some("vol") => m.volume(pid, c["v"].as_f64().unwrap_or(1.0) as f32),
                _ => {}
            }
        }
        m.restore();
    });
    (tx, h)
}
