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
    Section(usize),
    Style(bool),
    Color(u8),
    Preset(u8, usize),
    Opacity,
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

// Paletas rapidas (COLORREF); el campo hex permite cualquier otro color.
const PRESET_SOLID: [u32; 7] = [rgb(0, 0, 0), rgb(0x0d, 0x0f, 0x14), rgb(0x0b, 0x12, 0x20), rgb(0x1a, 0x0f, 0x14), rgb(0x0c, 0x15, 0x11), rgb(0x1f, 0x1f, 0x22), rgb(0xf4, 0xf1, 0xea)];
const PRESET_TINT: [u32; 7] = [rgb(0x10, 0x12, 0x1c), rgb(0x1a, 0x10, 0x30), rgb(0x0a, 0x1f, 0x2a), rgb(0x2a, 0x0f, 0x18), rgb(0x10, 0x10, 0x10), rgb(0x1f, 0x1a, 0x10), rgb(0xe8, 0xee, 0xf7)];
const PRESET_ACCENT: [u32; 7] = [rgb(0x63, 0x66, 0xf1), rgb(0x8b, 0x5c, 0xf6), rgb(0xec, 0x48, 0x99), rgb(0xf9, 0x73, 0x16), rgb(0x22, 0xc5, 0x5e), rgb(0x06, 0xb6, 0xd4), rgb(0xea, 0xb3, 0x08)];
const SECTIONS: [&str; 6] = ["EN LA CARD", "APARIENCIA", "BOT DE DISCORD", "ENTRADA DEL PUENTE", "NAVEGADOR", "ATAJOS"];

