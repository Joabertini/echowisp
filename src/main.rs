#![windows_subsystem = "windows"]
// Widget flotante de estetica oscura: ventana en capas (alfa por pixel, esquinas
// suavizadas), formas dibujadas a mano sobre un DIB y texto con GDI. Sin frameworks.
mod brave;
mod bridge;
mod cdp;
mod engine;
mod panel;
mod theme;

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
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*, Threading::CreateMutexW},
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

use brave::wide;
use panel::{PHit, Panel};
use theme::th;

// Medidas en px a 96 dpi; se escalan con `px`. El ancho es el de los cinco controles + margen.
const W: i32 = 240;
const RADIUS: f32 = 16.0;
const H_MINI: i32 = 50; // tarjeta colapsada: solo cancion y artista
const Y_CTRL: i32 = 50;
const Y_PROG: i32 = 92;
const Y_VOL: i32 = 114;
const Y_SRCH: i32 = 140;
const Y_TABS: i32 = 182;
const H_BASE: i32 = 230;
const Y_BR: i32 = 226; // seccion del puente a Discord (solo con ytm-bridge.exe instalado)
const ROW: i32 = 40;
const MAX_ROWS: usize = 8;

// Colores de estado (fijos). Los de la interfaz (fondo, tarjetas, texto, acento) salen del tema: theme.rs.
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
const T_MIX: usize = 5; // refresco de la lista de apps mientras esta abierta

const HK_TOGGLE: i32 = 1;
const HK_NEXT: i32 = 2;
const HK_PREV: i32 = 3;
const HK_SHOW: i32 = 4;
const HK_SEARCH: i32 = 5;
const HK_MEDIA_PLAY: i32 = 6;
const HK_MEDIA_NEXT: i32 = 7;
const HK_MEDIA_PREV: i32 = 8;
const HK_MINI: i32 = 9;
const HOTKEYS: &[(i32, HOT_KEY_MODIFIERS, VIRTUAL_KEY)] = &[
    (HK_TOGGLE, MOD_CONTROL | MOD_ALT, VK_SPACE),
    (HK_NEXT, MOD_CONTROL | MOD_ALT, VK_RIGHT),
    (HK_PREV, MOD_CONTROL | MOD_ALT, VK_LEFT),
    (HK_SHOW, MOD_CONTROL | MOD_ALT, 0x48), // H (Ctrl+Alt+M lo tiene otra app)
    (HK_MINI, MOD_CONTROL | MOD_ALT, 0x4E), // N
    (HK_SEARCH, MOD_CONTROL | MOD_ALT, 0x42), // B
    (HK_MEDIA_PLAY, 0, VK_MEDIA_PLAY_PAUSE),
    (HK_MEDIA_NEXT, 0, VK_MEDIA_NEXT_TRACK),
    (HK_MEDIA_PREV, 0, VK_MEDIA_PREV_TRACK),
];

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
    Vol,
    VolBar,
    Field,
    Lists,
    Again,
    Quick,
    Gear,
    BrChannel,
    BrGo,
    BrApps,
    DscRow,
    Row(usize),
    Radio(usize),
}

struct Item {
    id: Option<String>,
    list: Option<String>,
    title: String,
    sub: String,
    pick: Option<String>, // valor elegido en los selectores del puente
}

/// App con sonido del mezclador del puente (ver bridge/src/mixer.rs).
struct MixApp {
    pid: u64,
    name: String,
    on: bool, // mandada al cable (a Discord)
    vol: f64, // 0-100
}

struct BrGuild {
    id: String,
    name: String,
    channels: Vec<(String, String)>,
}

impl Item {
    fn from_json(x: &Value) -> Item {
        let s = |k: &str| x[k].as_str().map(String::from);
        Item { id: s("id"), list: s("list"), title: s("title").unwrap_or_default(), sub: s("sub").unwrap_or_default(), pick: s("pick") }
    }
}

/// "CABLE Output (VB-Audio Virtual Cable)" → "CABLE Output".
fn short_dev(d: &str) -> &str {
    d.split(" (").next().unwrap_or(d)
}

