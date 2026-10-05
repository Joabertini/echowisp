#![windows_subsystem = "windows"]
// Widget flotante: Win32 + GDI a mano, doble buffer, sin frameworks.
mod brave;
mod cdp;
mod engine;

use engine::{Engine, Ev, WM_ENGINE};
use serde_json::{json, Value};
use std::{
    mem::zeroed,
    ptr::{null, null_mut},
    sync::Arc,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

use brave::wide;

// Medidas en px a 96 dpi; se escalan con `px`.
const W: i32 = 340;
const HDR: i32 = 50;
const BAR: i32 = 3;
const SRCH: i32 = 36;
const ROW: i32 = 38;
const MAX_ROWS: usize = 8;

const C_BG: u32 = rgb(18, 18, 18);
const C_FIELD: u32 = rgb(34, 34, 34);
const C_HOVER: u32 = rgb(42, 42, 42);
const C_TEXT: u32 = rgb(236, 236, 236);
const C_SUB: u32 = rgb(140, 140, 140);
const C_ACCENT: u32 = rgb(255, 78, 69);
const C_BORDER: u32 = rgb(48, 48, 48);

const WM_MOUSELEAVE_: u32 = 0x02A3;
const T_SEARCH: usize = 1;
const T_TICK: usize = 2;
const T_MSG: usize = 3;

const K_SEARCH: u32 = 1 << 24;
const K_LISTS: u32 = 2 << 24;
const K_OTHER: u32 = 3 << 24;
const K_MASK: u32 = 0xff << 24;

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    None,
    Prev,
    Play,
    Next,
    Close,
    Login,
    Lists,
    Bar,
    Row(usize),
    Radio(usize),
}

struct Item {
    id: Option<String>,
    list: Option<String>,
    title: String,
    sub: String,
}

struct App {
    hwnd: HWND,
    edit: HWND,
    eng: Arc<Engine>,
    scale: f32,
    f_bold: HFONT,
    f_text: HFONT,
    f_small: HFONT,
    f_icon: HFONT,
    br_field: HBRUSH,
    title: String,
    artist: String,
    status: String,
    msg: String,
    paused: bool,
    pos: f64,
    dur: f64,
    at: Instant,
    logged: bool,
    ready: bool,
    items: Vec<Item>,
    scroll: usize,
    sel: Option<usize>,
    hover: Hit,
    tracking: bool,
    seq: u32,
}

static mut APP: *mut App = null_mut();
#[allow(static_mut_refs)]
fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

impl App {
    fn px(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }
    fn width(&self) -> i32 {
        self.px(W)
    }
    fn rows_visible(&self) -> usize {
        self.items.len().min(MAX_ROWS)
    }
    fn height(&self) -> i32 {
        let base = HDR + BAR + SRCH;
        let n = self.rows_visible() as i32;
        self.px(if n > 0 { base + n * ROW + 6 } else { base })
    }
    fn r(&self, l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT { left: self.px(l), top: self.px(t), right: self.px(r), bottom: self.px(b) }
    }
    // Botones del encabezado, de derecha a izquierda.
    fn btn(&self, h: Hit) -> RECT {
        let (y0, y1) = (9, 41);
        match h {
            Hit::Close => self.r(W - 32, y0, W - 4, y1),
            Hit::Next => self.r(W - 66, y0, W - 34, y1),
            Hit::Play => self.r(W - 100, y0, W - 66, y1),
            Hit::Prev => self.r(W - 134, y0, W - 102, y1),
            Hit::Login => self.r(W - 168, y0, W - 136, y1),
            Hit::Lists => self.r(W - 64, HDR + BAR + 6, W - 8, HDR + BAR + 30),
            Hit::Bar => self.r(0, HDR - 4, W, HDR + BAR + 3),
            _ => RECT { left: 0, top: 0, right: 0, bottom: 0 },
        }
    }
    fn text_right(&self) -> i32 {
        if self.logged || !self.ready { W - 140 } else { W - 174 }
    }
    fn row_rect(&self, vis: usize) -> RECT {
        let y = HDR + BAR + SRCH + vis as i32 * ROW;
        self.r(4, y, W - 4, y + ROW)
    }
    fn radio_rect(&self, vis: usize) -> RECT {
        let y = HDR + BAR + SRCH + vis as i32 * ROW;
        self.r(W - 62, y + 8, W - 10, y + ROW - 8)
    }

