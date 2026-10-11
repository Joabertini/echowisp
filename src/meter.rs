// Pico de audio de las sesiones de un arbol de procesos (Brave y sus hijos), para el vumetro opcional.
// Solo se consulta con la opcion prendida, la card visible y algo sonando.

use windows::core::Interface;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;

/// Si `pid` es `root` o desciende de el (Chromium suena desde un proceso hijo, el servicio de audio).
pub fn in_tree(pid: u32, root: u32, parents: &[(u32, u32)]) -> bool {
    let mut p = pid;
    for _ in 0..16 {
        if p == root {
            return true;
        }
        match parents.iter().find(|(c, _)| *c == p) {
            Some(&(_, parent)) if parent != p && parent != 0 => p = parent,
            _ => return false,
        }
    }
    false
}

fn parent_table() -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return v };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                v.push((e.th32ProcessID, e.th32ParentProcessID));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    v
}

pub struct Meter {
    mgr: IAudioSessionManager2,
    parents: Vec<(u32, u32)>,
    refreshed: std::time::Instant,
}

impl Meter {
    pub fn new() -> Option<Meter> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let en = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let mgr = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None).ok()?;
            Some(Meter { mgr, parents: parent_table(), refreshed: std::time::Instant::now() })
        }
    }

    /// Pico (0..1) de las sesiones del arbol de `root_pid`; 0 si no suena nada.
    pub fn peak(&mut self, root_pid: u32) -> f32 {
        if self.refreshed.elapsed().as_secs() >= 5 {
            self.parents = parent_table();
            self.refreshed = std::time::Instant::now();
        }
        let mut best = 0.0f32;
        unsafe {
            let Ok(list) = self.mgr.GetSessionEnumerator() else { return 0.0 };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(c2) = list.GetSession(i).and_then(|c| c.cast::<IAudioSessionControl2>()) else { continue };
                let pid = c2.GetProcessId().unwrap_or(0);
                if pid == 0 || !in_tree(pid, root_pid, &self.parents) {
                    continue;
                }
                if let Ok(m) = c2.cast::<IAudioMeterInformation>() {
                    best = best.max(m.GetPeakValue().unwrap_or(0.0));
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_membership() {
        let parents = [(10, 1), (11, 10), (12, 11), (20, 1)];
        assert!(in_tree(12, 10, &parents));
        assert!(in_tree(10, 10, &parents));
        assert!(!in_tree(20, 10, &parents));
        assert!(!in_tree(99, 10, &parents));
    }

    #[test]
    fn tree_cycle_does_not_hang() {
        let parents = [(5, 6), (6, 5)];
        assert!(!in_tree(5, 1, &parents));
    }
}
