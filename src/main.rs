#![windows_subsystem = "windows"]
// Widget flotante de estetica oscura: ventana en capas (alfa por pixel, esquinas
// suavizadas), formas dibujadas a mano sobre un DIB y texto con GDI. Sin frameworks.
mod brave;
mod cdp;
mod engine;

use engine::{Engine, Ev, WM_ENGINE};
use serde_json::{json, Value};
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::Arc,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

use brave::wide;

// Medidas en px a 96 dpi; se escalan con `px`.
const W: i32 = 360;
const RADIUS: f32 = 16.0;
const Y_CTRL: i32 = 58;
const Y_PROG: i32 = 102;
const Y_SRCH: i32 = 128;
const H_BASE: i32 = 176;
const ROW: i32 = 40;
const MAX_ROWS: usize = 8;

// Paleta: fondo negro, tarjetas grafito, acento indigo.
const BG: u32 = rgb(0, 0, 0);
const CARD: u32 = rgb(0x14, 0x15, 0x18);
const TAB: u32 = rgb(0x1d, 0x1f, 0x23);
const HOVER: u32 = rgb(0x25, 0x28, 0x30);
const HAIRLINE: u32 = rgb(0x1c, 0x1d, 0x20);
const INK: u32 = rgb(0xf5, 0xf6, 0xf8);
const DIM: u32 = rgb(0x93, 0x98, 0xa1);
const DIM3: u32 = rgb(0x6b, 0x70, 0x79);
const INDIGO: u32 = rgb(0x63, 0x66, 0xf1);
const GREEN: u32 = rgb(0x22, 0xc5, 0x5e);
const AMBER: u32 = rgb(0xf5, 0xa5, 0x24);
const CYAN: u32 = rgb(0x22, 0xd3, 0xee);
const RED: u32 = rgb(0xf4, 0x50, 0x5e);
const PINK: u32 = rgb(0xf4, 0x72, 0xb6);

const WM_RENDER: u32 = WM_APP + 2;
const WM_MOUSELEAVE_: u32 = 0x02A3;
const CF_UNICODETEXT: u32 = 13;
const T_SEARCH: usize = 1;
const T_TICK: usize = 2;
const T_MSG: usize = 3;
const T_CARET: usize = 4;

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
    Close,
    Login,
    Shuffle,
    Prev,
    Play,
    Next,
    Repeat,
    Bar,
    Field,
    Lists,
    Row(usize),
    Radio(usize),
}

struct Item {
    id: Option<String>,
    list: Option<String>,
    title: String,
    sub: String,
}

// ---- Lienzo: DIB de 32 bits con formas antialias hechas a mano ----
struct Canvas {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
}

impl Canvas {
    unsafe fn new(w: i32, h: i32) -> Canvas {
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(null_mut(), screen);
        let mut bi: BITMAPINFO = zeroed();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h; // de arriba hacia abajo
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let old = SelectObject(dc, bmp);
        SetBkMode(dc, TRANSPARENT as _);
        Canvas { dc, bmp, old, bits: bits.cast(), w, h }
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }

    /// Rectangulo redondeado (o circulo si rad = mitad del lado) con borde suavizado.
    fn round(&mut self, l: f32, t: f32, r: f32, b: f32, rad: f32, c: u32) {
        let (w, h) = (self.w, self.h);
        let (cr, cg, cb) = ((c & 0xff) as f32, ((c >> 8) & 0xff) as f32, ((c >> 16) & 0xff) as f32);
        let (cx, cy, hw, hh) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0, (b - t) / 2.0);
        let px = self.pixels();
        for y in (t.floor() as i32).max(0)..(b.ceil() as i32).min(h) {
            for x in (l.floor() as i32).max(0)..(r.ceil() as i32).min(w) {
                let qx = ((x as f32 + 0.5 - cx).abs() - (hw - rad)).max(0.0);
                let qy = ((y as f32 + 0.5 - cy).abs() - (hh - rad)).max(0.0);
                let cov = (0.5 - ((qx * qx + qy * qy).sqrt() - rad)).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let i = (y * w + x) as usize;
                let p = px[i];
                let mix = |o: u32, n: f32| (o as f32 + (n - o as f32) * cov) as u32;
                px[i] = mix((p >> 16) & 0xff, cr) << 16 | mix((p >> 8) & 0xff, cg) << 8 | mix(p & 0xff, cb);
            }
        }
    }

    /// Recorta la ventana con esquinas redondeadas y premultiplica el alfa.
    fn finish(&mut self, rad: f32) {
        let (w, h) = (self.w, self.h);
        let (cx, cy, hw, hh) = (w as f32 / 2.0, h as f32 / 2.0, w as f32 / 2.0, h as f32 / 2.0);
        let px = self.pixels();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let qx = ((x as f32 + 0.5 - cx).abs() - (hw - rad)).max(0.0);
                let qy = ((y as f32 + 0.5 - cy).abs() - (hh - rad)).max(0.0);
                let a = (0.5 - ((qx * qx + qy * qy).sqrt() - rad)).clamp(0.0, 1.0);
                let p = px[i] & 0xffffff;
                px[i] = if a >= 1.0 {
                    p | 0xff00_0000
                } else {
                    let m = |v: u32| (v as f32 * a) as u32;
                    ((a * 255.0) as u32) << 24 | m((p >> 16) & 0xff) << 16 | m((p >> 8) & 0xff) << 8 | m(p & 0xff)
                };
            }
        }
    }

    unsafe fn text(&self, f: HFONT, s: &str, mut r: RECT, c: u32, fmt: DRAW_TEXT_FORMAT) {
        if s.is_empty() {
            return; // DrawTextW con puntero de Vec vacio revienta
        }
        let w: Vec<u16> = s.encode_utf16().collect();
        let old = SelectObject(self.dc, f);
        SetTextColor(self.dc, c);
        DrawTextW(self.dc, w.as_ptr(), w.len() as i32, &mut r, fmt | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
        SelectObject(self.dc, old);
    }

    unsafe fn text_width(&self, f: HFONT, s: &str) -> i32 {
        let w: Vec<u16> = s.encode_utf16().collect();
        let old = SelectObject(self.dc, f);
        let mut sz: SIZE = zeroed();
        GetTextExtentPoint32W(self.dc, w.as_ptr(), w.len() as i32, &mut sz);
        SelectObject(self.dc, old);
        sz.cx
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

struct Fonts {
    title: HFONT,
    text: HFONT,
    small: HFONT,
    icon: HFONT,
    icon_big: HFONT,
}

struct App {
    hwnd: HWND,
    eng: Arc<Engine>,
    scale: f32,
    f: Fonts,
    canvas: Option<Canvas>,
    dirty: bool,
    // reproduccion
    title: String,
    artist: String,
    status: String,
    msg: String,
    paused: bool,
    ad: bool,
    pos: f64,
    dur: f64,
    at: Instant,
    logged: bool,
    ready: bool,
    repeat: String,
    // busqueda
    query: String,
    active: bool,
    caret_on: bool,
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

fn mmss(t: f64) -> String {
    let s = t.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

impl App {
    fn px(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }
    fn pf(&self, v: f32) -> f32 {
        v * self.scale
    }
    fn r(&self, l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT { left: self.px(l), top: self.px(t), right: self.px(r), bottom: self.px(b) }
    }
    fn rows_visible(&self) -> usize {
        self.items.len().min(MAX_ROWS)
    }
    fn height(&self) -> i32 {
        let n = self.rows_visible() as i32;
        self.px(if n > 0 { H_BASE + n * ROW + 8 + 12 } else { H_BASE })
    }
    fn ctrl_center(&self, k: i32) -> i32 {
        W / 2 + k * 46
    }

    // Zonas clickeables (en px de 96 dpi).
    fn zone(&self, h: Hit) -> RECT {
        match h {
            Hit::Close => self.r(W - 40, 12, W - 12, 40),
            Hit::Login => self.r(W - 70, 12, W - 42, 40),
            Hit::Shuffle => self.ctrl(-2),
            Hit::Prev => self.ctrl(-1),
            Hit::Play => self.ctrl(0),
            Hit::Next => self.ctrl(1),
            Hit::Repeat => self.ctrl(2),
            Hit::Bar => self.r(54, Y_PROG, W - 54, Y_PROG + 20),
            Hit::Field => self.r(12, Y_SRCH, W - 86, Y_SRCH + 36),
            Hit::Lists => self.r(W - 80, Y_SRCH, W - 12, Y_SRCH + 36),
            _ => RECT { left: 0, top: 0, right: 0, bottom: 0 },
        }
    }
    fn ctrl(&self, k: i32) -> RECT {
        let c = self.ctrl_center(k);
        self.r(c - 18, Y_CTRL + 2, c + 18, Y_CTRL + 38)
    }
    fn row_y(&self, vis: usize) -> i32 {
        H_BASE + 4 + vis as i32 * ROW
    }
    fn row_rect(&self, vis: usize) -> RECT {
        let y = self.row_y(vis);
        self.r(16, y, W - 16, y + ROW)
    }
    fn radio_rect(&self, vis: usize) -> RECT {
        let y = self.row_y(vis);
        self.r(W - 76, y + 9, W - 24, y + ROW - 9)
    }

    fn hit(&self, x: i32, y: i32) -> Hit {
        let inside = |r: RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        let mut hs = vec![Hit::Close, Hit::Shuffle, Hit::Prev, Hit::Play, Hit::Next, Hit::Repeat, Hit::Field, Hit::Lists];
        if self.ready && !self.logged {
            hs.push(Hit::Login);
        }
        if self.dur > 0.0 {
            hs.push(Hit::Bar);
        }
        if let Some(h) = hs.into_iter().find(|&h| inside(self.zone(h))) {
            return h;
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

    /// Pide un redibujado; se agrupan en un solo WM_RENDER.
    fn invalidate(&mut self) {
        if !self.dirty {
            self.dirty = true;
            unsafe { PostMessageW(self.hwnd, WM_RENDER, 0, 0) };
        }
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
        self.invalidate();
    }

    fn clear_search(&mut self) {
        self.seq += 1; // descarta respuestas en vuelo
        self.query.clear();
        unsafe { KillTimer(self.hwnd, T_SEARCH) };
        self.set_items(Vec::new());
    }

    fn query_changed(&mut self) {
        self.caret_on = true;
        unsafe { SetTimer(self.hwnd, T_SEARCH, 250, None) };
        self.invalidate();
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

    fn toggle(&mut self) {
        // Respuesta instantanea en la UI; el evento de la pagina confirma despues.
        self.pos = self.cur_pos();
        self.at = Instant::now();
        self.paused = !self.paused;
        self.update_tick();
        self.invalidate();
        self.cmd("toggle");
    }

    fn click(&mut self, x: i32, y: i32) {
        match self.hit(x, y) {
            Hit::Close => unsafe {
                DestroyWindow(self.hwnd);
            },
            Hit::Prev => self.cmd("prev"),
            Hit::Next => self.cmd("next"),
            Hit::Play => self.toggle(),
            Hit::Shuffle => {
                self.cmd("shuffle");
                self.flash("Cola mezclada");
            }
            Hit::Repeat => {
                self.repeat = match self.repeat.as_str() {
                    "NONE" => "ALL",
                    "ALL" => "ONE",
                    _ => "NONE",
                }
                .into();
                self.invalidate();
                self.cmd("repeat");
            }
            Hit::Login => {
                let eng = self.eng.clone();
                std::thread::spawn(move || eng.login());
            }
            Hit::Lists => {
                self.seq += 1;
                self.query.clear();
                self.eng.call("playlists", Value::Null, K_LISTS | (self.seq & 0xffffff));
                self.flash("Cargando listas…");
            }
            Hit::Bar => {
                let b = self.zone(Hit::Bar);
                let t = ((x - b.left) as f64 / (b.right - b.left) as f64).clamp(0.0, 1.0) * self.dur;
                self.pos = t;
                self.at = Instant::now();
                self.invalidate();
                self.eng.call("seek", json!(t), K_OTHER);
            }
            Hit::Field => unsafe {
                SetForegroundWindow(self.hwnd);
            },
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
                self.ad = s["ad"].as_bool().unwrap_or(false);
                self.pos = s["pos"].as_f64().unwrap_or(0.0);
                self.dur = s["dur"].as_f64().unwrap_or(0.0);
                self.logged = s["logged"].as_bool().unwrap_or(false);
                self.repeat = s["repeat"].as_str().unwrap_or("NONE").to_string();
                self.at = Instant::now();
                self.update_tick();
            }
            Ev::Reply(tag, r) => {
                let ok = r["ok"].as_bool().unwrap_or(false);
                let kind = tag & K_MASK;
                let current = (tag & 0xffffff) == (self.seq & 0xffffff);
                if !ok {
                    if kind == K_OTHER || current {
                        self.flash(r["error"].as_str().unwrap_or("error"));
                    }
                    return;
                }
                if (kind == K_SEARCH || kind == K_LISTS) && current {
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
                } else if kind == K_OTHER && r["data"]["spa"] == false {
                    self.flash("Recargando YouTube Music…");
                }
            }
        }
        self.invalidate();
    }

    fn key_down(&mut self, vk: u16) -> bool {
        let n = self.items.len();
        let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
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
            }
            VK_RETURN => {
                if let Some(s) = self.sel {
                    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
                    self.play(s, shift); // Shift+Enter = radio
                }
            }
            VK_ESCAPE => self.clear_search(),
            VK_BACK => {
                if ctrl {
                    let t = self.query.trim_end().len();
                    let cut = self.query[..t].rfind(' ').map(|i| i + 1).unwrap_or(0);
                    self.query.truncate(cut);
                } else {
                    self.query.pop();
                }
                self.query_changed();
            }
            0x56 if ctrl => {
                // Ctrl+V
                let clip = unsafe { clipboard() };
                self.query.push_str(clip.lines().next().unwrap_or("").trim());
                self.query_changed();
            }
            _ => return false,
        }
        true
    }

    fn char_in(&mut self, c: u16) {
        if c < 0x20 || c == 0x7f {
            return;
        }
        if c == 0x20 && self.query.is_empty() {
            self.toggle(); // espacio con el buscador vacio = play/pausa
            return;
        }
        if let Some(ch) = char::from_u32(c as u32) {
            self.query.push(ch);
            self.query_changed();
        }
    }

    unsafe fn render(&mut self) {
        self.dirty = false;
        let (w, h) = (self.px(W), self.height());
        if self.canvas.as_ref().map_or(true, |c| c.w != w || c.h != h) {
            self.canvas = None;
            self.canvas = Some(Canvas::new(w, h));
        }
        let mut cv = self.canvas.take().unwrap();
        cv.pixels().fill(BG);
        let s = self.scale;
        let wf = w as f32;

        // Borde finito (hairline).
        cv.round(0.0, 0.0, wf, h as f32, RADIUS * s, HAIRLINE);
        cv.round(1.0, 1.0, wf - 1.0, h as f32 - 1.0, RADIUS * s - 1.0, BG);

        // Punto de estado.
        let dot = if !self.ready {
            CYAN
        } else if !self.msg.is_empty() && self.msg.starts_with("Error") {
            RED
        } else if self.ad {
            AMBER
        } else if self.paused || self.title.is_empty() {
            DIM3
        } else {
            GREEN
        };
        let (dx, dy) = (self.pf(20.0), self.pf(22.0));
        cv.round(dx - self.pf(4.0), dy - self.pf(4.0), dx + self.pf(4.0), dy + self.pf(4.0), self.pf(4.0), dot);

        // Botones redondos del encabezado (hover).
        for hb in [Hit::Close, Hit::Login] {
            if self.hover == hb {
                let z = self.zone(hb);
                cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(14.0), TAB);
            }
        }

        // Controles: hover en circulo; play como boton blanco.
        for (k, hb) in [(-2, Hit::Shuffle), (-1, Hit::Prev), (1, Hit::Next), (2, Hit::Repeat)] {
            if self.hover == hb {
                let c = self.pf(self.ctrl_center(k) as f32);
                let cy = self.pf((Y_CTRL + 20) as f32);
                cv.round(c - self.pf(17.0), cy - self.pf(17.0), c + self.pf(17.0), cy + self.pf(17.0), self.pf(17.0), TAB);
            }
        }
        let (pc, pcy) = (self.pf(self.ctrl_center(0) as f32), self.pf((Y_CTRL + 20) as f32));
        let pr = self.pf(if self.hover == Hit::Play { 18.0 } else { 17.0 });
        cv.round(pc - pr, pcy - pr, pc + pr, pcy + pr, pr, INK);

        // Progreso.
        let (bl, br, by) = (self.pf(54.0), self.pf((W - 54) as f32), self.pf((Y_PROG + 10) as f32));
        let bh = self.pf(if self.hover == Hit::Bar { 3.0 } else { 2.0 });
        cv.round(bl, by - bh, br, by + bh, bh, HOVER);
        if self.dur > 0.0 {
            let f = (self.cur_pos() / self.dur).clamp(0.0, 1.0) as f32;
            let x = bl + (br - bl) * f;
            if x - bl > 1.0 {
                cv.round(bl, by - bh, x, by + bh, bh, if self.ad { AMBER } else { INDIGO });
            }
            if self.hover == Hit::Bar {
                cv.round(x - self.pf(6.0), by - self.pf(6.0), x + self.pf(6.0), by + self.pf(6.0), self.pf(6.0), INK);
            }
        }

        // Buscador y boton Listas como pastillas.
        let fz = self.zone(Hit::Field);
        cv.round(fz.left as f32, fz.top as f32, fz.right as f32, fz.bottom as f32, self.pf(12.0),
            if self.active || self.hover == Hit::Field { TAB } else { CARD });
        let lz = self.zone(Hit::Lists);
        cv.round(lz.left as f32, lz.top as f32, lz.right as f32, lz.bottom as f32, self.pf(12.0),
            if self.hover == Hit::Lists { HOVER } else { TAB });

        // Resultados en una tarjeta.
        let n = self.rows_visible();
        if n > 0 {
            let top = self.pf(H_BASE as f32);
            let bottom = self.pf((self.row_y(n) + 4) as f32);
            cv.round(self.pf(12.0), top, wf - self.pf(12.0), bottom, self.pf(13.0), CARD);
            for vis in 0..n {
                let i = self.scroll + vis;
                let hot = matches!(self.hover, Hit::Row(j) | Hit::Radio(j) if j == i);
                if hot || self.sel == Some(i) {
                    let rr = self.row_rect(vis);
                    cv.round(rr.left as f32, rr.top as f32, rr.right as f32, rr.bottom as f32, self.pf(9.0), TAB);
                    if self.items[i].id.is_some() {
                        let rb = self.radio_rect(vis);
                        cv.round(rb.left as f32, rb.top as f32, rb.right as f32, rb.bottom as f32, self.pf(11.0),
                            if self.hover == Hit::Radio(i) { INDIGO } else { HOVER });
                    }
                }
            }
            if self.items.len() > MAX_ROWS {
                let span = (n as i32 * ROW) as f32;
                let th = (span * MAX_ROWS as f32 / self.items.len() as f32).max(14.0);
                let ty = self.row_y(0) as f32 + (span - th) * self.scroll as f32 / (self.items.len() - MAX_ROWS) as f32;
                cv.round(wf - self.pf(16.0), self.pf(ty), wf - self.pf(13.0), self.pf(ty + th), self.pf(1.5), HOVER);
            }
        }

        GdiFlush();

        // ---- Texto ----
        let tr = if self.ready && !self.logged { W - 74 } else { W - 44 };
        let (line1, line2) = if !self.ready {
            (self.status.as_str(), "")
        } else if self.title.is_empty() {
            ("Nada sonando", if self.logged { "Buscá una canción o abrí tus listas" } else { "Sin sesión · tocá el ícono de usuario" })
        } else {
            (self.title.as_str(), self.artist.as_str())
        };
        cv.text(self.f.title, line1, self.r(32, 10, tr, 32), INK, DT_LEFT);
        if self.msg.is_empty() {
            cv.text(self.f.small, line2, self.r(32, 31, tr, 48), DIM, DT_LEFT);
        } else {
            cv.text(self.f.small, &self.msg, self.r(32, 31, tr, 48), PINK, DT_LEFT);
        }
        cv.text(self.f.icon, "\u{E8BB}", self.zone(Hit::Close), if self.hover == Hit::Close { INK } else { DIM }, DT_CENTER);
        if self.ready && !self.logged {
            cv.text(self.f.icon, "\u{E77B}", self.zone(Hit::Login), if self.hover == Hit::Login { INK } else { AMBER }, DT_CENTER);
        }

        let ic = |hb: Hit| if self.hover == hb { INK } else { DIM };
        cv.text(self.f.icon, "\u{E8B1}", self.zone(Hit::Shuffle), ic(Hit::Shuffle), DT_CENTER);
        cv.text(self.f.icon_big, "\u{E892}", self.zone(Hit::Prev), ic(Hit::Prev), DT_CENTER);
        cv.text(self.f.icon_big, if self.paused { "\u{E768}" } else { "\u{E769}" }, self.zone(Hit::Play), BG, DT_CENTER);
        cv.text(self.f.icon_big, "\u{E893}", self.zone(Hit::Next), ic(Hit::Next), DT_CENTER);
        let (rg, rc) = match self.repeat.as_str() {
            "ONE" => ("\u{E8ED}", INDIGO),
            "ALL" => ("\u{E8EE}", INDIGO),
            _ => ("\u{E8EE}", ic(Hit::Repeat)),
        };
        cv.text(self.f.icon, rg, self.zone(Hit::Repeat), rc, DT_CENTER);

        if self.dur > 0.0 {
            cv.text(self.f.small, &mmss(self.cur_pos()), self.r(10, Y_PROG, 48, Y_PROG + 20), DIM, DT_RIGHT);
            cv.text(self.f.small, &mmss(self.dur), self.r(W - 48, Y_PROG, W - 10, Y_PROG + 20), DIM, DT_LEFT);
        }

        cv.text(self.f.icon, "\u{E721}", self.r(22, Y_SRCH, 40, Y_SRCH + 36), DIM3, DT_CENTER);
        let field = self.r(44, Y_SRCH, W - 96, Y_SRCH + 36);
        if self.query.is_empty() {
            cv.text(self.f.text, "Buscar canción…", field, DIM3, DT_LEFT);
        } else {
            // Si no entra, se ve el final (donde se escribe).
            let tw = cv.text_width(self.f.text, &self.query);
            let fw = field.right - field.left;
            let mut f2 = field;
            if tw > fw {
                f2.left = field.right - tw;
            }
            let wq: Vec<u16> = self.query.encode_utf16().collect();
            let old = SelectObject(cv.dc, self.f.text);
            SetTextColor(cv.dc, INK);
            let rgn = CreateRectRgn(field.left, field.top, field.right, field.bottom);
            SelectClipRgn(cv.dc, rgn);
            DrawTextW(cv.dc, wq.as_ptr(), wq.len() as i32, &mut f2, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            SelectClipRgn(cv.dc, null_mut());
            DeleteObject(rgn);
            SelectObject(cv.dc, old);
        }
        if self.active && self.caret_on {
            let tw = if self.query.is_empty() { 0 } else { cv.text_width(self.f.text, &self.query) };
            let x = field.left + tw.min(field.right - field.left);
            GdiFlush();
            cv.round(x as f32, self.pf((Y_SRCH + 10) as f32), x as f32 + self.pf(1.5).max(1.0), self.pf((Y_SRCH + 26) as f32), 0.5, INDIGO);
        }
        cv.text(self.f.small, "Listas", self.zone(Hit::Lists), INK, DT_CENTER);

        for vis in 0..n {
            let i = self.scroll + vis;
            let it = &self.items[i];
            let y = self.row_y(vis);
            let hot = matches!(self.hover, Hit::Row(j) | Hit::Radio(j) if j == i) || self.sel == Some(i);
            let right = if it.id.is_some() && hot { W - 82 } else { W - 26 };
            cv.text(self.f.text, &it.title, self.r(26, y + 4, right, y + 22), INK, DT_LEFT);
            cv.text(self.f.small, &it.sub, self.r(26, y + 21, right, y + 37), DIM, DT_LEFT);
            if it.id.is_some() && hot {
                cv.text(self.f.small, "Radio", self.radio_rect(vis), INK, DT_CENTER);
            }
        }

        GdiFlush();
        cv.finish(RADIUS * s);

        // Si al crecer se sale del monitor, se corre hacia arriba lo justo.
        let mut wr: RECT = zeroed();
        GetWindowRect(self.hwnd, &mut wr);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
        let top = wr.top.min(mi.rcWork.bottom - h).max(mi.rcWork.top);
        let dst = POINT { x: wr.left, y: top };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(self.hwnd, null_mut(), &dst, &size, cv.dc, &src, 0, &blend, ULW_ALPHA);
        self.canvas = Some(cv);
    }
}

unsafe fn clipboard() -> String {
    if OpenClipboard(null_mut()) == 0 {
        return String::new();
    }
    let mut s = String::new();
    let h = GetClipboardData(CF_UNICODETEXT);
    if !h.is_null() {
        let p = GlobalLock(h as _) as *const u16;
        if !p.is_null() {
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            GlobalUnlock(h as _);
        }
    }
    CloseClipboard();
    s
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
        WM_RENDER => {
            a.render();
            0
        }
        WM_NCHITTEST => {
            let mut p = POINT { x: mx, y: my };
            ScreenToClient(hwnd, &mut p);
            // El encabezado (fuera de los botones) arrastra la ventana.
            if p.y < a.px(Y_CTRL) && a.hit(p.x, p.y) == Hit::None {
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
                SetCursor(LoadCursorW(null_mut(), if h == Hit::Field { IDC_IBEAM } else if h == Hit::None { IDC_ARROW } else { IDC_HAND }));
                a.invalidate();
            }
            0
        }
        WM_SETCURSOR => {
            if (lp & 0xffff) as u32 == HTCLIENT && a.hover != Hit::None {
                SetCursor(LoadCursorW(null_mut(), if a.hover == Hit::Field { IDC_IBEAM } else { IDC_HAND }));
                1
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
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
        WM_ACTIVATE => {
            a.active = (wp & 0xffff) != 0;
            a.caret_on = true;
            if a.active {
                SetTimer(hwnd, T_CARET, 530, None);
            } else {
                KillTimer(hwnd, T_CARET);
            }
            a.invalidate();
            0
        }
        WM_KEYDOWN => {
            if a.key_down(wp as u16) {
                0
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_CHAR => {
            a.char_in(wp as u16);
            0
        }
        WM_TIMER => {
            match wp {
                T_SEARCH => {
                    KillTimer(hwnd, T_SEARCH);
                    let q = a.query.trim().to_string();
                    a.seq += 1;
                    if q.chars().count() > 1 {
                        a.eng.call("search", json!(q), K_SEARCH | (a.seq & 0xffffff));
                    } else {
                        a.set_items(Vec::new());
                    }
                }
                T_TICK => a.invalidate(),
                T_MSG => {
                    KillTimer(hwnd, T_MSG);
                    a.msg.clear();
                    a.invalidate();
                }
                T_CARET => {
                    a.caret_on = !a.caret_on;
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
        let (w, h) = (px(W), px(H_BASE));
        let (x, y) = initial_pos(w, h);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, cls.as_ptr(), wide("YTM").as_ptr(),
            WS_POPUP, x, y, w, h, null_mut(), null_mut(), hinst, null(),
        );

        let a = Box::new(App {
            hwnd,
            eng: engine::start(hwnd as isize),
            scale,
            f: Fonts {
                title: font(px(14), 600, "Segoe UI"),
                text: font(px(13), 400, "Segoe UI"),
                small: font(px(11), 400, "Segoe UI"),
                icon: font(px(13), 400, "Segoe MDL2 Assets"),
                icon_big: font(px(15), 400, "Segoe MDL2 Assets"),
            },
            canvas: None,
            dirty: false,
            title: String::new(),
            artist: String::new(),
            status: "Iniciando…".into(),
            msg: String::new(),
            paused: true,
            ad: false,
            pos: 0.0,
            dur: 0.0,
            at: Instant::now(),
            logged: true,
            ready: false,
            repeat: "NONE".into(),
            query: String::new(),
            active: false,
            caret_on: true,
            items: Vec::new(),
            scroll: 0,
            sel: None,
            hover: Hit::None,
            tracking: false,
            seq: 0,
        });
        APP = Box::into_raw(a);
        app().render(); // una ventana en capas no se ve hasta el primer UpdateLayeredWindow
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut m: MSG = zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}