    fn hit(&self, x: i32, y: i32) -> Hit {
        let inside = |r: RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        let mut hs = vec![Hit::Close, Hit::Next, Hit::Play, Hit::Prev, Hit::Lists];
        if self.ready && !self.logged {
            hs.push(Hit::Login);
        }
        if let Some(h) = hs.into_iter().find(|&h| inside(self.btn(h))) {
            return h;
        }
        if self.dur > 0.0 && inside(self.btn(Hit::Bar)) {
            return Hit::Bar;
        }
        for vis in 0..self.rows_visible() {
            let i = self.scroll + vis;
            if inside(self.row_rect(vis)) {
                let song = self.items[i].id.is_some();
                return if song && inside(self.radio_rect(vis)) { Hit::Radio(i) } else { Hit::Row(i) };
            }
        }
        Hit::None
    }

    fn cur_pos(&self) -> f64 {
        let p = if self.paused { self.pos } else { self.pos + self.at.elapsed().as_secs_f64() };
        if self.dur > 0.0 { p.min(self.dur) } else { p }
    }

    fn relayout(&mut self) {
        let (w, h) = (self.width(), self.height());
        unsafe {
            // Si al crecer se sale del monitor, se corre hacia arriba lo justo.
            let mut r: RECT = zeroed();
            GetWindowRect(self.hwnd, &mut r);
            let mut mi: MONITORINFO = zeroed();
            mi.cbSize = size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
            let top = r.top.min(mi.rcWork.bottom - h).max(mi.rcWork.top);
            SetWindowPos(self.hwnd, null_mut(), r.left, top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.invalidate();
    }
    fn invalidate(&self) {
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }
    fn flash(&mut self, m: &str) {
        self.msg = m.to_string();
        unsafe { SetTimer(self.hwnd, T_MSG, 4000, None) };
        self.invalidate();
    }

    fn set_items(&mut self, items: Vec<Item>) {
        self.items = items;
        self.scroll = 0;
        self.sel = if self.items.is_empty() { None } else { Some(0) };
        self.relayout();
    }

    fn clear_search(&mut self) {
        self.seq += 1; // descarta respuestas en vuelo
        unsafe { SetWindowTextW(self.edit, wide("").as_ptr()) };
        self.set_items(Vec::new());
    }

    fn play(&mut self, i: usize, radio: bool) {
        let Some(it) = self.items.get(i) else { return };
        let arg = match (&it.id, &it.list) {
            (Some(v), _) => json!({ "v": v, "radio": radio }),
            (_, Some(l)) => json!({ "list": l }),
            _ => return,
        };
        self.eng.call("play", arg, K_OTHER);
        self.clear_search();
    }

    fn cmd(&mut self, c: &str) {
        self.eng.call(c, Value::Null, K_OTHER);
    }

    fn click(&mut self, x: i32, _y: i32) {
        match self.hit(x, _y) {
            Hit::Close => unsafe {
                DestroyWindow(self.hwnd);
            },
            Hit::Prev => self.cmd("prev"),
            Hit::Next => self.cmd("next"),
            Hit::Play => {
                // Respuesta instantanea en la UI; el evento de la pagina confirma despues.
                self.pos = self.cur_pos();
                self.at = Instant::now();
                self.paused = !self.paused;
                self.update_tick();
                self.invalidate();
                self.cmd("toggle");
            }
            Hit::Login => {
                let eng = self.eng.clone();
                std::thread::spawn(move || eng.login());
            }
            Hit::Lists => {
                self.seq += 1;
                self.eng.call("playlists", Value::Null, K_LISTS | (self.seq & 0xffffff));
                self.flash("Cargando listas…");
            }
            Hit::Bar => {
                let b = self.btn(Hit::Bar);
                let t = ((x - b.left) as f64 / (b.right - b.left) as f64).clamp(0.0, 1.0) * self.dur;
                self.pos = t;
                self.at = Instant::now();
                self.invalidate();
                self.eng.call("seek", json!(t), K_OTHER);
            }
            Hit::Row(i) => self.play(i, false),
            Hit::Radio(i) => self.play(i, true),
            Hit::None => {}
        }
    }

    fn update_tick(&self) {
        unsafe {
            if !self.paused && self.dur > 0.0 {
                SetTimer(self.hwnd, T_TICK, 500, None);
            } else {
                KillTimer(self.hwnd, T_TICK);
            }
        }
    }

    fn on_engine(&mut self, ev: Ev) {
        match ev {
            Ev::Status(s) => {
                self.status = s;
                self.ready = false;
            }
            Ev::State(s) => {
                self.ready = true;
                self.title = s["title"].as_str().unwrap_or_default().to_string();
                self.artist = s["artist"].as_str().unwrap_or_default().to_string();
                self.paused = s["paused"].as_bool().unwrap_or(true);
                self.pos = s["pos"].as_f64().unwrap_or(0.0);
                self.dur = s["dur"].as_f64().unwrap_or(0.0);
                self.logged = s["logged"].as_bool().unwrap_or(false);
                self.at = Instant::now();
                self.update_tick();
            }
            Ev::Reply(tag, r) => {
                let ok = r["ok"].as_bool().unwrap_or(false);
                let kind = tag & K_MASK;
                if !ok {
                    if kind == K_OTHER || (tag & 0xffffff) == (self.seq & 0xffffff) {
                        self.flash(r["error"].as_str().unwrap_or("error"));
                    }
                    return;
                }
                if (kind == K_SEARCH || kind == K_LISTS) && (tag & 0xffffff) == (self.seq & 0xffffff) {
                    let items: Vec<Item> = r["data"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| Item {
                                    id: x["id"].as_str().map(String::from),
                                    list: x["list"].as_str().map(String::from),
                                    title: x["title"].as_str().unwrap_or_default().to_string(),
                                    sub: x["sub"].as_str().unwrap_or_default().to_string(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.msg.clear();
                    if items.is_empty() {
                        self.flash(if kind == K_LISTS { "Sin listas (¿sesión iniciada?)" } else { "Sin resultados" });
                    }
                    self.set_items(items);
                    if kind == K_LISTS {
                        unsafe { SetFocus(self.edit) };
                    }
                } else if kind == K_OTHER && r["data"]["spa"] == false {
                    self.flash("Recargando YouTube Music…");
                }
            }
        }
        self.invalidate();
    }

    fn key(&mut self, vk: u16) -> bool {
        let n = self.items.len();
        match vk {
            VK_DOWN | VK_UP if n > 0 => {
                let s = self.sel.unwrap_or(0);
                let s = if vk == VK_DOWN { (s + 1).min(n - 1) } else { s.saturating_sub(1) };
                self.sel = Some(s);
                if s < self.scroll {
                    self.scroll = s;
                } else if s >= self.scroll + MAX_ROWS {
                    self.scroll = s + 1 - MAX_ROWS;
                }
                self.invalidate();
                true
            }
            VK_RETURN => {
                if let Some(s) = self.sel {
                    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
                    self.play(s, shift); // Shift+Enter = radio
                }
                true
            }
            VK_ESCAPE => {
                self.clear_search();
                true
            }
            _ => false,
        }
    }

    unsafe fn paint(&mut self, hdc: HDC) {
        let (w, h) = (self.width(), self.height());
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, bmp);
        SetBkMode(mem, TRANSPARENT as _);

        fill(mem, &RECT { left: 0, top: 0, right: w, bottom: h }, C_BG);

        // Encabezado: titulo/artista o estado.
        let tr = self.text_right();
        let (line1, line2) = if !self.ready {
            (self.status.as_str(), "")
        } else if self.title.is_empty() {
            ("Nada sonando", if self.logged { "Buscá una canción o abrí tus listas" } else { "Sin sesión · tocá el ícono de usuario para entrar" })
        } else {
            (self.title.as_str(), self.artist.as_str())
        };
        let line2 = if self.msg.is_empty() { line2 } else { self.msg.as_str() };
        text(mem, self.f_bold, line1, self.r(12, 8, tr, 27), C_TEXT, DT_LEFT);
        text(mem, self.f_small, line2, self.r(12, 27, tr, 44), if self.msg.is_empty() { C_SUB } else { C_ACCENT }, DT_LEFT);

        let mut btns = vec![(Hit::Prev, "\u{E892}"), (Hit::Play, if self.paused { "\u{E768}" } else { "\u{E769}" }), (Hit::Next, "\u{E893}"), (Hit::Close, "\u{E8BB}")];
        if self.ready && !self.logged {
            btns.push((Hit::Login, "\u{E77B}"));
        }
        for (hb, glyph) in btns {
            let r = self.btn(hb);
            if self.hover == hb {
                fill(mem, &r, C_HOVER);
            }
            text(mem, self.f_icon, glyph, r, C_TEXT, DT_CENTER);
        }

        // Barra de progreso.
        let bar = self.r(0, HDR, W, HDR + BAR);
        fill(mem, &bar, C_FIELD);
        if self.dur > 0.0 {
            let f = (self.cur_pos() / self.dur).clamp(0.0, 1.0);
            let mut done = bar;
            done.right = bar.left + ((bar.right - bar.left) as f64 * f) as i32;
            fill(mem, &done, C_ACCENT);
        }

        // Fila de busqueda: lupa + EDIT (control hijo) + boton Listas.
        let y0 = HDR + BAR;
        fill(mem, &self.r(8, y0 + 6, W - 70, y0 + 30), C_FIELD);
        text(mem, self.f_icon, "\u{E721}", self.r(12, y0 + 6, 30, y0 + 30), C_SUB, DT_CENTER);
        let lr = self.btn(Hit::Lists);
        fill(mem, &lr, if self.hover == Hit::Lists { C_HOVER } else { C_FIELD });
        text(mem, self.f_small, "Listas", lr, C_TEXT, DT_CENTER);

        // Resultados.
        for vis in 0..self.rows_visible() {
            let i = self.scroll + vis;
            let it = &self.items[i];
            let rr = self.row_rect(vis);
            let hot = matches!(self.hover, Hit::Row(j) | Hit::Radio(j) if j == i);
            if hot || self.sel == Some(i) {
                fill(mem, &rr, C_HOVER);
            }
            let y = HDR + BAR + SRCH + vis as i32 * ROW;
            let right = if it.id.is_some() { W - 66 } else { W - 12 };
            text(mem, self.f_text, &it.title, self.r(12, y + 3, right, y + 20), C_TEXT, DT_LEFT);
            text(mem, self.f_small, &it.sub, self.r(12, y + 20, right, y + 35), C_SUB, DT_LEFT);
            if it.id.is_some() && (hot || self.sel == Some(i)) {
                let rb = self.radio_rect(vis);
                if self.hover == Hit::Radio(i) {
                    fill(mem, &rb, C_FIELD);
                }
                text(mem, self.f_small, "Radio", rb, C_SUB, DT_CENTER);
            }
        }
        if self.items.len() > MAX_ROWS {
            // Indicador de scroll fino a la derecha.
            let top = HDR + BAR + SRCH;
            let span = MAX_ROWS as i32 * ROW;
            let th = (span * MAX_ROWS as i32 / self.items.len() as i32).max(12);
            let ty = top + (span - th) * self.scroll as i32 / (self.items.len() - MAX_ROWS) as i32;
            fill(mem, &self.r(W - 3, ty, W - 1, ty + th), C_BORDER);
        }

        // Borde de 1 px.
        let pen = CreatePen(PS_SOLID, 1, C_BORDER);
        let op = SelectObject(mem, pen);
        let ob = SelectObject(mem, GetStockObject(NULL_BRUSH));
        Rectangle(mem, 0, 0, w, h);
        SelectObject(mem, ob);
        SelectObject(mem, op);
        DeleteObject(pen);

        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
    }
}

unsafe fn fill(dc: HDC, r: &RECT, c: u32) {
    SetBkColor(dc, c);
    ExtTextOutW(dc, 0, 0, ETO_OPAQUE, r, null(), 0, null());
}

unsafe fn text(dc: HDC, f: HFONT, s: &str, mut r: RECT, c: u32, align: DRAW_TEXT_FORMAT) {
    if s.is_empty() {
        return; // DrawTextW con puntero de Vec vacio revienta
    }
    let w: Vec<u16> = s.encode_utf16().collect();
    let old = SelectObject(dc, f);
    SetTextColor(dc, c);
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    SelectObject(dc, old);
}

unsafe fn font(px: i32, weight: i32, face: &str) -> HFONT {
    CreateFontW(-px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32, DEFAULT_PITCH as u32, wide(face).as_ptr())
}

fn pos_file() -> std::path::PathBuf {
    brave::data_dir().join("pos.txt")
}

unsafe fn save_pos(hwnd: HWND) {
    let mut r: RECT = zeroed();
    GetWindowRect(hwnd, &mut r);
    let _ = std::fs::create_dir_all(brave::data_dir());
    let _ = std::fs::write(pos_file(), format!("{} {}", r.left, r.top));
}

unsafe fn initial_pos(w: i32, h: i32) -> (i32, i32) {
    if let Some((x, y)) = std::fs::read_to_string(pos_file()).ok().and_then(|s| {
        let mut it = s.split_whitespace().map(|n| n.parse::<i32>().ok());
        Some((it.next()??, it.next()??))
    }) {
        // Solo si sigue cayendo en algun monitor.
        if !MonitorFromPoint(POINT { x: x + 20, y: y + 20 }, MONITOR_DEFAULTTONULL).is_null() {
            return (x, y);
        }
    }
    let mut wa: RECT = zeroed();
    SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut wa as *mut RECT).cast(), 0);
    (wa.right - w - 16, wa.bottom - h - 16)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let a = app();
    let (mx, my) = ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            a.paint(hdc);
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCHITTEST => {
            let mut p = POINT { x: mx, y: my };
            ScreenToClient(hwnd, &mut p);
            // El encabezado (fuera de los botones) arrastra la ventana.
            if p.y < a.px(HDR - 4) && a.hit(p.x, p.y) == Hit::None {
                HTCAPTION as isize
            } else {
                HTCLIENT as isize
            }
        }
        WM_LBUTTONDOWN => {
            a.click(mx, my);
            0
        }
        WM_MOUSEMOVE => {
            if !a.tracking {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut t);
                a.tracking = true;
            }
            let h = a.hit(mx, my);
            if h != a.hover {
                a.hover = h;
                a.invalidate();
            }
            0
        }
        WM_MOUSELEAVE_ => {
            a.tracking = false;
            if a.hover != Hit::None {
                a.hover = Hit::None;
                a.invalidate();
            }
            0
        }
        WM_MOUSEWHEEL => {
            let n = a.items.len();
            if n > MAX_ROWS {
                let d = ((wp >> 16) & 0xffff) as i16;
                a.scroll = if d > 0 { a.scroll.saturating_sub(2) } else { (a.scroll + 2).min(n - MAX_ROWS) };
                a.invalidate();
            }
            0
        }
        WM_COMMAND => {
            if lp as HWND == a.edit && ((wp >> 16) & 0xffff) as u32 == EN_CHANGE {
                SetTimer(hwnd, T_SEARCH, 250, None);
            }
            0
        }
        WM_CTLCOLOREDIT => {
            let dc = wp as HDC;
            SetTextColor(dc, C_TEXT);
            SetBkColor(dc, C_FIELD);
            a.br_field as LRESULT
        }
        WM_TIMER => {
            match wp {
                T_SEARCH => {
                    KillTimer(hwnd, T_SEARCH);
                    let mut buf = [0u16; 256];
                    let n = GetWindowTextW(a.edit, buf.as_mut_ptr(), 256);
                    let q = String::from_utf16_lossy(&buf[..n as usize]);
                    let q = q.trim();
                    a.seq += 1;
                    if q.chars().count() > 1 {
                        a.eng.call("search", json!(q), K_SEARCH | (a.seq & 0xffffff));
                    } else {
                        a.set_items(Vec::new());
                    }
                }
                T_TICK => {
                    let b = a.btn(Hit::Bar);
                    InvalidateRect(hwnd, &b, 0);
                }
                T_MSG => {
                    KillTimer(hwnd, T_MSG);
                    a.msg.clear();
                    a.invalidate();
                }
                _ => {}
            }
            0
        }
        WM_ENGINE => {
            let ev = Box::from_raw(lp as *mut Ev);
            a.on_engine(*ev);
            0
        }
        WM_EXITSIZEMOVE => {
            save_pos(hwnd);
            0
        }
        WM_DESTROY => {
            save_pos(hwnd);
            a.eng.shutdown();
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() {
    unsafe {
        std::panic::set_hook(Box::new(|i| {
            let _ = std::fs::create_dir_all(brave::data_dir());
            let _ = std::fs::write(brave::data_dir().join("panic.txt"), i.to_string());
        }));
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        let scale = GetDpiForSystem() as f32 / 96.0;
        let hinst = GetModuleHandleW(null());
        let cls = wide("ytm-float");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: cls.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);

        let px = |v: i32| (v as f32 * scale).round() as i32;
        let (w, h) = (px(W), px(HDR + BAR + SRCH));
        let (x, y) = initial_pos(w, h);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW, cls.as_ptr(), wide("YTM").as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN, x, y, w, h, null_mut(), null_mut(), hinst, null(),
        );
        let y0 = HDR + BAR;
        let edit = CreateWindowExW(
            0, wide("EDIT").as_ptr(), null(), WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
            px(34), px(y0 + 10), px(W - 70 - 38), px(16), hwnd, null_mut(), hinst, null(),
        );
        let f_text = font(px(13), 400, "Segoe UI");
        SendMessageW(edit, WM_SETFONT, f_text as usize, 0);

        let a = Box::new(App {
            hwnd,
            edit,
            eng: engine::start(hwnd as isize),
            scale,
            f_bold: font(px(13), 600, "Segoe UI"),
            f_text,
            f_small: font(px(11), 400, "Segoe UI"),
            f_icon: font(px(12), 400, "Segoe MDL2 Assets"),
            br_field: CreateSolidBrush(C_FIELD),
            title: String::new(),
            artist: String::new(),
            status: "Iniciando…".into(),
            msg: String::new(),
            paused: true,
            pos: 0.0,
            dur: 0.0,
            at: Instant::now(),
            logged: true,
            ready: false,
            items: Vec::new(),
            scroll: 0,
            sel: None,
            hover: Hit::None,
            tracking: false,
            seq: 0,
        });
        APP = Box::into_raw(a);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut m: MSG = zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            // Teclas de navegacion en el buscador: se manejan antes de que el EDIT las vea.
            if m.message == WM_KEYDOWN && m.hwnd == edit && app().key(m.wParam as u16) {
                continue;
            }
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

use std::mem::size_of;
