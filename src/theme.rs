// Tema visual: color de fondo y acento elegidos por el usuario. El resto de los colores (tarjeta,
// boton, hover, borde, texto) se derivan del fondo, asi cambiar uno repinta todo. Se guarda en
// theme.json. La card es translucida con una opacidad fija (OPACITY) y Windows desenfoca lo de
// atras: no suma memoria ni CPU, solo se deja pasar un poco de cada pixel al presentar.
use super::{brave, rgb};
use serde_json::{json, Value};
use windows_sys::Win32::{Foundation::HWND, Graphics::Gdi::*};

/// Cuanto tapa la card (0.87 = se transparenta un 13 %, justo para sentir el vidrio sin perder letras).
pub const OPACITY: f32 = 0.87;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style {
    Instrumento,
    Minimal,
    Fatuo, // "fuego fatuo": el acento como fuente de luz
}

impl Style {
    /// Orden del desplegable de temas.
    pub const ALL: [Style; 3] = [Style::Instrumento, Style::Minimal, Style::Fatuo];

    pub fn key(self) -> &'static str {
        match self {
            Style::Instrumento => "instrumento",
            Style::Minimal => "minimal",
            Style::Fatuo => "fatuo",
        }
    }

    /// Clave de i18n con su nombre.
    pub fn label(self) -> &'static str {
        match self {
            Style::Instrumento => "style_inst",
            Style::Minimal => "style_min",
            Style::Fatuo => "style_fatuo",
        }
    }
}

#[derive(Clone)]
pub struct Theme {
    pub accent: u32, // COLORREF (0x00BBGGRR), como el resto de los colores
    // derivados del fondo
    pub bg: u32,
    pub card: u32,
    pub tab: u32,
    pub hover: u32,
    pub hairline: u32,
    pub ink: u32,
    pub dim: u32,
    pub dim3: u32,
    pub style: Style,
    pub audio: bool, // reaccion al audio (vumetro), apagada por defecto
    // derivados del estilo Instrumento
    pub light: bool,
    pub well: u32,    // zonas hundidas
    pub led_off: u32, // LEDs y segmentos apagados
    pub top: u32,     // arriba del gradiente de la card
    pub glow_a: f32,  // intensidad del brillo del acento
    pub inset_a: f32, // sombra interior de los hundidos
    pub lit: u32,     // acento para glifos, LEDs, segmentos y filos: se acerca al texto hasta contrastar 3:1
}

fn ch(c: u32) -> (f32, f32, f32) {
    ((c & 0xff) as f32, ((c >> 8) & 0xff) as f32, ((c >> 16) & 0xff) as f32)
}

/// `a` hacia `b`, t de 0 a 1.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let (a, b) = (ch(a), ch(b));
    let m = |x: f32, y: f32| (x + (y - x) * t).round().clamp(0.0, 255.0) as u8;
    rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Contraste WCAG entre dos colores (1 a 21).