// ---- Lienzo: DIB de 32 bits con formas antialias hechas a mano ----
struct Canvas {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
    opaque: Vec<(f32, f32, f32)>, // discos que no se vuelven transparentes con la translucidez (boton de play)
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
        Canvas { dc, bmp, old, bits: bits.cast(), w, h, opaque: Vec::new() }
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }

    /// Pinta todo con el fondo del tema (el DIB guarda 0x00RRGGBB; los colores del tema son COLORREF).
    fn fill_bg(&mut self) {
        self.opaque.clear();
        self.pixels().fill(theme::px(th().bg));
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

    /// Circulo relleno de radio `r` centrado en (x, y).
    fn dot(&mut self, x: f32, y: f32, r: f32, c: u32) {
        self.round(x - r, y - r, x + r, y + r, r, c);
    }

    /// Recorta la ventana con esquinas redondeadas y premultiplica el alfa. La card deja pasar un poco de
    /// lo de atras (theme::OPACITY); el boton de play queda opaco para que no pierda contraste.
    fn finish(&mut self, rad: f32) {
        let (w, h) = (self.w, self.h);
        let opaque = std::mem::take(&mut self.opaque);
        let (cx, cy, hw, hh) = (w as f32 / 2.0, h as f32 / 2.0, w as f32 / 2.0, h as f32 / 2.0);
        let px = self.pixels();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let qx = ((x as f32 + 0.5 - cx).abs() - (hw - rad)).max(0.0);
                let qy = ((y as f32 + 0.5 - cy).abs() - (hh - rad)).max(0.0);
                let mut a = (0.5 - ((qx * qx + qy * qy).sqrt() - rad)).clamp(0.0, 1.0);
                let p = px[i] & 0xffffff;
                if a > 0.0 && !opaque.iter().any(|&(ox, oy, r)| (x as f32 + 0.5 - ox).powi(2) + (y as f32 + 0.5 - oy).powi(2) <= r * r) {
                    a *= theme::OPACITY;
                }
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

    /// Texto centrado que puede partirse en dos lineas.
    unsafe fn text_wrap(&self, f: HFONT, s: &str, r: RECT, c: u32) {
        let w: Vec<u16> = s.encode_utf16().collect();
        let old = SelectObject(self.dc, f);
        SetTextColor(self.dc, c);
        let fmt = DT_CENTER | DT_WORDBREAK | DT_NOPREFIX;
        let mut m = r;
        DrawTextW(self.dc, w.as_ptr(), w.len() as i32, &mut m, fmt | DT_CALCRECT);
        let mut out = r;
        out.top = r.top + ((r.bottom - r.top) - (m.bottom - m.top)) / 2;
        DrawTextW(self.dc, w.as_ptr(), w.len() as i32, &mut out, fmt);
        SelectObject(self.dc, old);
    }
}

/// Ancho de un texto sin lienzo (para dimensionar la tarjeta colapsada).
unsafe fn measure(f: HFONT, s: &str) -> i32 {
    if s.is_empty() {
        return 0;
    }
    let dc = CreateCompatibleDC(null_mut());
    let w: Vec<u16> = s.encode_utf16().collect();
    let old = SelectObject(dc, f);
    let mut sz: SIZE = zeroed();
    GetTextExtentPoint32W(dc, w.as_ptr(), w.len() as i32, &mut sz);
    SelectObject(dc, old);
    DeleteDC(dc);
    sz.cx
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
    vol: f64,
    muted: bool,
    vol_at: Instant, // ultimo cambio local: el estado de la pagina no lo pisa por un rato
    vol_sent: i32,
    dragging: bool,
    mini: bool,
    // busqueda
    query: String,
    active: bool,
    caret_on: bool,
    items: Vec<Item>,
    shown: Hit, // pestaña cuyos resultados estan a la vista
    scroll: usize,
    sel: Option<usize>,
    hover: Hit,
    tracking: bool,
    seq: u32,
    // puente a Discord (None si ytm-bridge.exe no esta)
    br: Option<bridge::Bridge>,
    mix_apps: Vec<MixApp>,
    mix_cable: bool,
    mix_sel: Option<u64>, // app con la barra de volumen desplegada
    mix_drag: Option<usize>,
    br_state: String,
    br_err: String,
    br_has_token: bool,
    br_level: f32,
    br_devices: Vec<String>,
    br_guilds: Vec<BrGuild>,
    br_device: String,
    br_guild: Option<String>,
    br_channel: Option<String>,
    // Discord: ventana real aparte (opcional, modulo del instalador)
    dsc_avail: bool, // discord.module junto al exe
    dsc_win: Option<brave::Proc>,
    // card de costado (panel.rs): apps a Discord o configuracion
    panel: Panel,
    panel_hwnd: HWND,
    panel_canvas: Option<Canvas>,
    panel_t: f32,      // 0 escondida .. 1 afuera (antes del easing)
    panel_target: f32,
    panel_last: Instant,
    panel_sx: i32, // columna del contenido que asoma en el borde de la ventana
    panel_hover: PHit,
    panel_tracking: bool,
    panel_capture: Option<usize>, // atajo esperando combinacion nueva
    browsers: Vec<(String, std::path::PathBuf)>,
    browser_cur: std::path::PathBuf,
    hotkeys: Vec<(i32, HOT_KEY_MODIFIERS, u32)>,
    cfg_all_dev: bool, // configuracion: mostrar todas las entradas de audio
    blur_main: theme::BlurCache,
    blur_panel: theme::BlurCache,
    panel_edit: Option<(u8, String, u32)>, // (0 fondo/tinte, 1 acento; texto tecleado; color original)
    sec_open: [bool; 6],
    show_br: bool,     // filas del puente y de Discord a la vista (card.json)
    show_dsc: bool,
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
        if self.mini {
            return self.px(H_MINI);
        }
        let n = self.rows_visible() as i32;
        let b = self.base();
        self.px(if n > 0 { b + n * ROW + 8 + 12 } else { b })
    }
    /// Colapsada, la tarjeta mide lo que el nombre de la cancion y el artista.
    fn width(&self) -> i32 {
        if !self.mini {
            return self.px(W);
        }
        let (l1, l2) = self.lines();
        let l2 = if self.msg.is_empty() { l2 } else { self.msg.as_str() };
        let tw = unsafe { measure(self.f.title, l1).max(measure(self.f.small, l2)) };
        (tw + self.px(56)).clamp(self.px(110), self.px(W))
    }
    fn lines(&self) -> (&str, &str) {
        if !self.ready {
            (self.status.as_str(), "")
        } else if self.title.is_empty() {
            ("Nada sonando", if self.logged { "Buscá o abrí tus listas" } else { "Sin sesión · tocá el ícono" })
        } else {
            (self.title.as_str(), self.artist.as_str())
        }
    }
    /// Caja de cancion y artista: doble clic colapsa o expande.
    fn in_title_box(&self, x: i32, y: i32) -> bool {
        self.mini || (x >= self.px(40) && x < self.px(W - 40) && y < self.px(Y_CTRL))
    }
    fn ctrl_center(&self, k: i32) -> i32 {
        W / 2 + k * 46
    }

    // Zonas clickeables (en px de 96 dpi).
    fn zone(&self, h: Hit) -> RECT {
        match h {
            Hit::Close => self.r(W - 36, 4, W - 8, 32),
            Hit::Login => self.r(8, 4, 36, 32),
            Hit::Shuffle => self.ctrl(-2),
            Hit::Prev => self.ctrl(-1),
            Hit::Play => self.ctrl(0),
            Hit::Next => self.ctrl(1),
            Hit::Repeat => self.ctrl(2),
            Hit::Bar => self.r(54, Y_PROG, W - 54, Y_PROG + 20),
            Hit::Vol => self.r(20, Y_VOL, 46, Y_VOL + 20),
            Hit::VolBar => self.r(54, Y_VOL, W - 54, Y_VOL + 20),
            Hit::Field => self.r(12, Y_SRCH, W - 12, Y_SRCH + 36),
            Hit::Lists => self.tab(0),
            Hit::Again => self.tab(1),
            Hit::Quick => self.tab(2),
            Hit::Gear => self.r(8, 4, 36, 32),
            Hit::BrChannel => self.r(12, Y_BR, W - 78, Y_BR + 32),
            Hit::BrGo => self.r(W - 76, Y_BR + 2, W - 46, Y_BR + 30),
            Hit::BrApps => self.r(W - 44, Y_BR + 2, W - 14, Y_BR + 30),
            Hit::DscRow => {
                let t = self.dsc_top();
                self.r(12, t, W - 12, t + 32)
            }
            _ => RECT { left: 0, top: 0, right: 0, bottom: 0 },
        }
    }
    fn tab(&self, k: i32) -> RECT {
        let w = (W - 24 - 2 * 6) / 3;
        let l = 12 + k * (w + 6);
        self.r(l, Y_TABS, l + w, Y_TABS + 36)
    }
    fn ctrl(&self, k: i32) -> RECT {
        let c = self.ctrl_center(k);
        self.r(c - 18, Y_CTRL + 2, c + 18, Y_CTRL + 38)
    }
    fn row_y(&self, vis: usize) -> i32 {
        self.base() + 4 + vis as i32 * ROW
    }
    /// Donde empiezan los resultados: debajo de las pestañas o de la seccion del puente.
    fn base(&self) -> i32 {
        if !self.dsc_shown() {
            return self.dsc_top();
        }
        self.dsc_top() + 32 + 8
    }
    fn br_h(&self) -> i32 {
        if self.br_shown() { 32 + 8 } else { 0 }
    }
    /// Filas opcionales de la card: solo si el modulo esta instalado y el usuario las quiere a la vista.
    fn br_shown(&self) -> bool {
        self.br.is_some() && self.show_br
    }
    fn dsc_shown(&self) -> bool {
        self.dsc_avail && self.show_dsc
    }
    fn dsc_top(&self) -> i32 {
        Y_BR + self.br_h()
    }
    fn br_busy(&self) -> bool {
        matches!(self.br_state.as_str(), "transmitiendo" | "entrando")
    }
    fn br_guild_ref(&self) -> Option<&BrGuild> {
        let g = self.br_guild.as_deref()?;
        self.br_guilds.iter().find(|x| x.id == g)
    }
    fn br_channel_name(&self) -> Option<&str> {
        let c = self.br_channel.as_deref()?;
        self.br_guild_ref()?.channels.iter().find(|x| x.0 == c).map(|x| x.1.as_str())
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
        if self.mini {
            return Hit::None;
        }
        let inside = |r: RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        let mut hs = vec![Hit::Close, Hit::Shuffle, Hit::Prev, Hit::Play, Hit::Next, Hit::Repeat,
            Hit::Vol, Hit::VolBar, Hit::Field, Hit::Lists, Hit::Again, Hit::Quick];
        hs.push(if self.ready && !self.logged { Hit::Login } else { Hit::Gear });
        if self.dur > 0.0 {
            hs.push(Hit::Bar);
        }
        if self.br_shown() {
            hs.extend([Hit::BrGo, Hit::BrApps, Hit::BrChannel]);
        }
        if self.dsc_shown() {
            hs.push(Hit::DscRow);
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
        self.shown = Hit::None;
        unsafe { KillTimer(self.hwnd, T_SEARCH) };
        self.set_items(Vec::new());
    }

    /// Listas / Escuchar otra vez / Seleccion rapida; tocar la pestaña abierta la cierra.
    fn open_tab(&mut self, t: Hit) {
        if self.shown == t {
            self.clear_search();
            return;
        }
        self.clear_search();
        self.shown = t;
        let tag = K_LISTS | (self.seq & 0xffffff);
        match t {
            Hit::Again => self.eng.call("home", json!("again"), tag),
            Hit::Quick => self.eng.call("home", json!("quick"), tag),
            _ => self.eng.call("playlists", Value::Null, tag),
        }
        self.flash("Cargando…");
    }

    fn set_vol(&mut self, v: f64) {
        self.vol = v.clamp(0.0, 100.0).round();
        self.muted = false;
        self.vol_at = Instant::now();
        self.invalidate();
        if self.vol as i32 != self.vol_sent {
            self.vol_sent = self.vol as i32;
            self.eng.call("volume", json!(self.vol_sent), K_OTHER);
        }
    }

    fn vol_from_x(&mut self, x: i32) {
        let b = self.zone(Hit::VolBar);
        self.set_vol((x - b.left) as f64 / (b.right - b.left) as f64 * 100.0);
    }

    fn toggle_mini(&mut self) {
        self.mini = !self.mini;
        self.panel_hide_now();
        self.clear_search();
        self.hover = Hit::None;
    }

    /// Selectores del puente: reusan la lista de resultados.
    fn br_pick(&mut self, t: Hit) {
        if self.shown == t {
            self.clear_search();
            return;
        }
        let it = |title: &str, sub: String, pick: &str| Item { id: None, list: None, title: title.into(), sub, pick: Some(pick.into()) };
        // El bot vive en un servidor: los canales de todos van juntos (el servidor solo se muestra si hay varios).
        let many = self.br_guilds.len() > 1;
        let items: Vec<Item> = self
            .br_guilds
            .iter()
            .flat_map(|g| g.channels.iter().map(move |(id, n)| (g, id, n)))
            .map(|(g, id, n)| it(n, if many { g.name.clone() } else { String::new() }, &format!("{}:{id}", g.id)))
            .collect();
        if items.is_empty() {
            self.flash(if self.br_guilds.is_empty() { "Bot sin servidor: invitalo desde ⚙" } else { "El servidor no tiene canales de voz" });
            return;
        }
        self.clear_search();
        self.shown = t;
        self.set_items(items);
    }

    fn br_go(&mut self) {
        let msg = if self.br_busy() {
            json!({ "cmd": "leave" })
        } else {
            let (Some(g), Some(c)) = (self.br_guild.clone(), self.br_channel.clone()) else {
                self.flash("Elegí servidor y canal");
                return;
            };
            json!({ "cmd": "join", "guild": g, "channel": c, "device": self.br_device })
        };
        if let Some(b) = self.br.as_mut() {
            b.send(msg);
        }
    }

    /// Abre Discord en una ventana real al costado de la card, o la cierra (vuelve toda su RAM).
    fn dsc_toggle(&mut self) {
        if self.dsc_open() {
            self.dsc_win = None; // Drop cierra el Job object: se lleva todo el proceso
        } else if let Some(exe) = brave::find_brave() {
            let (w, h) = (self.px(480), self.px(720));
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            unsafe { GetWindowRect(self.hwnd, &mut rc) };
            // A la derecha si entra; si no, a la izquierda.
            let x = if rc.right + 8 + w <= sw { rc.right + 8 } else { (rc.left - 8 - w).max(0) };
            match brave::launch_discord(&exe, x, rc.top.max(0), w, h) {
                Ok(p) => self.dsc_win = Some(p),
                Err(e) => self.flash(&format!("No pude abrir Discord: {e}")),
            }
        }
        self.invalidate();
    }
    fn dsc_open(&self) -> bool {
        self.dsc_win.as_ref().is_some_and(|p| !p.wait(0))
    }

        /// El token se pega desde el portapapeles y nunca se muestra.
    fn br_paste_token(&mut self) {
        let t = unsafe { clipboard() }.trim().to_string();
        if t.split('.').count() != 3 {
            self.flash("Copiá el token del bot y tocá de nuevo");
            return;
        }
        if let Some(b) = self.br.as_mut() {
            b.send(json!({ "cmd": "token", "token": t }));
        }
        self.br_has_token = true;
        self.flash("Token cargado");
        self.panel_render();
    }

    fn mix_send(&mut self, v: Value) {
        if let Some(b) = self.br.as_mut() {
            b.send(v);
        }
    }
    fn on_bridge(&mut self, v: Value) {
        let strs = |x: &Value| x.as_str().map(String::from);
        match v["ev"].as_str() {
            Some("devices") => {
                self.br_devices = v["list"].as_array().map(|a| a.iter().filter_map(strs).collect()).unwrap_or_default();
                // el nombre guardado puede ser parcial ("CABLE Output"): se completa con el real
                if let Some(d) = self.br_devices.iter().find(|d| d.starts_with(&self.br_device)) {
                    self.br_device = d.clone();
                }
            }
            Some("config") => {
                self.br_has_token = v["token"].as_bool().unwrap_or(false);
                if let Some(d) = v["device"].as_str() {
                    self.br_device = self.br_devices.iter().find(|x| x.starts_with(d)).cloned().unwrap_or_else(|| d.into());
                }
                self.br_guild = strs(&v["guild"]);
                self.br_channel = strs(&v["channel"]);
            }
            Some("guilds") => {
                self.br_guilds = v["list"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|g| BrGuild {
                                id: strs(&g["id"]).unwrap_or_default(),
                                name: strs(&g["name"]).unwrap_or_default(),
                                channels: g["channels"]
                                    .as_array()
                                    .map(|c| c.iter().map(|c| (strs(&c["id"]).unwrap_or_default(), strs(&c["name"]).unwrap_or_default())).collect())
                                    .unwrap_or_default(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // con un solo servidor, se elige solo
                if self.br_guild.is_none() && self.br_guilds.len() == 1 {
                    self.br_guild = Some(self.br_guilds[0].id.clone());
                }
            }
            Some("state") => {
                self.br_state = strs(&v["s"]).unwrap_or_default();
                if self.br_state == "sin_token" {
                    self.br_has_token = false;
                }
                if self.br_state == "error" {
                    self.br_err = v["msg"].as_str().unwrap_or("error").to_string();
                    let m = format!("Puente: {}", self.br_err);
                    engine::log(&m); // la card corta el mensaje; el texto entero queda en engine.log
                    self.flash(&m);
                }
                if self.br_state != "transmitiendo" {
                    self.br_level = 0.0;
                }
            }
            Some("invite") => {
                // Pagina de Discord para agregar el bot: se abre en el navegador elegido (perfil de siempre).
                if let (Some(url), Some(exe)) = (v["url"].as_str(), brave::find_brave()) {
                    let _ = std::process::Command::new(exe).arg(url).spawn();
                }
            }
            Some("level") => self.br_level = v["v"].as_f64().unwrap_or(0.0) as f32,
            Some("apps") if self.mix_drag.is_none() => {
                self.mix_cable = v["cable"] != false;
                self.mix_apps = v["list"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|x| MixApp {
                                pid: x["pid"].as_u64().unwrap_or(0),
                                name: strs(&x["name"]).unwrap_or_default(),
                                on: x["on"] == true,
                                vol: x["vol"].as_f64().unwrap_or(100.0),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if self.mix_sel.is_some_and(|p| !self.mix_apps.iter().any(|a| a.pid == p)) {
                    self.mix_sel = None;
                }
                self.panel_render();
            }
            Some("mix_error") => {
                let m = format!("Mezclador: {}", v["msg"].as_str().unwrap_or("error"));
                self.flash(&m);
            }
            _ => {}
        }
        if self.panel == Panel::Config {
            self.panel_render(); // token, entradas
        }
        self.invalidate();
    }

    fn query_changed(&mut self) {
        self.caret_on = true;
        self.shown = Hit::None;
        unsafe { SetTimer(self.hwnd, T_SEARCH, 250, None) };
        self.invalidate();
    }

    fn play(&mut self, i: usize, radio: bool) {
        let Some(it) = self.items.get(i) else { return };
        if let (Some(v), Hit::BrChannel) = (it.pick.clone(), self.shown) {
            if let Some((g, c)) = v.split_once(':') {
                self.br_guild = Some(g.into());
                self.br_channel = Some(c.into());
            }
            self.clear_search();
            return;
        }
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
            Hit::Lists | Hit::Again | Hit::Quick => self.open_tab(self.hit(x, y)),
            Hit::Vol => {
                self.muted = !self.muted;
                self.vol_at = Instant::now();
                self.invalidate();
                self.cmd("mute");
            }
            Hit::VolBar => {
                self.dragging = true;
                unsafe { SetCapture(self.hwnd) };
                self.vol_from_x(x);
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
            Hit::Gear => self.panel_open(Panel::Config),
            Hit::BrApps => self.panel_open(Panel::Apps),
            Hit::BrChannel | Hit::BrGo if !self.br_has_token => self.panel_open(Panel::Config),
            Hit::BrChannel => self.br_pick(Hit::BrChannel),
            Hit::BrGo => self.br_go(),
            Hit::DscRow => self.dsc_toggle(),
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
                if self.vol_at.elapsed().as_millis() > 800 && !self.dragging {
                    self.vol = s["vol"].as_f64().unwrap_or(100.0);
                    self.vol_sent = self.vol as i32;
                    self.muted = s["muted"].as_bool().unwrap_or(false);
                }
                self.at = Instant::now();
                self.update_tick();
            }
            Ev::Reply(tag, r) => {
                let ok = r["ok"].as_bool().unwrap_or(false);
                let kind = tag & K_MASK;
                let current = (tag & 0xffffff) == (self.seq & 0xffffff);
                if !ok {
                    if kind == K_OTHER || current {
                        if kind != K_OTHER {
                            self.shown = Hit::None;
                        }
                        self.flash(r["error"].as_str().unwrap_or("error"));
                    }
                    return;
                }
                if kind != K_OTHER && current {
                    let items: Vec<Item> = r["data"].as_array().map(|a| a.iter().map(Item::from_json).collect()).unwrap_or_default();
                    self.msg.clear();
                    if items.is_empty() {
                        self.flash(match (kind, self.shown) {
                            (K_LISTS, Hit::Lists) => "Sin listas (¿sesión iniciada?)",
                            (K_LISTS, _) => "No encontré esa sección",
                            _ => "Sin resultados",
                        });
                        self.shown = Hit::None;
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
        let (w, h) = (self.width(), self.height());
        if self.canvas.as_ref().map_or(true, |c| c.w != w || c.h != h) {
            self.canvas = None;
            self.canvas = Some(Canvas::new(w, h));
        }
        let mut cv = self.canvas.take().unwrap();
        cv.fill_bg();
        let s = self.scale;
        let wf = w as f32;

        // Borde finito (hairline).
        cv.round(0.0, 0.0, wf, h as f32, RADIUS * s, th().hairline);
        cv.round(1.0, 1.0, wf - 1.0, h as f32 - 1.0, RADIUS * s - 1.0, th().bg);

        let (line1, line2) = self.lines();
        let line2 = if self.msg.is_empty() { line2 } else { self.msg.as_str() };
        let c2 = if self.msg.is_empty() { th().dim } else { PINK };

        // Punto de estado (lo reemplaza el icono de usuario si no hay sesion).
        let dot = if !self.ready {
            CYAN
        } else if !self.msg.is_empty() && self.msg.starts_with("Error") {
            RED
        } else if self.ad {
            AMBER
        } else if self.paused || self.title.is_empty() {
            th().dim3
        } else {
            GREEN
        };
        if self.mini {
            // Colapsada conserva el punto de estado (verde sonando, gris en pausa).
            let (dx, dy) = (self.pf(16.0), self.pf((H_MINI / 2) as f32));
            cv.dot(dx, dy, self.pf(4.0), dot);
            GdiFlush();
            cv.text(self.f.title, line1, RECT { left: self.px(28), top: self.px(6), right: w - self.px(28), bottom: self.px(27) }, th().ink, DT_CENTER);
            cv.text(self.f.small, line2, RECT { left: self.px(28), top: self.px(26), right: w - self.px(28), bottom: self.px(43) }, c2, DT_CENTER);
            GdiFlush();
            cv.finish(RADIUS * s);
            self.present(cv, w, h);
            return;
        }

        let login = self.ready && !self.logged;
        let _ = dot;

        // Botones redondos del encabezado (hover; el engranaje se resalta mientras su card esta afuera).
        for hb in [Hit::Close, Hit::Login, Hit::Gear] {
            if self.hover == hb || (hb == Hit::Gear && !login && self.panel == Panel::Config && self.panel_target > 0.0) {
                let z = self.zone(hb);
                cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(14.0), th().tab);
            }
        }

        // Controles: hover en circulo; play como boton blanco.
        for (k, hb) in [(-2, Hit::Shuffle), (-1, Hit::Prev), (1, Hit::Next), (2, Hit::Repeat)] {
            if self.hover == hb {
                let c = self.pf(self.ctrl_center(k) as f32);
                let cy = self.pf((Y_CTRL + 20) as f32);
                cv.round(c - self.pf(17.0), cy - self.pf(17.0), c + self.pf(17.0), cy + self.pf(17.0), self.pf(17.0), th().tab);
            }
        }
        let (pc, pcy) = (self.pf(self.ctrl_center(0) as f32), self.pf((Y_CTRL + 20) as f32));
        let pr = self.pf(if self.hover == Hit::Play { 18.0 } else { 17.0 });
        cv.round(pc - pr, pcy - pr, pc + pr, pcy + pr, pr, th().ink);
        cv.opaque.push((pc, pcy, pr));

        // Progreso y volumen: misma barra fina.
        let (bl, br) = (self.pf(54.0), self.pf((W - 54) as f32));
        let by = self.pf((Y_PROG + 10) as f32);
        let bh = self.pf(if self.hover == Hit::Bar { 3.0 } else { 2.0 });
        cv.round(bl, by - bh, br, by + bh, bh, th().hover);
        if self.dur > 0.0 {
            let f = (self.cur_pos() / self.dur).clamp(0.0, 1.0) as f32;
            let x = bl + (br - bl) * f;
            if x - bl > 1.0 {
                cv.round(bl, by - bh, x, by + bh, bh, if self.ad { AMBER } else { th().accent });
            }
            if self.hover == Hit::Bar {
                cv.round(x - self.pf(6.0), by - self.pf(6.0), x + self.pf(6.0), by + self.pf(6.0), self.pf(6.0), th().ink);
            }
        }
        let vy = self.pf((Y_VOL + 10) as f32);
        let vhot = self.hover == Hit::VolBar || self.dragging;
        let vh = self.pf(if vhot { 3.0 } else { 2.0 });
        cv.round(bl, vy - vh, br, vy + vh, vh, th().hover);
        let vx = bl + (br - bl) * (self.vol / 100.0) as f32;
        if vx - bl > 1.0 {
            cv.round(bl, vy - vh, vx, vy + vh, vh, if self.muted { th().dim3 } else { th().dim });
        }
        if vhot {
            cv.round(vx - self.pf(6.0), vy - self.pf(6.0), vx + self.pf(6.0), vy + self.pf(6.0), self.pf(6.0), th().ink);
        }

        // Buscador de punta a punta y, debajo, las tres pestañas.
        let fz = self.zone(Hit::Field);
        cv.round(fz.left as f32, fz.top as f32, fz.right as f32, fz.bottom as f32, self.pf(12.0),
            if self.active || self.hover == Hit::Field { th().tab } else { th().card });
        for t in [Hit::Lists, Hit::Again, Hit::Quick] {
            let z = self.zone(t);
            let bg = if self.shown == t { th().accent } else if self.hover == t { th().hover } else { th().tab };
            cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(12.0), bg);
        }

        // Puente: una fila con estado + canal, conectar/salir y apps a Discord.
        if self.br_shown() {
            let z = self.r(12, Y_BR, W - 12, Y_BR + 32);
            cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(12.0), th().card);
            let c = self.zone(Hit::BrChannel);
            if self.hover == Hit::BrChannel || self.shown == Hit::BrChannel {
                cv.round(c.left as f32, c.top as f32, c.right as f32, c.bottom as f32, self.pf(12.0), th().hover);
            }
            for hb in [Hit::BrGo, Hit::BrApps] {
                let b = self.zone(hb);
                let on = hb == Hit::BrApps && self.panel == Panel::Apps && self.panel_target > 0.0;
                if self.hover == hb || on {
                    cv.round(b.left as f32, b.top as f32, b.right as f32, b.bottom as f32, self.pf(10.0), if on { th().accent } else { th().hover });
                }
            }
            let dot = match self.br_state.as_str() {
                "transmitiendo" => GREEN,
                "conectando" | "entrando" => CYAN,
                "error" => RED,
                "listo" => th().dim,
                _ => th().dim3,
            };
            let (dx, dy) = (z.left as f32 + self.pf(14.0), (z.top + z.bottom) as f32 / 2.0 - self.pf(1.0));
            cv.dot(dx, dy, self.pf(4.0), dot);
            if self.br_state == "transmitiendo" {
                let (l, r, y) = (c.left as f32 + self.pf(26.0), c.right as f32 - self.pf(6.0), z.bottom as f32 - self.pf(5.0));
                cv.round(l, y - self.pf(1.0), r, y + self.pf(1.0), self.pf(1.0), th().hover);
                let x = l + (r - l) * self.br_level.clamp(0.0, 1.0);
                if x - l > 1.0 {
                    cv.round(l, y - self.pf(1.0), x, y + self.pf(1.0), self.pf(1.0), GREEN);
                }
            }
        }

        // Seccion Discord: fila de estado (colapsable) y, abierta, controles de voz + integrantes.
        if self.dsc_shown() {
            let z = self.zone(Hit::DscRow);
            cv.round(z.left as f32, z.top as f32, z.right as f32, z.bottom as f32, self.pf(12.0),
                if self.hover == Hit::DscRow { th().hover } else { th().card });
            let (dx, dy) = (z.left as f32 + self.pf(14.0), (z.top + z.bottom) as f32 / 2.0);
            cv.dot(dx, dy, self.pf(4.0), if self.dsc_open() { GREEN } else { th().dim3 });
        }

        // Resultados en una tarjeta.
        let n = self.rows_visible();
        if n > 0 {
            let top = self.pf(self.base() as f32);
            let bottom = self.pf((self.row_y(n) + 4) as f32);
            cv.round(self.pf(12.0), top, wf - self.pf(12.0), bottom, self.pf(13.0), th().card);
            for vis in 0..n {
                let i = self.scroll + vis;
                let hot = matches!(self.hover, Hit::Row(j) | Hit::Radio(j) if j == i);
                if hot || self.sel == Some(i) {
                    let rr = self.row_rect(vis);
                    cv.round(rr.left as f32, rr.top as f32, rr.right as f32, rr.bottom as f32, self.pf(9.0), th().tab);
                    if self.items[i].id.is_some() {
                        let rb = self.radio_rect(vis);
                        cv.round(rb.left as f32, rb.top as f32, rb.right as f32, rb.bottom as f32, self.pf(11.0),
                            if self.hover == Hit::Radio(i) { th().accent } else { th().hover });
                    }
                }
            }
            if self.items.len() > MAX_ROWS {
                let span = (n as i32 * ROW) as f32;
                let thumb = (span * MAX_ROWS as f32 / self.items.len() as f32).max(14.0);
                let ty = self.row_y(0) as f32 + (span - thumb) * self.scroll as f32 / (self.items.len() - MAX_ROWS) as f32;
                cv.round(wf - self.pf(16.0), self.pf(ty), wf - self.pf(13.0), self.pf(ty + thumb), self.pf(1.5), th().hover);
            }
        }

        GdiFlush();

        // ---- Texto ----
        cv.text(self.f.title, line1, self.r(40, 8, W - 40, 28), th().ink, DT_CENTER);
        cv.text(self.f.small, line2, self.r(40, 27, W - 40, 44), c2, DT_CENTER);
        cv.text(self.f.icon, "\u{E8BB}", self.zone(Hit::Close), if self.hover == Hit::Close { th().ink } else { th().dim }, DT_CENTER);
        if login {
            cv.text(self.f.icon, "\u{E77B}", self.zone(Hit::Login), if self.hover == Hit::Login { th().ink } else { AMBER }, DT_CENTER);
        } else {
            cv.text(self.f.icon, "\u{E713}", self.zone(Hit::Gear), if self.hover == Hit::Gear { th().ink } else { th().dim }, DT_CENTER);
        }

        let ic = |hb: Hit| if self.hover == hb { th().ink } else { th().dim };
        cv.text(self.f.icon, "\u{E8B1}", self.zone(Hit::Shuffle), ic(Hit::Shuffle), DT_CENTER);
        cv.text(self.f.icon_big, "\u{E892}", self.zone(Hit::Prev), ic(Hit::Prev), DT_CENTER);
        cv.text(self.f.icon_big, if self.paused { "\u{E768}" } else { "\u{E769}" }, self.zone(Hit::Play), th().bg, DT_CENTER);
        cv.text(self.f.icon_big, "\u{E893}", self.zone(Hit::Next), ic(Hit::Next), DT_CENTER);
        let (rg, rc) = match self.repeat.as_str() {
            "ONE" => ("\u{E8ED}", th().accent),
            "ALL" => ("\u{E8EE}", th().accent),
            _ => ("\u{E8EE}", ic(Hit::Repeat)),
        };
        cv.text(self.f.icon, rg, self.zone(Hit::Repeat), rc, DT_CENTER);

        if self.dur > 0.0 {
            cv.text(self.f.small, &mmss(self.cur_pos()), self.r(6, Y_PROG, 48, Y_PROG + 20), th().dim, DT_RIGHT);
            cv.text(self.f.small, &mmss(self.dur), self.r(W - 48, Y_PROG, W - 6, Y_PROG + 20), th().dim, DT_LEFT);
        }
        let vg = if self.muted || self.vol < 1.0 {
            "\u{E74F}"
        } else if self.vol < 34.0 {
            "\u{E993}"
        } else if self.vol < 67.0 {
            "\u{E994}"
        } else {
            "\u{E995}"
        };
        cv.text(self.f.icon, vg, self.zone(Hit::Vol), if self.hover == Hit::Vol { th().ink } else { th().dim }, DT_RIGHT);
        cv.text(self.f.small, &format!("{}", self.vol as i32), self.r(W - 48, Y_VOL, W - 6, Y_VOL + 20), th().dim, DT_LEFT);

        cv.text(self.f.icon, "\u{E721}", self.r(22, Y_SRCH, 40, Y_SRCH + 36), th().dim3, DT_CENTER);
        let field = self.r(44, Y_SRCH, W - 24, Y_SRCH + 36);
        if self.query.is_empty() {
            cv.text(self.f.text, "Buscar canción…", field, th().dim3, DT_LEFT);
        } else {
            // Si no entra, se ve el final (donde se escribe).
            let tw = measure(self.f.text, &self.query);
            let fw = field.right - field.left;
            let mut f2 = field;
            if tw > fw {
                f2.left = field.right - tw;
            }
            let wq: Vec<u16> = self.query.encode_utf16().collect();
            let old = SelectObject(cv.dc, self.f.text);
            SetTextColor(cv.dc, th().ink);
            let rgn = CreateRectRgn(field.left, field.top, field.right, field.bottom);
            SelectClipRgn(cv.dc, rgn);
            DrawTextW(cv.dc, wq.as_ptr(), wq.len() as i32, &mut f2, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            SelectClipRgn(cv.dc, null_mut());
            DeleteObject(rgn);
            SelectObject(cv.dc, old);
        }
        if self.active && self.caret_on {
            let tw = if self.query.is_empty() { 0 } else { measure(self.f.text, &self.query) };
            let x = field.left + tw.min(field.right - field.left);
            GdiFlush();
            cv.round(x as f32, self.pf((Y_SRCH + 10) as f32), x as f32 + self.pf(1.5).max(1.0), self.pf((Y_SRCH + 26) as f32), 0.5, th().accent);
        }
        for (t, label) in [(Hit::Lists, "Listas"), (Hit::Again, "Escuchar otra vez"), (Hit::Quick, "Selección rápida")] {
            let mut z = self.zone(t);
            z.left += self.px(4);
            z.right -= self.px(4);
            cv.text_wrap(self.f.small, label, z, if self.shown == t || self.hover == t { th().ink } else { th().dim });
        }

        if self.br_shown() {
            let c = self.zone(Hit::BrChannel);
            let chan = self.br_channel_name().map(|c| format!("#{c}"));
            let label = if !self.br_has_token {
                "Puente · configurar".to_string()
            } else {
                match (self.br_state.as_str(), chan) {
                    ("transmitiendo", Some(c)) => format!("{c} · al aire"),
                    ("entrando", Some(c)) => format!("Entrando a {c}…"),
                    ("conectando", _) => "Puente · conectando…".into(),
                    ("listo", None) if self.br_guilds.is_empty() => "Invitá el bot al servidor".into(),
                    ("error", _) => format!("Puente · {}", self.br_err),
                    (_, Some(c)) => c,
                    _ => "Puente · elegí canal".into(),
                }
            };
            let tz = RECT { left: c.left + self.px(26), right: c.right - self.px(6), bottom: c.bottom - self.px(2), ..c };
            cv.text(self.f.small, &label, tz, if self.br_state == "transmitiendo" { th().ink } else { th().dim }, DT_LEFT);
            let ready = self.br_has_token && self.br_channel.is_some();
            let (gi, gc) = if self.br_busy() { ("\u{E778}", RED) } else { ("\u{E717}", if ready { th().ink } else { th().dim3 }) };
            cv.text(self.f.icon, gi, self.zone(Hit::BrGo), gc, DT_CENTER);
            cv.text(self.f.icon, "\u{E8D6}", self.zone(Hit::BrApps), if self.mix_apps.iter().any(|a| a.on) { GREEN } else { th().ink }, DT_CENTER);
        }

        if self.dsc_shown() {
            let z = self.zone(Hit::DscRow);
            let open = self.dsc_open();
            let label = if open { "Discord · abierto (clic cierra)" } else { "Discord · abrir ventana" };
            let tz = RECT { left: z.left + self.px(26), top: z.top, right: z.right - self.px(26), bottom: z.bottom };
            cv.text(self.f.small, label, tz, if open { th().ink } else { th().dim }, DT_LEFT);
            let cz = RECT { left: z.right - self.px(26), ..z };
            cv.text(self.f.icon, if open { "\u{E711}" } else { "\u{E8A7}" }, cz, th().dim, DT_CENTER);
        }

        for vis in 0..n {
            let i = self.scroll + vis;
            let it = &self.items[i];
            let y = self.row_y(vis);
            let hot = matches!(self.hover, Hit::Row(j) | Hit::Radio(j) if j == i) || self.sel == Some(i);
            let right = if it.id.is_some() && hot { W - 82 } else { W - 26 };
            cv.text(self.f.text, &it.title, self.r(26, y + 4, right, y + 22), th().ink, DT_LEFT);
            cv.text(self.f.small, &it.sub, self.r(26, y + 21, right, y + 37), th().dim, DT_LEFT);
            if it.id.is_some() && hot {
                cv.text(self.f.small, "Radio", self.radio_rect(vis), th().ink, DT_CENTER);
            }
        }

        GdiFlush();
        cv.finish(RADIUS * s);
        self.present(cv, w, h);
    }

    /// Vuelca el lienzo a la ventana. Si cambia el ancho se mantiene el centro (colapsar
    /// encoge hacia el medio); si al crecer se sale del monitor, se corre hacia arriba lo justo.
    unsafe fn present(&mut self, cv: Canvas, w: i32, h: i32) {
        let mut wr: RECT = zeroed();
        GetWindowRect(self.hwnd, &mut wr);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
        let left = wr.left + ((wr.right - wr.left) - w) / 2;
        let top = wr.top.min(mi.rcWork.bottom - h).max(mi.rcWork.top);
        let dst = POINT { x: left, y: top };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(self.hwnd, null_mut(), &dst, &size, cv.dc, &src, 0, &blend, ULW_ALPHA);
        self.canvas = Some(cv);
        let mut c = self.blur_main;
        theme::blur(self.hwnd, &mut c, w, w, 0, h, (RADIUS * self.scale).round() as i32);
        self.blur_main = c;
        self.panel_present();
    }
}

/// Muestra la ventana arriba de todo; con `focus` tambien le da el teclado.
unsafe fn show(hwnd: HWND, focus: bool) {
    ShowWindow(hwnd, if focus { SW_SHOW } else { SW_SHOWNOACTIVATE });
    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | if focus { 0 } else { SWP_NOACTIVATE });
    if focus {
        SetForegroundWindow(hwnd);
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

/// ClearType asume un fondo opaco y la card es translucida: suavizado en escala de grises.
unsafe fn font(px: i32, weight: i32, face: &str) -> HFONT {
    CreateFontW(-px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32, ANTIALIASED_QUALITY as u32, DEFAULT_PITCH as u32, wide(face).as_ptr())
}

unsafe fn make_fonts(scale: f32) -> Fonts {
    let px = |v: i32| (v as f32 * scale).round() as i32;
    Fonts {
        title: font(px(14), 600, "Segoe UI"),
        text: font(px(13), 400, "Segoe UI"),
        small: font(px(11), 400, "Segoe UI"),
        icon: font(px(13), 400, "Segoe MDL2 Assets"),
        icon_big: font(px(15), 400, "Segoe MDL2 Assets"),
    }
}

fn pos_file() -> std::path::PathBuf {
    brave::data_dir().join("pos.txt")
}

unsafe fn save_pos(hwnd: HWND) {
    let mut r: RECT = zeroed();
    GetWindowRect(hwnd, &mut r);
    let _ = std::fs::create_dir_all(brave::data_dir());
    // Se guarda la posicion de la tarjeta expandida aunque este colapsada (mismo centro).
    let left = r.left + (r.right - r.left) / 2 - app().px(W) / 2;
    let _ = std::fs::write(pos_file(), format!("{} {}", left, r.top));
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
            // El encabezado (fuera de los botones) arrastra la ventana; colapsada, toda.
            if a.mini || (p.y < a.px(Y_CTRL) && a.hit(p.x, p.y) == Hit::None) {
                HTCAPTION as isize
            } else {
                HTCLIENT as isize
            }
        }
        // Clic en botones sin robarle el foco al juego; solo el buscador toma el teclado.
        WM_MOUSEACTIVATE => {
            let mut p: POINT = zeroed();
            GetCursorPos(&mut p);
            ScreenToClient(hwnd, &mut p);
            if a.hit(p.x, p.y) == Hit::Field { MA_ACTIVATE as isize } else { MA_NOACTIVATE as isize }
        }
        WM_HOTKEY => {
            match wp as i32 {
                HK_TOGGLE | HK_MEDIA_PLAY => a.toggle(),
                HK_NEXT | HK_MEDIA_NEXT => a.cmd("next"),
                HK_PREV | HK_MEDIA_PREV => a.cmd("prev"),
                HK_MINI => {
                    if IsWindowVisible(hwnd) == 0 {
                        show(hwnd, false);
                    }
                    a.toggle_mini();
                }
                HK_SHOW => {
                    if IsWindowVisible(hwnd) != 0 {
                        a.panel_hide_now();
                        ShowWindow(hwnd, SW_HIDE);
                    } else {
                        show(hwnd, false);
                    }
                }
                HK_SEARCH => {
                    a.mini = false;
                    show(hwnd, true);
                    a.clear_search();
                }
                _ => {}
            }
            0
        }
        // Doble clic en la caja de cancion/artista (zona de arrastre) colapsa o expande.
        WM_NCLBUTTONDBLCLK if wp as u32 == HTCAPTION => {
            let mut p = POINT { x: mx, y: my };
            ScreenToClient(hwnd, &mut p);
            if a.in_title_box(p.x, p.y) {
                a.toggle_mini();
            }
            0
        }
        WM_LBUTTONDOWN => {
            a.click(mx, my);
            0
        }
        WM_LBUTTONUP => {
            if a.dragging {
                a.dragging = false;
                ReleaseCapture();
                a.invalidate();
            }
            0
        }
        WM_MOUSEMOVE if a.dragging => {
            a.vol_from_x(mx);
            0
        }
        WM_CAPTURECHANGED => {
            a.dragging = false;
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
        // Rueda: sobre los resultados desplaza; en el resto de la tarjeta, volumen.
        WM_MOUSEWHEEL => {
            let n = a.items.len();
            let d = ((wp >> 16) & 0xffff) as i16;
            let mut p = POINT { x: mx, y: my };
            ScreenToClient(hwnd, &mut p);
            if !a.mini && n > 0 && p.y >= a.px(a.base()) {
                if n > MAX_ROWS {
                    a.scroll = if d > 0 { a.scroll.saturating_sub(2) } else { (a.scroll + 2).min(n - MAX_ROWS) };
                    a.invalidate();
                }
            } else {
                a.set_vol(a.vol + if d > 0 { 5.0 } else { -5.0 });
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
                T_MIX => a.mix_send(json!({ "cmd": "apps" })),
                panel::T_PANEL => a.panel_tick(),
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
        bridge::WM_BRIDGE => {
            let v = Box::from_raw(lp as *mut Value);
            a.on_bridge(*v);
            0
        }
        WM_ENGINE => {
            let ev = Box::from_raw(lp as *mut Ev);
            a.on_engine(*ev);
            0
        }
        // La card de costado sigue a la principal.
        WM_WINDOWPOSCHANGED => {
            a.panel_present();
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_EXITSIZEMOVE => {
            save_pos(hwnd);
            0
        }
        WM_DESTROY => {
            a.br = None; // cierra stdin: el puente sale del canal y termina
            for &(id, _, _) in &a.hotkeys {
                UnregisterHotKey(hwnd, id);
            }
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
        // Una sola instancia: dos se pelearian por el mismo perfil de Brave.
        CreateMutexW(null(), 0, wide("ytm-float-instancia").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let w = FindWindowW(wide("ytm-float").as_ptr(), null());
            if !w.is_null() {
                show(w, true);
            }
            return;
        }
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
            f: make_fonts(scale),
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
            vol: 100.0,
            muted: false,
            vol_at: Instant::now(),
            vol_sent: 100,
            dragging: false,
            mini: false,
            query: String::new(),
            active: false,
            caret_on: true,
            items: Vec::new(),
            shown: Hit::None,
            scroll: 0,
            sel: None,
            hover: Hit::None,
            tracking: false,
            seq: 0,
            br: None,
            mix_apps: Vec::new(),
            mix_cable: true,
            mix_sel: None,
            mix_drag: None,
            br_state: String::new(),
            br_err: String::new(),
            br_has_token: false,
            br_level: 0.0,
            br_devices: Vec::new(),
            br_guilds: Vec::new(),
            br_device: "CABLE Output".into(),
            br_guild: None,
            br_channel: None,
            dsc_avail: false,
            dsc_win: None,
            panel: Panel::None,
            panel_hwnd: panel::create(hwnd, hinst),
            panel_canvas: None,
            panel_t: 0.0,
            panel_target: 0.0,
            panel_last: Instant::now(),
            panel_sx: 0,
            panel_hover: PHit::None,
            panel_tracking: false,
            panel_capture: None,
            browsers: Vec::new(),
            browser_cur: brave::find_brave().unwrap_or_default(),
            hotkeys: panel::load_hotkeys(),
            cfg_all_dev: false,
            blur_main: (0, 0, 0),
            blur_panel: (0, 0, 0),
            panel_edit: None,
            sec_open: [true, true, true, false, false, false],
            show_br: panel::card_pref("puente"),
            show_dsc: panel::card_pref("discord"),
        });
        APP = Box::into_raw(a);
        app().br = bridge::Bridge::start(hwnd as isize); // modulo opcional
        app().dsc_avail = std::env::current_exe().map(|e| e.with_file_name("discord.module").exists()).unwrap_or(false);
        app().render(); // una ventana en capas no se ve hasta el primer UpdateLayeredWindow
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Atajos globales (andan dentro de juegos). Si otra app ya tiene alguno, se ignora.
        for &(id, m, vk) in &app().hotkeys {
            if RegisterHotKey(hwnd, id, m | MOD_NOREPEAT, vk) == 0 {
                engine::log(&format!("atajo {id} (vk {vk:#x}) tomado por otra app"));
            }
        }

        let mut m: MSG = zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}