/// Dos colores casi iguales (para marcar un circulo que se confundiria con el fondo).
fn lum_close(a: u32, b: u32) -> bool {
    let d = |x: u32, y: u32| ((x & 0xff) as i32 - (y & 0xff) as i32).abs() + (((x >> 8) & 0xff) as i32 - ((y >> 8) & 0xff) as i32).abs() + (((x >> 16) & 0xff) as i32 - ((y >> 16) & 0xff) as i32).abs();
    d(a, b) < 40
}

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
                // Cada seccion es plegable: la cabecera siempre se ve, las filas solo si esta abierta.
                let head = |i: usize, y: &mut i32, v: &mut Vec<(PHit, RECT)>| {
                    v.push((PHit::Section(i), RECT { left: 12, top: *y + 2, right: PW - 12, bottom: *y + 26 }));
                    *y += 28;
                };
                let open = |i: usize| self.sec_open[i];
                if self.br.is_some() || self.dsc_avail {
                    head(0, &mut y, &mut v);
                    if open(0) {
                        for (h, ok) in [(PHit::ShowBr, self.br.is_some()), (PHit::ShowDsc, self.dsc_avail)] {
                            if ok {
                                row(h, 12, PW - 12, y, y + P_ROW, &mut v);
                                y += P_ROW;
                            }
                        }
                    }
                }
                head(1, &mut y, &mut v);
                if open(1) {
                    let t = th();
                    row(PHit::Style(false), 12, PW / 2 - 1, y, y + P_ROW, &mut v);
                    row(PHit::Style(true), PW / 2 + 1, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW + 4;
                    row(PHit::Color(0), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                    row(PHit::Preset(0, 0), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                    row(PHit::Color(1), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                    row(PHit::Preset(1, 0), 12, PW - 12, y, y + P_ROW, &mut v);
                    y += P_ROW;
                    if t.glass {
                        row(PHit::Opacity, 12, PW - 12, y, y + P_ROW, &mut v);
                        y += P_ROW;
                    }
                }
                if self.br.is_some() {
                    head(2, &mut y, &mut v);
                    if open(2) {
                        row(PHit::Token, 12, PW - 12, y, y + 30, &mut v);
                        y += 34;
                    }
                    head(3, &mut y, &mut v);
                    if open(3) {
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
                }
                head(4, &mut y, &mut v);
                if open(4) {
                    for i in 0..self.browsers.len() {
                        row(PHit::Browser(i), 12, PW - 12, y, y + P_ROW, &mut v);
                        y += P_ROW;
                    }
                }
                head(5, &mut y, &mut v);
                if open(5) {
                    for i in 0..EDITABLE.len() {
                        row(PHit::Hotkey(i), 12, PW - 12, y, y + P_ROW, &mut v);
                        y += P_ROW;
                    }
                }
            }
            Panel::None => {}
        }
        (v, y + 12)
    }

    fn pr(&self, r: RECT) -> RECT {
        self.r(r.left, r.top, r.right, r.bottom)
    }

    fn presets(&self, which: u8) -> &'static [u32; 7] {
        match (which, th().glass) {
            (1, _) => &PRESET_ACCENT,
            (_, true) => &PRESET_TINT,
            _ => &PRESET_SOLID,
        }
    }

    /// Centro (px logicos, desde el borde izquierdo de la fila) del circulo i de una paleta.
    fn preset_x(&self, i: usize) -> i32 {
        26 + i as i32 * (PW - 24 - 52) / 6
    }

    /// Cambia el tema, lo guarda y repinta las dos cards.
    fn theme_apply(&mut self, t: theme::Theme, save: bool) {
        if save {
            t.save();
        }
        let glass_changed = t.glass != th().glass;
        theme::set(t);
        if glass_changed {
            unsafe {
                let old = std::mem::replace(&mut self.f, make_fonts(self.scale, th().glass));
                for f in [old.title, old.text, old.small, old.icon, old.icon_big] {
                    DeleteObject(f);
                }
            }
        }
        self.panel_render();
        self.invalidate();
    }

    fn theme_edit(&self) -> theme::Theme {
        th().clone()
    }

    /// Color elegido (0 fondo/tinte, 1 acento) en el tema `t`.
    fn theme_set_color(t: &mut theme::Theme, which: u8, c: u32) {
        match which {
            0 if t.glass => t.glass_tint = c,
            0 => t.solid_bg = c,
            _ => t.accent = c,
        }
        t.derive();
    }

    fn opacity_from_x(&mut self, x: i32, save: bool) {
        let Some(&(_, r)) = self.panel_layout().0.iter().find(|(h, _)| *h == PHit::Opacity) else { return };
        let (l, rr) = (self.px(r.left + 94), self.px(r.right - 38));
        let f = ((x + self.panel_sx - l) as f32 / (rr - l) as f32).clamp(0.0, 1.0);
        let mut t = self.theme_edit();
        t.opacity = theme::OPACITY_MIN + f * (theme::OPACITY_MAX - theme::OPACITY_MIN);
        t.derive();
        self.theme_apply(t, save);
    }

    /// Caracter tecleado mientras se edita un color hex. Enter confirma, Esc deja el color de antes.
    pub fn panel_char(&mut self, c: u16) {
        let Some((which, mut buf, orig)) = self.panel_edit.take() else { return };
        let mut done = false;
        match c {
            13 => done = true,
            27 => {
                let mut t = self.theme_edit();
                Self::theme_set_color(&mut t, which, orig);
                self.theme_apply(t, true);
                self.panel_render();
                return;
            }
            8 => {
                buf.pop();
            }
            22 => buf = unsafe { clipboard() }.chars().filter(|c| c.is_ascii_hexdigit()).take(6).collect(),
            c if (c as u8 as char).is_ascii_hexdigit() && c < 128 && buf.len() < 6 => buf.push(c as u8 as char),
            _ => {}
        }
        // Con 3 o 6 digitos validos se ve en vivo.
        if let Some(col) = theme::parse_hex(&buf).filter(|_| buf.len() == 3 || buf.len() == 6) {
            let mut t = self.theme_edit();
            Self::theme_set_color(&mut t, which, col);
            self.theme_apply(t, done);
        }
        self.panel_edit = if done { None } else { Some((which, buf, orig)) };
        self.panel_render();
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
            let mut c = self.blur_panel;
            theme::blur(self.panel_hwnd, &mut c, vw, pw, -self.panel_sx, cv.h, (RADIUS * self.scale).round() as i32);
            self.blur_panel = c;
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
            cv.fill_bg();
            let (wf, hf, s) = (w as f32, h as f32, self.scale);
            cv.round(0.0, 0.0, wf, hf, RADIUS * s, th().hairline);
            cv.round(1.0, 1.0, wf - 1.0, hf - 1.0, RADIUS * s - 1.0, th().bg);
            let hov = self.panel_hover;
            if hov == PHit::Close {
                let z = self.r(PW - 36, 4, PW - 8, 32);
                cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(14.0), th().tab);
            }
            // Formas
            for &(hit, r) in &rows {
                let z = self.pr(r);
                let (l, t, rr, b) = (z.left as f32, z.top as f32, z.right as f32, z.bottom as f32);
                match hit {
                    PHit::Sw(i) => {
                        let on = self.mix_apps[i].on;
                        cv.round(l, t, rr, b, (b - t) / 2.0, if on { GREEN } else { th().hover });
                        let k = (b - t) / 2.0 - self.pf(2.5);
                        cv.dot(if on { rr - (b - t) / 2.0 } else { l + (b - t) / 2.0 }, (t + b) / 2.0, k, th().ink);
                    }
                    PHit::Vol(i) => {
                        let y = (t + b) / 2.0;
                        cv.round(l, y - self.pf(2.0), rr, y + self.pf(2.0), self.pf(2.0), th().hover);
                        let x = l + (rr - l) * (self.mix_apps[i].vol / 100.0) as f32;
                        if x - l > 1.0 {
                            cv.round(l, y - self.pf(2.0), x, y + self.pf(2.0), self.pf(2.0), th().accent);
                        }
                        cv.dot(x, y, self.pf(5.0), th().ink);
                    }
                    PHit::Section(_) => {
                        if hov == hit {
                            cv.round(l, t, rr, b, self.pf(8.0), th().hover);
                        }
                    }
                    PHit::Style(g) => cv.round(l, t, rr, b, self.pf(9.0), if th().glass == g { th().accent } else if hov == hit { th().hover } else { th().tab }),
                    PHit::Color(w) => {
                        if hov == hit || self.panel_edit.as_ref().is_some_and(|e| e.0 == w) {
                            cv.round(l, t, rr, b, self.pf(8.0), th().hover);
                        }
                        let c = if w == 0 { th().base() } else { th().accent };
                        let (cx, cy) = (rr - self.pf(16.0), (t + b) / 2.0);
                        cv.dot(cx, cy, self.pf(8.5), th().dim3);
                        cv.dot(cx, cy, self.pf(7.0), c);
                    }
                    PHit::Preset(w, _) => {
                        let cur = if w == 0 { th().base() } else { th().accent };
                        for (i, c) in self.presets(w).iter().enumerate() {
                            let cx = l + self.pf(self.preset_x(i) as f32 - 12.0);
                            let cy = (t + b) / 2.0;
                            if *c == cur {
                                cv.dot(cx, cy, self.pf(9.5), th().ink);
                                cv.dot(cx, cy, self.pf(8.0), th().bg);
                            }
                            cv.dot(cx, cy, self.pf(6.5), *c);
                            if lum_close(*c, th().bg) {
                                cv.dot(cx, cy, self.pf(6.5), th().dim3);
                                cv.dot(cx, cy, self.pf(5.5), *c);
                            }
                        }
                    }
                    PHit::Opacity => {
                        let (tl, tr, y) = (l + self.pf(82.0), rr - self.pf(38.0), (t + b) / 2.0);
                        cv.round(tl, y - self.pf(2.0), tr, y + self.pf(2.0), self.pf(2.0), th().hover);
                        let f = (th().opacity - theme::OPACITY_MIN) / (theme::OPACITY_MAX - theme::OPACITY_MIN);
                        let x = tl + (tr - tl) * f;
                        cv.round(tl, y - self.pf(2.0), x, y + self.pf(2.0), self.pf(2.0), th().accent);
                        cv.dot(x, y, self.pf(5.0), th().ink);
                    }
                    PHit::Token => cv.round(l, t, rr, b, self.pf(10.0), if hov == hit { th().accent } else { th().tab }),
                    PHit::ShowBr | PHit::ShowDsc => {
                        if hov == hit {
                            cv.round(l, t, rr, b, self.pf(8.0), th().hover);
                        }
                        let on = if hit == PHit::ShowBr { self.show_br } else { self.show_dsc };
                        let (sl, st, sr, sb) = (rr - self.pf(42.0), t + self.pf(5.0), rr - self.pf(4.0), b - self.pf(5.0));
                        cv.round(sl, st, sr, sb, (sb - st) / 2.0, if on { GREEN } else { th().hover });
                        let k = (sb - st) / 2.0 - self.pf(2.5);
                        cv.dot(if on { sr - (sb - st) / 2.0 } else { sl + (sb - st) / 2.0 }, (st + sb) / 2.0, k, th().ink);
                    }
                    PHit::App(_) | PHit::Device(_) | PHit::AllDevices | PHit::Browser(_) | PHit::Hotkey(_) => {
                        if hov == hit || self.panel_capture.is_some_and(|c| hit == PHit::Hotkey(c)) {
                            cv.round(l, t, rr, b, self.pf(8.0), th().hover);
                        }
                        let sel = match hit {
                            PHit::Device(i) => Some(self.br_devices[i] == self.br_device),
                            PHit::Browser(i) => Some(self.browsers[i].1 == self.browser_cur),
                            _ => None,
                        };
                        if let Some(sel) = sel {
                            let (cx, cy) = (l + self.pf(14.0), (t + b) / 2.0);
                            cv.dot(cx, cy, self.pf(5.0), if sel { th().accent } else { th().hover });
                            if sel {
                                cv.dot(cx, cy, self.pf(2.0), th().ink);
                            }
                        }
                    }
                    _ => {}
                }
            }
            GdiFlush();
            // Texto
            let title = if self.panel == Panel::Apps { "Apps a Discord" } else { "Configuración" };
            cv.text(self.f.title, title, self.r(16, 8, PW - 40, 32), th().ink, DT_LEFT);
            cv.text(self.f.icon, "\u{E8BB}", self.r(PW - 36, 4, PW - 8, 32), if hov == PHit::Close { th().ink } else { th().dim }, DT_CENTER);
            for &(hit, r) in &rows {
                let z = self.pr(r);
                let inset = RECT { left: z.left + self.px(30), right: z.right - self.px(8), ..z };
                match hit {
                    PHit::Section(i) => {
                        cv.text(self.f.small, SECTIONS[i], RECT { left: z.left + self.px(4), ..z }, th().dim3, DT_LEFT);
                        let ch = if self.sec_open[i] { "\u{E70E}" } else { "\u{E70D}" };
                        cv.text(self.f.icon, ch, RECT { left: z.right - self.px(24), ..z }, th().dim3, DT_CENTER);
                    }
                    PHit::Style(g) => {
                        let on = th().glass == g;
                        cv.text(self.f.small, if g { "Cristal" } else { "Sólido" }, z, if on { theme::on(th().accent) } else { th().dim }, DT_CENTER);
                    }
                    PHit::Color(w) => {
                        let name = if w == 1 { "Acento" } else if th().glass { "Tinte" } else { "Fondo" };
                        cv.text(self.f.small, name, RECT { left: z.left + self.px(10), ..z }, th().ink, DT_LEFT);
                        let val = match &self.panel_edit {
                            Some((e, buf, _)) if *e == w => format!("#{buf}_"),
                            _ => theme::hex(if w == 0 { th().base() } else { th().accent }),
                        };
                        let c = if self.panel_edit.as_ref().is_some_and(|e| e.0 == w) { AMBER } else { th().dim };
                        cv.text(self.f.small, &val, RECT { right: z.right - self.px(32), ..z }, c, DT_RIGHT);
                    }
                    PHit::Opacity => {
                        cv.text(self.f.small, "Opacidad", RECT { left: z.left + self.px(10), ..z }, th().ink, DT_LEFT);
                        let v = format!("{}%", (th().opacity * 100.0).round() as i32);
                        cv.text(self.f.small, &v, RECT { left: z.right - self.px(36), ..z }, th().dim, DT_CENTER);
                    }
                    PHit::Msg(t) => cv.text(self.f.small, t, z, th().dim, DT_CENTER),
                    PHit::App(i) => {
                        let a = &self.mix_apps[i];
                        cv.text(self.f.small, &a.name, RECT { left: z.left + self.px(10), ..z }, if a.on { th().ink } else { th().dim }, DT_LEFT);
                    }
                    PHit::Vol(i) => {
                        let v = format!("{}", self.mix_apps[i].vol as i32);
                        cv.text(self.f.small, &v, RECT { left: z.right + self.px(6), right: z.right + self.px(40), top: z.top - self.px(6), bottom: z.bottom + self.px(6) }, th().dim, DT_CENTER);
                    }
                    PHit::ShowBr => cv.text(self.f.small, "Puente a Discord", RECT { left: z.left + self.px(10), ..z }, th().ink, DT_LEFT),
                    PHit::ShowDsc => cv.text(self.f.small, "Ventana de Discord", RECT { left: z.left + self.px(10), ..z }, th().ink, DT_LEFT),
                    PHit::Token => {
                        let t = if self.br_has_token { "Token guardado · pegar otro" } else { "Pegar token del bot (copialo y tocá)" };
                        cv.text(self.f.small, t, z, th().ink, DT_CENTER);
                    }
                    PHit::Device(i) => cv.text(self.f.small, short_dev(&self.br_devices[i]), inset, th().ink, DT_LEFT),
                    PHit::AllDevices => cv.text(self.f.small, if self.cfg_all_dev { "Mostrar solo cables" } else { "Mostrar todas las entradas" }, inset, th().dim, DT_LEFT),
                    PHit::Browser(i) => cv.text(self.f.small, &self.browsers[i].0, inset, th().ink, DT_LEFT),
                    PHit::Hotkey(i) => {
                        let (id, name) = EDITABLE[i];
                        cv.text(self.f.small, name, RECT { left: z.left + self.px(10), ..z }, th().ink, DT_LEFT);
                        let combo = if self.panel_capture == Some(i) {
                            "presioná la combinación…".to_string()
                        } else {
                            self.hotkeys.iter().find(|h| h.0 == id).map(|h| key_name(h.1, h.2)).unwrap_or_default()
                        };
                        let c = if self.panel_capture == Some(i) { AMBER } else { th().dim };
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
            .find(|&(h, r)| !matches!(h, PHit::Msg(_)) && inside(self.pr(r)))
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
            PHit::Section(i) => self.sec_open[i] = !self.sec_open[i],
            PHit::Style(g) => {
                if th().glass != g {
                    let mut t = self.theme_edit();
                    t.glass = g;
                    t.derive();
                    self.theme_apply(t, true);
                }
            }
            PHit::Color(w) => {
                if self.panel_edit.as_ref().is_some_and(|e| e.0 == w) {
                    self.panel_edit = None;
                } else {
                    let cur = if w == 0 { th().base() } else { th().accent };
                    self.panel_edit = Some((w, String::new(), cur));
                    // Para recibir el teclado, la card de costado toma el foco hasta confirmar.
                    unsafe { SetForegroundWindow(self.panel_hwnd) };
                }
            }
            PHit::Preset(w, _) => {
                let rows = self.panel_layout().0;
                if let Some(&(_, r)) = rows.iter().find(|(h, _)| matches!(h, PHit::Preset(k, _) if *k == w)) {
                    let rel = x + self.panel_sx - self.px(r.left);
                    let i = (0..7).min_by_key(|&i| (rel - self.px(self.preset_x(i) - 12)).abs()).unwrap_or(0);
                    let c = self.presets(w)[i];
                    let mut t = self.theme_edit();
                    Self::theme_set_color(&mut t, w, c);
                    self.theme_apply(t, true);
                }
            }
            PHit::Opacity => {
                self.op_drag = true;
                unsafe { SetCapture(self.panel_hwnd) };
                self.opacity_from_x(x, false);
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
            if a.op_drag {
                a.op_drag = false;
                ReleaseCapture();
                th().save();
            }
            0
        }
        WM_CHAR => {
            a.panel_char(wp as u16);
            0
        }
        WM_MOUSEMOVE => {
            if let Some(i) = a.mix_drag {
                a.mix_vol_from_x(i, mx);
                return 0;
            }
            if a.op_drag {
                a.opacity_from_x(mx, false);
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
            let ended = a.panel_capture.take().is_some() | a.panel_edit.take().is_some();
            if ended {
                th().save();
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