pub fn contrast(a: u32, b: u32) -> f32 {
    let rl = |c: u32| {
        let (r, g, b) = ch(c);
        let f = |v: f32| {
            let v = v / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
    };
    let (x, y) = (rl(a), rl(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn lum(c: u32) -> f32 {
    let (r, g, b) = ch(c);
    (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
}

/// COLORREF → pixel del DIB (0x00RRGGBB): se intercambian rojo y azul.
pub fn px(c: u32) -> u32 {
    (c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff
}

pub fn parse_hex(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    let h: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&h, 16).ok()?;
    Some(rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn hex(c: u32) -> String {
    let (r, g, b) = ch(c);
    format!("#{:02x}{:02x}{:02x}", r as u8, g as u8, b as u8)
}

impl Theme {
    pub fn default() -> Theme {
        Theme::new(rgb(0, 0, 0), rgb(0x63, 0x66, 0xf1))
    }

    pub fn new(bg: u32, accent: u32) -> Theme {
        let mut t = Theme {
            accent, bg, card: 0, tab: 0, hover: 0, hairline: 0, ink: 0, dim: 0, dim3: 0,
            style: Style::Instrumento, audio: false, light: false, well: 0, led_off: 0, top: 0, glow_a: 0.0, inset_a: 0.0, lit: 0,
        };
        t.derive();
        t
    }

    pub fn derive(&mut self) {
        let b = self.bg;
        // Fondo claro → texto oscuro, y los paneles se oscurecen en vez de aclararse.
        self.ink = if lum(b) > 0.6 { rgb(0x14, 0x15, 0x18) } else { rgb(0xf5, 0xf6, 0xf8) };
        self.card = mix(b, self.ink, 0.075);
        self.tab = mix(b, self.ink, 0.115);
        self.hover = mix(b, self.ink, 0.150);
        self.hairline = mix(b, self.ink, 0.112);
        self.dim = mix(b, self.ink, 0.60);
        self.dim3 = mix(b, self.ink, 0.44);
        self.light = lum(b) > 0.6;
        self.well = if self.light { mix(b, self.ink, 0.05) } else { mix(b, rgb(0, 0, 0), 0.35) };
        self.led_off = mix(b, self.ink, 0.14);
        self.top = mix(b, self.ink, 0.09);
        self.glow_a = if self.light { 0.35 } else { 0.70 };
        self.inset_a = if self.light { 0.18 } else { 0.60 };
        // Hacia el texto (oscuro en fondos claros, claro en oscuros) hasta contrastar 3:1.
        self.lit = self.accent;
        let mut k = 0.0;
        while k < 1.0 && (contrast(self.lit, self.tab) < 3.0 || contrast(self.lit, self.led_off) < 3.0) {
            k += 0.05;
            self.lit = mix(self.accent, self.ink, k);
        }
    }

    pub fn from_json(v: &Value) -> Theme {
        let d = Theme::default();
        let col = |k: &str, def: u32| v[k].as_str().and_then(parse_hex).unwrap_or(def);
        let mut t = Theme::new(col("fondo", d.bg), col("acento", d.accent));
        t.style = Style::ALL.into_iter().find(|s| v["estilo"].as_str() == Some(s.key())).unwrap_or(Style::Instrumento);
        t.audio = v["audio_reactivo"].as_bool().unwrap_or(false);
        t
    }

    pub fn to_json(&self) -> Value {
        json!({
            "fondo": hex(self.bg), "acento": hex(self.accent),
            "estilo": self.style.key(),
            "audio_reactivo": self.audio,
        })
    }

    pub fn load() -> Theme {
        let v: Value = std::fs::read_to_string(brave::data_dir().join("theme.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
        Theme::from_json(&v)
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(brave::data_dir());
        let _ = std::fs::write(brave::data_dir().join("theme.json"), self.to_json().to_string());
    }
}

static mut THEME: Option<Theme> = None;

#[allow(static_mut_refs)]
pub fn th() -> &'static Theme {
    unsafe { THEME.get_or_insert_with(Theme::load) }
}

#[allow(static_mut_refs)]
pub fn set(t: Theme) {
    unsafe { THEME = Some(t) };
}

/// Ultimo tamaño aplicado del desenfoque de una ventana: (ancho, desplazamiento, alto).
pub type BlurCache = (i32, i32, i32);

#[repr(C)]
struct AccentPolicy {
    state: u32, // 0 apagado, 3 desenfoque de fondo, 4 acrilico
    flags: u32,
    color: u32,
    anim: u32,
}

#[repr(C)]
struct CompAttr {
    attrib: u32, // 19 = politica de acento
    data: *mut std::ffi::c_void,
    size: usize,
}

type SetCompAttr = unsafe extern "system" fn(HWND, *mut CompAttr) -> i32;

/// Desenfoque de fondo del propio Windows detras de la ventana. SetWindowCompositionAttribute no esta
/// documentada pero la usan Windows Terminal y la mayoria de los temas de Windows 10/11; si falta, la
/// card queda solo translucida, sin desenfoque. La forma redondeada se recorta con una region de ventana.
/// `x0` desplaza la forma (la card de costado muestra solo un tramo de su contenido).
pub unsafe fn blur(hwnd: HWND, cache: &mut BlurCache, w: i32, full_w: i32, x0: i32, h: i32, rad: i32) {
    let key = (w, x0, h);
    if *cache == key {
        return;
    }
    *cache = key;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    let f = GetProcAddress(GetModuleHandleA(b"user32.dll\0".as_ptr()), b"SetWindowCompositionAttribute\0".as_ptr());
    let rgn = CreateRoundRectRgn(x0, 0, x0 + full_w + 1, h + 1, rad * 2, rad * 2);
    // Con region, Windows se queda con ella (no se libera).
    SetWindowRgn(hwnd, rgn, 1);
    if let Some(f) = f {
        let f: SetCompAttr = std::mem::transmute(f);
        let mut ap = AccentPolicy { state: 3, flags: 0, color: 0, anim: 0 };
        let mut d = CompAttr { attrib: 19, data: (&mut ap as *mut AccentPolicy).cast(), size: std::mem::size_of::<AccentPolicy>() };
        f(hwnd, &mut d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_accent_keeps_contrast_on_light_backgrounds() {
        let accents = [0x6366f1u32, 0x8b5cf6, 0xec4899, 0xf97316, 0x22c55e, 0x06b6d4, 0xeab308];
        for hexrgb in accents {
            let acc = rgb((hexrgb >> 16) as u8, (hexrgb >> 8) as u8, hexrgb as u8);
            let t = Theme::new(rgb(0xf4, 0xf1, 0xea), acc);
            assert!(contrast(t.lit, t.tab) >= 3.0, "{} sobre tecla: {}", hex(acc), contrast(t.lit, t.tab));
            assert!(contrast(t.lit, t.led_off) >= 3.0, "{} contra apagado: {}", hex(acc), contrast(t.lit, t.led_off));
            for bg in [rgb(0x0d, 0x0f, 0x14), rgb(0x1f, 0x1f, 0x22), rgb(0, 0, 0)] {
                let dark = Theme::new(bg, acc);
                assert!(contrast(dark.lit, dark.tab) >= 3.0, "{} en oscuro: {}", hex(acc), contrast(dark.lit, dark.tab));
                assert!(contrast(dark.lit, dark.accent) < 1.6, "en oscuro el acento casi no cambia: {}", hex(acc));
            }
        }
    }

    #[test]
    fn dark_theme_tokens() {
        let t = Theme::new(rgb(0x1f, 0x1f, 0x22), rgb(0xf9, 0x73, 0x16));
        assert!(!t.light);
        assert_eq!(t.glow_a, 0.70);
        assert_eq!(t.inset_a, 0.60);
        assert!(lum(t.well) < lum(t.bg));
        assert!(lum(t.led_off) > lum(t.bg) && lum(t.led_off) < lum(t.ink));
        assert!(lum(t.top) > lum(t.bg));
    }

    #[test]
    fn light_theme_tokens() {
        let t = Theme::new(rgb(0xf4, 0xf1, 0xea), rgb(0x8b, 0x5c, 0xf6));
        assert!(t.light);
        assert_eq!(t.glow_a, 0.35);
        assert_eq!(t.inset_a, 0.18);
        assert!(lum(t.well) < lum(t.bg) && lum(t.bg) - lum(t.well) < 0.1);
        assert!(lum(t.led_off) < lum(t.bg));
    }

    #[test]
    fn dark_accent_on_dark_background_gets_lifted() {
        let t = Theme::new(rgb(0x15, 0x15, 0x15), rgb(0x3a, 0x2a, 0x55));
        assert!(contrast(t.lit, t.tab) >= 3.0, "lit sobre tecla: {}", contrast(t.lit, t.tab));
        assert!(contrast(t.lit, t.led_off) >= 3.0);
        assert!(lum(t.lit) > lum(t.accent), "se aclara");
    }

    #[test]
    fn fatuo_style_roundtrips() {
        let mut t = Theme::new(rgb(0, 0, 0), rgb(0x63, 0x66, 0xf1));
        t.style = Style::Fatuo;
        assert_eq!(t.to_json()["estilo"], "fatuo");
        assert_eq!(Theme::from_json(&t.to_json()).style, Style::Fatuo);
        assert_eq!(Style::ALL.len(), 3);
        assert_eq!(Style::ALL.iter().position(|&s| s == Style::Fatuo), Some(2));
    }

    #[test]
    fn json_roundtrip_and_defaults() {
        let d = Theme::from_json(&json!({}));
        assert_eq!(d.style, Style::Instrumento);
        assert!(!d.audio);
        let mut t = Theme::new(rgb(0, 0, 0), rgb(0x63, 0x66, 0xf1));
        t.style = Style::Minimal;
        t.audio = true;
        let back = Theme::from_json(&t.to_json());
        assert_eq!(back.style, Style::Minimal);
        assert!(back.audio);
        assert_eq!(back.accent, t.accent);
    }
}
