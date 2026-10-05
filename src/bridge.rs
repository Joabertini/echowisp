// Puente de audio a Discord (ytm-bridge.exe, modulo opcional junto al exe): la card lo lanza como
// hijo y le habla por lineas JSON (protocolo en bridge/src/serve.rs). Sin el exe, no hay seccion.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    process::{Child, ChildStdin, Command, Stdio},
    time::Duration,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_BRIDGE: u32 = WM_APP + 3;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Bridge {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Bridge {
    pub fn start(hwnd: isize) -> Option<Bridge> {
        let exe = std::env::current_exe().ok()?.with_file_name("ytm-bridge.exe");
        if !exe.exists() {
            return None;
        }
        let mut child = Command::new(exe)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let stdin = child.stdin.take();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                let p = Box::into_raw(Box::new(v));
                if unsafe { PostMessageW(hwnd as _, WM_BRIDGE, 0, p as isize) } == 0 {
                    drop(unsafe { Box::from_raw(p) });
                }
            }
        });
        Some(Bridge { child, stdin })
    }

    pub fn send(&mut self, v: Value) {
        if let Some(s) = self.stdin.as_mut() {
            let _ = writeln!(s, "{v}");
            let _ = s.flush();
        }
    }
}

impl Drop for Bridge {
    /// Cerrar stdin le avisa al puente: sale del canal y termina. Si no, se lo mata.
    fn drop(&mut self) {
        self.stdin = None;
        for _ in 0..20 {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
    }
}
