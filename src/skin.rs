// Componentes de la card en sus dos estilos. Minimal reproduce la card de siempre; Instrumento es el
// equipo de audio con brillos del acento. Mismo lugar y tamaño en los dos: cambia solo como se pinta.
// Reciben el tema como parametro (quien llama pasa th()) para poder probarlos sin estado global.

use super::{glow, rgb, Canvas, GREEN};
use crate::theme::{self, Style, Theme};

#[derive(Clone, Copy, Default)]
pub struct St {
    pub hover: f32,  // 0..1 (animado)
    pub active: bool, // elegido: play, pestaña, chip
    pub flash: f32,  // 0..1, destello al tocar
}

/// Fondo de la card: gradiente suave, anillo finito y linea de brillo arriba (mas fuerte en el centro).
pub fn card(cv: &mut Canvas, t: &Theme, w: f32, h: f32, rad: f32) {
    cv.round(0.0, 0.0, w, h, rad, t.hairline);
    if t.style == Style::Minimal {
        cv.round(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, t.bg);
        return;
    }
    cv.vgrad(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, t.top, t.bg);
    let (l, r) = (rad, w - rad);
    let n = (r - l).max(1.0) as i32;
    for i in 0..n {
        let x = l + i as f32;
        let k = 1.0 - ((x - w / 2.0).abs() / ((r - l) / 2.0)).powi(2);
        cv.round_a(x, 1.0, x + 1.0, 2.0, 0.0, t.accent, t.glow_a * k.max(0.0));
    }
}

pub fn button(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, rad: f32, st: St) {
    if t.style == Style::Minimal {
        let c = if st.active { t.accent } else { theme::mix(t.tab, t.hover, st.hover) };
        cv.round(l, top, r, b, rad, c);
        return;
    }
    let base = if st.active { theme::mix(t.tab, t.accent, 0.14) } else { theme::mix(t.tab, t.hover, st.hover) };
    let k = if st.active { 1.0 } else { 0.0 } + st.flash;
    if k > 0.0 {
        let spr = g.get((r - l).round() as i32, (b - top).round() as i32, rad, (8.0 * s).round() as i32);
        cv.glow(spr, l, top, t.accent, t.glow_a * 0.55 * k.min(1.5));
    }
    cv.round(l, top, r, b, rad, base);
    cv.round_a(l + rad, top, r - rad, top + 1.0, 0.0, t.ink, 0.07); // reflejo
    cv.round_a(l + rad, b, r - rad, b + 1.0, 0.0, rgb(0, 0, 0), 0.5); // sombra
    if st.active {
        cv.ring(l, top, r, b, rad, 1.0, t.accent, 1.0);
    }
}

/// Barra de `n` segmentos; `lit` encendidos (fraccional para la cascada); `boot` 0..1 los prende en orden.
pub fn segments(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, n: usize, lit: f32, c: u32, boot: f32) {
    let c = if c == t.accent { t.lit } else { c }; // en fondos claros, el acento con contraste
    let gap = (2.0 * s).max(1.0);
    let sw = (r - l - gap * (n as f32 - 1.0)) / n as f32;
    for i in 0..n {
        let x = l + i as f32 * (sw + gap);
        let booted = boot >= (i as f32 + 1.0) / n as f32;
        let on = if booted { (lit - i as f32).clamp(0.0, 1.0) } else { 0.0 };
        cv.round(x, top, x + sw, b, 0.0, t.led_off);
        if on > 0.0 {
            let spr = g.get(sw.round() as i32, (b - top).round() as i32, 0.0, (4.0 * s).round() as i32);
            cv.glow(spr, x, top, c, t.glow_a * 0.6 * on);
            cv.round_a(x, top, x + sw, b, 0.0, c, on);
        }
    }
}

/// LED cuadrado; `on` 0..1 es su brillo (respira), `c` el color encendido.
pub fn led(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, cx: f32, cy: f32, size: f32, on: f32, c: u32) {
    let c = if c == t.accent { t.lit } else { c };
    let (l, top) = (cx - size / 2.0, cy - size / 2.0);
    cv.round(l, top, l + size, top + size, 2.0 * s, t.led_off);
    if on > 0.0 {
        let spr = g.get(size.round() as i32, size.round() as i32, 2.0 * s, (6.0 * s).round() as i32);
        cv.glow(spr, l, top, c, t.glow_a * on);
        cv.round_a(l, top, l + size, top + size, 2.0 * s, c, on.min(1.0));
    }
}

/// Zona hundida (buscador, cajas de filas).
pub fn well(cv: &mut Canvas, t: &Theme, l: f32, top: f32, r: f32, b: f32, rad: f32) {
    if t.style == Style::Minimal {
        cv.round(l, top, r, b, rad, t.card);
        return;
    }
    cv.round(l, top, r, b, rad, t.hairline);
    cv.round(l + 1.0, top + 1.0, r - 1.0, b - 1.0, (rad - 1.0).max(0.0), t.well);
    cv.round_a(l + rad, top + 1.0, r - rad, top + 2.0, 0.0, rgb(0, 0, 0), t.inset_a);
}

