// Segunda card: sale deslizandose desde un costado de la principal (el que tenga lugar) y la sigue
// cuando se mueve. Muestra las apps que mandan sonido a Discord o la configuracion. Ventana en capas
// propia, duenia la card: el contenido se dibuja entero una vez y la animacion solo cambia que parte
// se ve (asoma primero el borde de afuera, como si saliera de atras de la card).
use super::*;

pub const PW: i32 = 232;
const GAP: i32 = 8;
const ANIM_MS: f32 = 190.0;
pub const T_PANEL: usize = 6;
const P_ROW: i32 = 28;
const P_VOL: i32 = 24;

#[derive(Clone, Copy, PartialEq)]
pub enum Panel {
    None,
    Apps,
    Config,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PHit {
    None,
    Close,
    Label(&'static str),
    Msg(&'static str),
    App(usize),
    Sw(usize),
    Vol(usize),
    ShowBr,
    ShowDsc,
    Token,
    Device(usize),
    AllDevices,
    Browser(usize),
    Hotkey(usize),
}

/// Atajos que se pueden cambiar (los multimedia no).
pub const EDITABLE: &[(i32, &str)] = &[
    (HK_TOGGLE, "Play / pausa"),
    (HK_NEXT, "Siguiente"),
    (HK_PREV, "Anterior"),
    (HK_SHOW, "Mostrar / ocultar"),
    (HK_MINI, "Colapsar"),
    (HK_SEARCH, "Buscar"),
];

fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn key_name(mods: HOT_KEY_MODIFIERS, vk: u32) -> String {
    let mut s = String::new();
    for (m, n) in [(MOD_CONTROL, "Ctrl+"), (MOD_ALT, "Alt+"), (MOD_SHIFT, "Shift+"), (MOD_WIN, "Win+")] {
        if mods & m != 0 {
            s.push_str(n);
        }
    }
    let k = match vk as u16 {
        VK_SPACE => "Espacio".into(),
        VK_LEFT => "←".into(),
        VK_RIGHT => "→".into(),
        VK_UP => "↑".into(),
        VK_DOWN => "↓".into(),
        v @ 0x30..=0x39 | v @ 0x41..=0x5A => (v as u8 as char).to_string(),
        v @ VK_F1..=VK_F12 => format!("F{}", v - VK_F1 + 1),
        v => format!("tecla {v:#x}"),
    };
    s + &k
}

fn card_file() -> std::path::PathBuf {
    brave::data_dir().join("card.json")
}

/// Fila opcional a la vista en la card ("puente" | "discord"). Por defecto si.
pub fn card_pref(k: &str) -> bool {
    std::fs::read_to_string(card_file()).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v[k].as_bool()).unwrap_or(true)
}

fn hotkeys_file() -> std::path::PathBuf {
    brave::data_dir().join("atajos.json")
}

/// Atajos por defecto con los cambios del usuario encima (atajos.json: {"id": [mods, vk]}).
pub fn load_hotkeys() -> Vec<(i32, HOT_KEY_MODIFIERS, u32)> {
    let saved: Value = std::fs::read_to_string(hotkeys_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
    HOTKEYS
        .iter()
        .map(|&(id, m, vk)| match saved[id.to_string()].as_array() {
            Some(a) if a.len() == 2 => (id, a[0].as_u64().unwrap_or(0) as HOT_KEY_MODIFIERS, a[1].as_u64().unwrap_or(0) as u32),
            _ => (id, m, vk as u32),
        })
        .collect()
}

impl App {
    /// Filas de la card de costado: (que es, rectangulo en px logicos). Sirve para dibujar y para clics.
    fn panel_layout(&self) -> (Vec<(PHit, RECT)>, i32) {
        let mut v = Vec::new();
        let mut y = 40;
        let row = |h: PHit, l: i32, r: i32, top: i32, b: i32, v: &mut Vec<(PHit, RECT)>| v.push((h, RECT { left: l, top, right: r, bottom: b }));
        match self.panel {
            Panel::Apps => {
                if !self.mix_cable {
                    row(PHit::Msg("Falta VB-Cable (CABLE Input)"), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                } else if self.mix_apps.is_empty() {
                    row(PHit::Msg("Ninguna app está sonando"), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                } else {
                    for (i, a) in self.mix_apps.iter().enumerate() {
                        row(PHit::Sw(i), PW - 54, PW - 16, y + 5, y + P_ROW - 5, &mut v);
                        row(PHit::App(i), 12, PW - 60, y, y + P_ROW, &mut v);
                        y += P_ROW;
                        if self.mix_sel == Some(a.pid) {
                            row(PHit::Vol(i), 24, PW - 58, y + 4, y + P_VOL - 4, &mut v);
                            y += P_VOL;
                        }
                    }
                }
            }
            Panel::Config => {
                let label = |t: &'static str, y: &mut i32, v: &mut Vec<(PHit, RECT)>| {
                    v.push((PHit::Label(t), RECT { left: 16, top: *y + 6, right: PW - 16, bottom: *y + 24 }));
                    *y += 26;
                };
                if self.br.is_some() || self.dsc_avail {
                    label("EN LA CARD", &mut y, &mut v);
                    for (h, ok) in [(PHit::ShowBr, self.br.is_some()), (PHit::ShowDsc, self.dsc_avail)] {
                        if ok {
                            row(h, 12, PW - 12, y, y + P_ROW, &mut v);
                            y += P_ROW;
                        }
                    }
                }
                if self.br.is_some() {
                    label("BOT DE DISCORD", &mut y, &mut v);
                    row(PHit::Token, 12, PW - 12, y, y + 30, &mut v);
                    y += 34;
                    label("ENTRADA DEL PUENTE", &mut y, &mut v);
                    // Por defecto solo cables virtuales y la elegida: Voicemeeter y microfonos alargan la lista.
                    let mut hidden = 0;
                    for (i, d) in self.br_devices.iter().enumerate() {
                        if self.cfg_all_dev || d.starts_with("CABLE") || *d == self.br_device {
                            row(PHit::Device(i), 12, PW - 12, y, y + P_ROW, &mut v);
                            y += P_ROW;
                        } else {
                            hidden += 1;
                        }
                    }
                    if hidden > 0 || self.cfg_all_dev {
                        row(PHit::AllDevices, 12, PW - 12, y, y + P_ROW, &mut v);
                        y += P_ROW;
                    }
                }
                label("NAVEGADOR", &mut y, &mut v);
                for i in 0..self.browsers.len() {
                    row(PHit::Browser(i), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                }
                label("ATAJOS", &mut y, &mut v);
                for i in 0..EDITABLE.len() {
                    row(PHit::Hotkey(i), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                }
            }
            Panel::None => {}
        }
        (v, y + 12)
    }

    fn pr(&self, r: RECT) -> RECT {
        self.r(r.left, r.top, r.right, r.bottom)
    }

    pub fn panel_open(&mut self, p: Panel) {
        if self.panel == p && self.panel_target > 0.0 {
            return self.panel_close();
        }
        if p == Panel::Config {
            self.browsers = brave::browsers();
        }
        self.panel = p;
        self.panel_hover = PHit::None;
        self.panel_capture = None;
        self.panel_target = 1.0;
        unsafe {
            if p == Panel::Apps {
                self.mix_send(json!({ "cmd": "apps" }));
                SetTimer(self.hwnd, T_MIX, 2000, None);
            } else {
                KillTimer(self.hwnd, T_MIX);
            }
            SetTimer(self.hwnd, T_PANEL, 15, None);
        }
        self.panel_last = Instant::now();
        self.panel_render();
        self.invalidate();
    }

    pub fn panel_close(&mut self) {
        if self.panel == Panel::None {
            return;
        }
        self.panel_target = 0.0;
        self.panel_capture = None;
        self.panel_last = Instant::now();
        unsafe {
            KillTimer(self.hwnd, T_MIX);
            SetTimer(self.hwnd, T_PANEL, 15, None);
        }
        self.invalidate();
    }

    /// Sin animacion: al colapsar u ocultar la card.
    pub fn panel_hide_now(&mut self) {
        self.panel = Panel::None;
        self.panel_t = 0.0;
        self.panel_target = 0.0;
        self.panel_capture = None;
        self.mix_sel = None;
        unsafe {
            KillTimer(self.hwnd, T_PANEL);
            KillTimer(self.hwnd, T_MIX);
            ShowWindow(self.panel_hwnd, SW_HIDE);
        }
    }

    /// Paso de la animacion (timer T_PANEL).
    pub fn panel_tick(&mut self) {
        let dt = self.panel_last.elapsed().as_secs_f32() * 1000.0 / ANIM_MS;
        self.panel_last = Instant::now();
        self.panel_t = if self.panel_target > self.panel_t { (self.panel_t + dt).min(1.0) } else { (self.panel_t - dt).max(0.0) };
        if self.panel_t == self.panel_target {
            unsafe { KillTimer(self.hwnd, T_PANEL) };
            if self.panel_t == 0.0 {
                self.panel = Panel::None;
                self.mix_sel = None;
                unsafe { ShowWindow(self.panel_hwnd, SW_HIDE) };
                return;
            }
        }
        self.panel_present();
    }

    /// Lo que se ve de la card de costado, pegada a la principal. Tambien al mover la principal.
    pub fn panel_present(&mut self) {
        if self.panel == Panel::None {
            return;
        }
        let Some(cv) = self.panel_canvas.as_ref() else { return };
        unsafe {
            let mut cr: RECT = zeroed();
            GetWindowRect(self.hwnd, &mut cr);
            let mut mi: MONITORINFO = zeroed();
            mi.cbSize = size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
            let (pw, gap) = (cv.w, self.px(GAP));
            let left = cr.right + gap + pw > mi.rcWork.right;
            let vw = (pw as f32 * ease(self.panel_t)).round() as i32;
            if vw <= 0 {
                ShowWindow(self.panel_hwnd, SW_HIDE);
                return;
            }
            let x = if left { cr.left - gap - vw } else { cr.right + gap };
            self.panel_sx = if left { 0 } else { pw - vw };
            let top = cr.top.min(mi.rcWork.bottom - cv.h).max(mi.rcWork.top);
            let dst = POINT { x, y: top };
            let size = SIZE { cx: vw, cy: cv.h };
            let src = POINT { x: self.panel_sx, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(self.panel_hwnd, null_mut(), &dst, &size, cv.dc, &src, 0, &blend, ULW_ALPHA);
            if IsWindowVisible(self.panel_hwnd) == 0 {
                ShowWindow(self.panel_hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    /// Dibuja la card de costado completa (cambia el contenido, no la animacion).
    pub fn panel_render(&mut self) {
        if self.panel == Panel::None {
            return;
        }
        let (rows, h) = self.panel_layout();
        let (w, h) = (self.px(PW), self.px(h));
        unsafe {
            if self.panel_canvas.as_ref().map_or(true, |c| c.w != w || c.h != h) {
                self.panel_canvas = None;
                self.panel_canvas = Some(Canvas::new(w, h));
            }
            let mut cv = self.panel_canvas.take().unwrap();
            cv.pixels().fill(BG);
            let (wf, hf, s) = (w as f32, h as f32, self.scale);
            cv.round(0.0, 0.0, wf, hf, RADIUS * s, HAIRLINE);
            cv.round(1.0, 1.0, wf - 1.0, hf - 1.0, RADIUS * s - 1.0, BG);
            let hov = self.panel_hover;
            if hov == PHit::Close {
                let z = self.r(PW - 36, 4, PW - 8, 32);
                cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(14.0), TAB);
            }
            // Formas
            for &(hit, r) in &rows {
                let z = self.pr(r);
                let (l, t, rr, b) = (z.left as f32, z.top as f32, z.right as f32, z.bottom as f32);
                match hit {
                    PHit::Sw(i) => {
                        let on = self.mix_apps[i].on;
                        cv.round(l, t, rr, b, (b - t) / 2.0, if on { GREEN } else { HOVER });
                        let k = (b - t) / 2.0 - self.pf(2.5);
                        cv.dot(if on { rr - (b - t) / 2.0 } else { l + (b - t) / 2.0 }, (t + b) / 2.0, k, INK);
                    }
                    PHit::Vol(i) => {
                        let y = (t + b) / 2.0;
                        cv.round(l, y - self.pf(2.0), rr, y + self.pf(2.0), self.pf(2.0), HOVER);
                        let x = l + (rr - l) * (self.mix_apps[i].vol / 100.0) as f32;
                        if x - l > 1.0 {
                            cv.round(l, y - self.pf(2.0), x, y + self.pf(2.0), self.pf(2.0), INDIGO);
                        }
                        cv.dot(x, y, self.pf(5.0), INK);
                    }
                    PHit::Token => cv.round(l, t, rr, b, self.pf(10.0), if hov == hit { INDIGO } else { TAB }),
                    PHit::ShowBr | PHit::ShowDsc => {
                        if hov == hit {
                            cv.round(l, t, rr, b, self.pf(8.0), HOVER);
                        }
                        let on = if hit == PHit::ShowBr { self.show_br } else { self.show_dsc };
                        let (sl, st, sr, sb) = (rr - self.pf(42.0), t + self.pf(5.0), rr - self.pf(4.0), b - self.pf(5.0));
                        cv.round(sl, st, sr, sb, (sb - st) / 2.0, if on { GREEN } else { HOVER });
                        let k = (sb - st) / 2.0 - self.pf(2.5);
                        cv.dot(if on { sr - (sb - st) / 2.0 } else { sl + (sb - st) / 2.0 }, (st + sb) / 2.0, k, INK);
                    }
                    PHit::App(_) | PHit::Device(_) | PHit::AllDevices | PHit::Browser(_) | PHit::Hotkey(_) => {
                        if hov == hit || self.panel_capture.is_some_and(|c| hit == PHit::Hotkey(c)) {
                            cv.round(l, t, rr, b, self.pf(8.0), HOVER);
                        }
                        let sel = match hit {
                            PHit::Device(i) => Some(self.br_devices[i] == self.br_device),
                            PHit::Browser(i) => Some(self.browsers[i].1 == self.browser_cur),
                            _ => None,
                        };
                        if let Some(sel) = sel {
                            let (cx, cy) = (l + self.pf(14.0), (t + b) / 2.0);
                            cv.dot(cx, cy, self.pf(5.0), if sel { INDIGO } else { HOVER });
                            if sel {
                                cv.dot(cx, cy, self.pf(2.0), INK);
                            }
                        }
                    }
                    _ => {}
                }
            }
            GdiFlush();
            // Texto
            let title = if self.panel == Panel::Apps { "Apps a Discord" } else { "Configuración" };
            cv.text(self.f.title, title, self.r(16, 8, PW - 40, 32), INK, DT_LEFT);
            cv.text(self.f.icon, "\u{E8BB}", self.r(PW - 36, 4, PW - 8, 32), if hov == PHit::Close { INK } else { DIM }, DT_CENTER);
            for &(hit, r) in &rows {
                let z = self.pr(r);
                let inset = RECT { left: z.left + self.px(30), right: z.right - self.px(8), ..z };
                match hit {
                    PHit::Label(t) => cv.text(self.f.small, t, z, DIM3, DT_LEFT),
                    PHit::Msg(t) => cv.text(self.f.small, t, z, DIM, DT_CENTER),
                    PHit::App(i) => {
                        let a = &self.mix_apps[i];
                        cv.text(self.f.small, &a.name, RECT { left: z.left + self.px(10), ..z }, if a.on { INK } else { DIM }, DT_LEFT);
                    }
                    PHit::Vol(i) => {
                        let v = format!("{}", self.mix_apps[i].vol as i32);
                        cv.text(self.f.small, &v, RECT { left: z.right + self.px(6), right: z.right + self.px(40), top: z.top - self.px(6), bottom: z.bottom + self.px(6) }, DIM, DT_CENTER);
                    }
                    PHit::ShowBr => cv.text(self.f.small, "Puente a Discord", RECT { left: z.left + self.px(10), ..z }, INK, DT_LEFT),
                    PHit::ShowDsc => cv.text(self.f.small, "Ventana de Discord", RECT { left: z.left + self.px(10), ..z }, INK, DT_LEFT),
                    PHit::Token => {
                        let t = if self.br_has_token { "Token guardado · pegar otro" } else { "Pegar token del bot (copialo y tocá)" };
                        cv.text(self.f.small, t, z, INK, DT_CENTER);
                    }
                    PHit::Device(i) => cv.text(self.f.small, short_dev(&self.br_devices[i]), inset, INK, DT_LEFT),
                    PHit::AllDevices => cv.text(self.f.small, if self.cfg_all_dev { "Mostrar solo cables" } else { "Mostrar todas las entradas" }, inset, DIM, DT_LEFT),
                    PHit::Browser(i) => cv.text(self.f.small, &self.browsers[i].0, inset, INK, DT_LEFT),
                    PHit::Hotkey(i) => {
                        let (id, name) = EDITABLE[i];
                        cv.text(self.f.small, name, RECT { left: z.left + self.px(10), ..z }, INK, DT_LEFT);
                        let combo = if self.panel_capture == Some(i) {
                            "presioná la combinación…".to_string()
                        } else {
                            self.hotkeys.iter().find(|h| h.0 == id).map(|h| key_name(h.1, h.2)).unwrap_or_default()
                        };
                        let c = if self.panel_capture == Some(i) { AMBER } else { DIM };
                        cv.text(self.f.small, &combo, RECT { right: z.right - self.px(10), ..z }, c, DT_RIGHT);
                    }
                    _ => {}
                }
            }
            GdiFlush();
            cv.finish(RADIUS * s);
            self.panel_canvas = Some(cv);
        }
        self.panel_present();
    }

    /// Coordenadas de la ventana de costado → px del contenido completo.
    fn panel_hit(&self, x: i32, y: i32) -> PHit {
        let x = x + self.panel_sx;
        let inside = |r: RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        if inside(self.r(PW - 36, 4, PW - 8, 32)) {
            return PHit::Close;
        }
        self.panel_layout()
            .0
            .into_iter()
            .find(|&(h, r)| !matches!(h, PHit::Label(_) | PHit::Msg(_)) && inside(self.pr(r)))
            .map_or(PHit::None, |(h, _)| h)
    }

    pub fn panel_click(&mut self, x: i32, y: i32) {
        let hit = self.panel_hit(x, y);
        match hit {
            PHit::Close => self.panel_close(),
            PHit::App(i) => {
                let pid = self.mix_apps[i].pid;
                self.mix_sel = if self.mix_sel == Some(pid) { None } else { Some(pid) };
            }
            PHit::Sw(i) => {
                let a = &mut self.mix_apps[i];
                a.on = !a.on; // optimista: la lista que vuelve confirma lo que dejo Windows
                let v = json!({ "cmd": "route", "pid": a.pid, "on": a.on });
                self.mix_send(v);
            }
            PHit::Vol(i) => {
                self.mix_drag = Some(i);
                unsafe { SetCapture(self.panel_hwnd) };
                self.mix_vol_from_x(i, x);
            }
            PHit::ShowBr | PHit::ShowDsc => {
                if hit == PHit::ShowBr {
                    self.show_br = !self.show_br;
                } else {
                    self.show_dsc = !self.show_dsc;
                }
                let _ = std::fs::write(card_file(), json!({ "puente": self.show_br, "discord": self.show_dsc }).to_string());
                self.invalidate(); // la card cambia de alto; la de costado la sigue
            }
            PHit::Token => self.br_paste_token(),
            PHit::AllDevices => self.cfg_all_dev = !self.cfg_all_dev,
            PHit::Device(i) => {
                self.br_device = self.br_devices[i].clone();
                if self.br_busy() {
                    self.flash("La entrada nueva se usa al reconectar");
                }
            }
            PHit::Browser(i) => {
                let p = self.browsers[i].1.clone();
                let file = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("navegador.txt")));
                match file.map(|f| std::fs::write(f, p.display().to_string())) {
                    Some(Ok(())) => {
                        self.browser_cur = p;
                        self.flash("Navegador cambiado: se usa al reabrir la card");
                    }
                    _ => self.flash("No pude guardar el navegador"),
                }
            }
            PHit::Hotkey(i) => {
                // Para recibir el teclado la card de costado toma el foco hasta que se elija.
                self.panel_capture = if self.panel_capture == Some(i) { None } else { Some(i) };
                if self.panel_capture.is_some() {
                    unsafe { SetForegroundWindow(self.panel_hwnd) };
                }
            }
            _ => {}
        }
        self.panel_render();
    }

    pub fn mix_vol_from_x(&mut self, i: usize, x: i32) {
        let Some(&(_, r)) = self.panel_layout().0.iter().find(|(h, _)| *h == PHit::Vol(i)) else { return };
        let b = self.pr(r);
        let v = ((x + self.panel_sx - b.left) as f64 / (b.right - b.left) as f64).clamp(0.0, 1.0);
        let Some(a) = self.mix_apps.get_mut(i) else { return };
        a.vol = (v * 100.0).round();
        let msg = json!({ "cmd": "vol", "pid": a.pid, "v": v });
        self.mix_send(msg);
        self.panel_render();
    }

    /// Tecla mientras se espera un atajo nuevo. Devuelve true si la uso.
    pub fn panel_key(&mut self, vk: u16) -> bool {
        let Some(i) = self.panel_capture else { return false };
        if vk == VK_ESCAPE {
            self.panel_capture = None;
            self.panel_render();
            return true;
        }
        if matches!(vk, VK_CONTROL | VK_MENU | VK_SHIFT | VK_LWIN | VK_RWIN | VK_LCONTROL | VK_RCONTROL | VK_LMENU | VK_RMENU | VK_LSHIFT | VK_RSHIFT) {
            return true; // falta la tecla principal
        }
        let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k as i32) } < 0;
        let mut mods: HOT_KEY_MODIFIERS = 0;
        if down(VK_CONTROL) {
            mods |= MOD_CONTROL;
        }
        if down(VK_MENU) {
            mods |= MOD_ALT;
        }
        if down(VK_SHIFT) {
            mods |= MOD_SHIFT;
        }
        if mods & (MOD_CONTROL | MOD_ALT) == 0 {
            self.flash("Usá Ctrl o Alt en la combinación");
            return true;
        }
        let id = EDITABLE[i].0;
        let Some(k) = self.hotkeys.iter().position(|h| h.0 == id) else { return true };
        let old = self.hotkeys[k];
        unsafe {
            UnregisterHotKey(self.hwnd, id);
            if RegisterHotKey(self.hwnd, id, mods | MOD_NOREPEAT, vk as u32) == 0 {
                RegisterHotKey(self.hwnd, id, old.1 | MOD_NOREPEAT, old.2);
                self.flash("Esa combinación la usa otra app");
            } else {
                self.hotkeys[k] = (id, mods, vk as u32);
                let saved: serde_json::Map<String, Value> = self.hotkeys.iter().map(|h| (h.0.to_string(), json!([h.1, h.2]))).collect();
                let _ = std::fs::write(hotkeys_file(), Value::Object(saved).to_string());
            }
        }
        self.panel_capture = None;
        self.panel_render();
        true
    }
}

pub unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let a = app();
    let (mx, my) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
    match msg {
        // Sin robar el foco, salvo cuando espera un atajo nuevo.
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_LBUTTONDOWN => {
            a.panel_click(mx, my);
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            if a.mix_drag.take().is_some() {
                ReleaseCapture();
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(i) = a.mix_drag {
                a.mix_vol_from_x(i, mx);
                return 0;
            }
            if !a.panel_tracking {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut t);
                a.panel_tracking = true;
            }
            let h = a.panel_hit(mx, my);
            if h != a.panel_hover {
                a.panel_hover = h;
                a.panel_render();
            }
            0
        }
        WM_SETCURSOR => {
            if (lp & 0xffff) as u32 == HTCLIENT && a.panel_hover != PHit::None {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                1
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_MOUSELEAVE_ => {
            a.panel_tracking = false;
            if a.panel_hover != PHit::None {
                a.panel_hover = PHit::None;
                a.panel_render();
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if a.panel_key(wp as u16) {
                0
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_KILLFOCUS => {
            if a.panel_capture.take().is_some() {
                a.panel_render();
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Crea la ventana de costado (oculta), duenia de la card: queda siempre arriba de ella.
pub unsafe fn create(owner: HWND, hinst: HINSTANCE) -> HWND {
    let cls = wide("ytm-float-panel");
    let wc = WNDCLASSW { lpfnWndProc: Some(panel_proc), hInstance: hinst, hCursor: LoadCursorW(null_mut(), IDC_ARROW), lpszClassName: cls.as_ptr(), ..zeroed() };
    RegisterClassW(&wc);
    CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, cls.as_ptr(), wide("YTM panel").as_ptr(), WS_POPUP, 0, 0, 1, 1, owner, null_mut(), hinst, null())
}
