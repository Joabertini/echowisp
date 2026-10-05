// Lanza Brave con un perfil propio. Cada proceso va dentro de un Job object con
// KILL_ON_JOB_CLOSE: si ytm-float muere (aunque sea por crash), Brave muere con el.
use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    mem::{size_of, zeroed},
    net::TcpStream,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    thread::sleep,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    System::{
        JobObjects::*,
        Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ},
        Threading::*,
    },
};

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("ytm-float")
}

fn profile_dir() -> PathBuf {
    data_dir().join("perfil")
}

pub fn find_brave() -> Option<PathBuf> {
    let sub = wide(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\brave.exe");
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(root, sub.as_ptr(), null(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut len)
        };
        if r == 0 {
            let n = (len as usize / 2).saturating_sub(1);
            let p = PathBuf::from(String::from_utf16_lossy(&buf[..n]).trim_matches('"'));
            if p.exists() {
                return Some(p);
            }
        }
    }
    ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|d| PathBuf::from(d).join(r"BraveSoftware\Brave-Browser\Application\brave.exe"))
        .find(|p| p.exists())
}

pub struct Proc {
    job: HANDLE,
    process: HANDLE,
}
unsafe impl Send for Proc {}

impl Proc {
    /// true si el proceso termino dentro de `ms`.
    pub fn wait(&self, ms: u32) -> bool {
        unsafe { WaitForSingleObject(self.process, ms) == WAIT_OBJECT_0 }
    }
    pub fn kill(&self) {
        unsafe { TerminateJobObject(self.job, 0) };
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.process);
            CloseHandle(self.job); // KILL_ON_JOB_CLOSE: se lleva lo que quede vivo
        }
    }
}

fn spawn(exe: &Path, args: &[String]) -> io::Result<Proc> {
    let mut cmd = format!("\"{}\"", exe.display());
    for a in args {
        cmd.push(' ');
        if a.contains(' ') {
            cmd.push('"');
            cmd.push_str(a);
            cmd.push('"');
        } else {
            cmd.push_str(a);
        }
    }
    let mut cmdw = wide(&cmd);
    unsafe {
        let job = CreateJobObjectW(null(), null());
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        let mut si: STARTUPINFOW = zeroed();
        si.cb = size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = zeroed();
        let ok = CreateProcessW(
            null(), cmdw.as_mut_ptr(), null(), null(), 0, CREATE_SUSPENDED,
            null(), null(), &si, &mut pi,
        );
        if ok == 0 {
            let e = io::Error::last_os_error();
            CloseHandle(job);
            return Err(e);
        }
        AssignProcessToJobObject(job, pi.hProcess);
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);
        let _ = std::fs::write(data_dir().join("brave.pid"), pi.dwProcessId.to_string());
        Ok(Proc { job, process: pi.hProcess })
    }
}

/// Si una corrida anterior dejo Brave vivo, tiene el perfil bloqueado: se cierra.
fn kill_orphan(exe: &Path) {
    // Brave mantiene `lockfile` abierto en exclusiva: si se puede borrar, no hay huerfano.
    let lock = profile_dir().join("lockfile");
    if !lock.exists() || std::fs::remove_file(&lock).is_ok() {
        return;
    }
    let Some(pid) = std::fs::read_to_string(data_dir().join("brave.pid")).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
        return;
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return;
        }
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        // Solo si el pid sigue siendo brave.exe (los pids se reciclan).
        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n) != 0
            && PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])) == exe
        {
            TerminateProcess(h, 0);
            WaitForSingleObject(h, 3000);
        }
        CloseHandle(h);
    }
}

/// Brave sin ninguna ventana, recortado al minimo para una sola pestaña de audio.
pub fn launch_headless(exe: &Path) -> io::Result<(Proc, u16)> {
    kill_orphan(exe);
    let prof = profile_dir();
    std::fs::create_dir_all(&prof)?;
    let port_file = prof.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);
    let args: Vec<String> = [
        "--headless=new",
        &format!("--user-data-dir={}", prof.display()),
        "--remote-debugging-port=0",
        "--no-first-run",
        "--no-default-browser-check",
        // Todo en un proceso: ahorra ~70 MB frente al modelo multiproceso.
        "--single-process",
        "--disable-gpu",
        "--in-process-gpu",
        "--js-flags=--lite-mode",
        "--disable-extensions",
        // Sin --disable-component-update ni --disable-background-networking: Brave Shields
        // necesita bajar y actualizar sus listas de filtros (con esos flags no bloqueaba nada).
        "--disable-sync",
        "--disable-default-apps",
        "--disable-features=IsolateOrigins,site-per-process,Translate,MediaRouter,OptimizationHints,\
BackForwardCache,SpareRendererForSitePerProcess,AudioServiceOutOfProcess,PaintHolding",
        "--enable-features=NetworkServiceInProcess2",
        "--blink-settings=imagesEnabled=false",
        "--autoplay-policy=no-user-gesture-required",
        "about:blank",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let p = spawn(exe, &args)?;
    for _ in 0..150 {
        sleep(Duration::from_millis(100));
        if let Some(port) = std::fs::read_to_string(&port_file).ok().and_then(|s| s.lines().next()?.parse().ok()) {
            return Ok((p, port));
        }
        if p.wait(0) {
            return Err(io::Error::other("Brave se cerró al arrancar"));
        }
    }
    Err(io::Error::other("Brave no respondió"))
}

/// Unica vez que se ve Brave: ventana normal para iniciar sesion en Google.
pub fn launch_login(exe: &Path) -> io::Result<Proc> {
    let prof = profile_dir();
    std::fs::create_dir_all(&prof)?;
    let args = [
        format!("--user-data-dir={}", prof.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--app=https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/".into(),
    ];
    spawn(exe, &args)
}

pub fn http_get(port: u16, path: &str) -> io::Result<String> {
    // El server de DevTools rechaza HTTP/1.0 y no cierra la conexion: se lee por Content-Length.
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")?;
    let mut r = io::BufReader::new(s);
    let mut len = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        if io::BufRead::read_line(&mut r, &mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    String::from_utf8(body).map_err(io::Error::other)
}