/// Interruptor: en Minimal la pildora de siempre; en Instrumento un boton chico con un LED.
pub fn switch(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, on: bool, st: St) {
    if t.style == Style::Minimal {
        cv.round(l, top, r, b, (b - top) / 2.0, if on { GREEN } else { t.hover });
        let k = (b - top) / 2.0 - 2.5 * s;
        cv.dot(if on { r - (b - top) / 2.0 } else { l + (b - top) / 2.0 }, (top + b) / 2.0, k, t.ink);
        return;
    }
    button(cv, t, g, s, l, top, r, b, 4.0 * s, St { active: false, ..st });
    led(cv, t, g, s, (l + r) / 2.0, (top + b) / 2.0, 6.0 * s, if on { 1.0 } else { 0.0 }, t.accent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn themes() -> (Theme, Theme) {
        let inst = Theme::new(rgb(0x1f, 0x1f, 0x22), rgb(0xf9, 0x73, 0x16));
        let mut min = inst.clone();
        min.style = Style::Minimal;
        (inst, min)
    }

    fn at(cv: &mut Canvas, x: i32, y: i32) -> u32 {
        let w = cv.w;
        cv.pixels()[(y * w + x) as usize]
    }

    #[test]
    fn minimal_button_is_flat_tab() {
        let (_, t) = themes();
        let mut cv = unsafe { Canvas::new(40, 30) };
        let mut g = glow::Cache::default();
        button(&mut cv, &t, &mut g, 1.0, 0.0, 0.0, 40.0, 30.0, 4.0, St::default());
        assert_eq!(at(&mut cv, 20, 15), theme::px(t.tab));
    }

    #[test]
    fn active_instrument_button_has_accent_ring() {
        let (t, _) = themes();
        let mut cv = unsafe { Canvas::new(60, 40) };
        cv.pixels().fill(theme::px(t.bg));
        let mut g = glow::Cache::default();
        button(&mut cv, &t, &mut g, 1.0, 10.0, 10.0, 50.0, 30.0, 4.0, St { active: true, ..St::default() });
        assert_eq!(at(&mut cv, 30, 10), theme::px(t.accent)); // borde de arriba
        assert_ne!(at(&mut cv, 30, 4), theme::px(t.bg)); // brillo afuera
    }

    #[test]
    fn light_theme_segments_use_contrasted_accent() {
        let t = Theme::new(rgb(0xf4, 0xf1, 0xea), rgb(0xea, 0xb3, 0x08));
        assert_ne!(t.lit, t.accent);
        let mut cv = unsafe { Canvas::new(46, 10) };
        let mut g = glow::Cache::default();
        segments(&mut cv, &t, &mut g, 1.0, 0.0, 2.0, 46.0, 8.0, 4, 4.0, t.accent, 1.0);
        assert_eq!(at(&mut cv, 5, 5), theme::px(t.lit));
    }

    #[test]
    fn segments_light_only_lit_count() {
        let (t, _) = themes();
        let mut cv = unsafe { Canvas::new(46, 10) };
        let mut g = glow::Cache::default();
        segments(&mut cv, &t, &mut g, 1.0, 0.0, 2.0, 46.0, 8.0, 4, 2.0, t.accent, 1.0);
        assert_eq!(at(&mut cv, 5, 5), theme::px(t.accent));
        assert_eq!(at(&mut cv, 41, 5), theme::px(t.led_off));
    }

    #[test]
    fn boot_holds_segments_off() {
        let (t, _) = themes();
        let mut cv = unsafe { Canvas::new(46, 10) };
        let mut g = glow::Cache::default();
        segments(&mut cv, &t, &mut g, 1.0, 0.0, 2.0, 46.0, 8.0, 4, 4.0, t.accent, 0.3);
        assert_eq!(at(&mut cv, 5, 5), theme::px(t.accent)); // 1/4 <= 0,3
        assert_eq!(at(&mut cv, 41, 5), theme::px(t.led_off)); // todavia apagado
    }

    #[test]
    fn led_off_and_on() {
        let (t, _) = themes();
        let mut g = glow::Cache::default();
        let mut cv = unsafe { Canvas::new(20, 20) };
        led(&mut cv, &t, &mut g, 1.0, 10.0, 10.0, 7.0, 0.0, t.accent);
        assert_eq!(at(&mut cv, 10, 10), theme::px(t.led_off));
        led(&mut cv, &t, &mut g, 1.0, 10.0, 10.0, 7.0, 1.0, t.accent);
        assert_eq!(at(&mut cv, 10, 10), theme::px(t.accent));
    }

    #[test]
    fn well_uses_well_color_in_instrument_and_card_in_minimal() {
        let (ti, tm) = themes();
        let mut cv = unsafe { Canvas::new(40, 30) };
        well(&mut cv, &ti, 0.0, 0.0, 40.0, 30.0, 6.0);
        assert_eq!(at(&mut cv, 20, 20), theme::px(ti.well));
        well(&mut cv, &tm, 0.0, 0.0, 40.0, 30.0, 6.0);
        assert_eq!(at(&mut cv, 20, 20), theme::px(tm.card));
    }
}
