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
    let fatuo = t.style == Style::Fatuo;
    if fatuo {
        // Borde teñido por la luz y fondo que arriba tira al acento.
        cv.round(0.0, 0.0, w, h, rad, theme::mix(t.hairline, t.accent, 0.3));
        cv.vgrad(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, theme::mix(t.bg, t.accent, if t.light { 0.07 } else { 0.11 }), t.bg);
    } else {
        cv.vgrad(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, t.top, t.bg);
    }
    let line = if fatuo { theme::mix(t.accent, rgb(255, 255, 255), 0.45) } else { t.accent };
    let (l, r) = (rad, w - rad);
    let n = (r - l).max(1.0) as i32;
    for i in 0..n {
        let x = l + i as f32;
        let k = 1.0 - ((x - w / 2.0).abs() / ((r - l) / 2.0)).powi(2);
        cv.round_a(x, 1.0, x + 1.0, 2.0, 0.0, line, t.glow_a * k.max(0.0));
    }
}

pub fn button(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, rad: f32, st: St) {
    if t.style == Style::Minimal {
        let c = if st.active { t.accent } else { theme::mix(t.tab, t.hover, st.hover) };
        cv.round(l, top, r, b, rad, c);
        return;
    }
    if t.style == Style::Fatuo {
        // Vidrio: casi transparente, con un filo de luz que se enciende al pasar y al elegir.
        let rad = (rad * 1.5).min((b - top) / 2.0);
        let k = if st.active { 1.0 } else { 0.0 } + st.flash;
        if k > 0.0 {
            let spr = g.get((r - l).round() as i32, (b - top).round() as i32, rad, (8.0 * s).round() as i32);
            cv.glow(spr, l, top, t.lit, t.glow_a * 0.5 * k.min(1.5));
        }
        let base = if st.active { theme::mix(t.card, t.lit, 0.18) } else { theme::mix(t.card, t.hover, st.hover) };
        cv.round(l, top, r, b, rad, base);
        cv.ring(l, top, r, b, rad, 1.0, t.lit, if st.active { 0.9 } else { 0.14 + 0.25 * st.hover });
        return;
    }
    let base = if st.active { theme::mix(t.tab, t.lit, 0.14) } else { theme::mix(t.tab, t.hover, st.hover) };
    let k = if st.active { 1.0 } else { 0.0 } + st.flash;
    if k > 0.0 {
        let spr = g.get((r - l).round() as i32, (b - top).round() as i32, rad, (8.0 * s).round() as i32);
        cv.glow(spr, l, top, t.lit, t.glow_a * 0.55 * k.min(1.5));
    }
    cv.round(l, top, r, b, rad, base);
    cv.round_a(l + rad, top, r - rad, top + 1.0, 0.0, t.ink, 0.07); // reflejo
    cv.round_a(l + rad, b, r - rad, b + 1.0, 0.0, rgb(0, 0, 0), 0.5); // sombra
    if st.active {
        cv.ring(l, top, r, b, rad, 1.0, t.lit, 1.0);
    }
}

/// Barra de `n` segmentos; `lit` encendidos (fraccional para la cascada); `boot` 0..1 los prende en orden.
pub fn segments(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, n: usize, lit: f32, c: u32, boot: f32) {
    let c = if c == t.accent { t.lit } else { c }; // en fondos claros, el acento con contraste
    if t.style == Style::Fatuo {
        return if n <= 7 { dots(cv, t, g, s, l, top, r, b, n, lit, c, boot) } else { beam(cv, t, g, s, l, top, r, b, lit / n as f32, c, boot) };
    }
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
    if t.style == Style::Fatuo {
        // Una chispa redonda.
        cv.dot(cx, cy, size / 2.0, t.led_off);
        if on > 0.0 {
            let spr = g.get(size.round() as i32, size.round() as i32, size / 2.0, (7.0 * s).round() as i32);
            cv.glow(spr, l, top, c, t.glow_a * 1.1 * on);
            cv.round_a(l, top, l + size, top + size, size / 2.0, c, on.min(1.0));
        }
        return;
    }
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
    if t.style == Style::Fatuo {
        cv.round(l, top, r, b, rad * 1.4, theme::mix(t.bg, t.ink, 0.04));
        cv.ring(l, top, r, b, rad * 1.4, 1.0, t.accent, 0.14);
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
    if t.style == Style::Fatuo {
        // Pildora; prendida se tiñe y brilla.
        let rad = (b - top) / 2.0;
        if on {
            let spr = g.get((r - l).round() as i32, (b - top).round() as i32, rad, (6.0 * s).round() as i32);
            cv.glow(spr, l, top, t.lit, t.glow_a * 0.45);
        }
        let track = if on { theme::mix(t.tab, t.lit, 0.45) } else { theme::mix(t.hover, t.tab, 1.0 - st.hover) };
        cv.round(l, top, r, b, rad, track);
        let k = rad - 2.5 * s;
        let knob = if on { theme::mix(t.lit, rgb(255, 255, 255), 0.65) } else { t.dim };
        cv.dot(if on { r - rad } else { l + rad }, (top + b) / 2.0, k, knob);
        return;
    }
    button(cv, t, g, s, l, top, r, b, 4.0 * s, St { active: false, ..st });
    led(cv, t, g, s, (l + r) / 2.0, (top + b) / 2.0, 6.0 * s, if on { 1.0 } else { 0.0 }, t.accent);
}

/// Fuego fatuo con pocos niveles (ambientes, vumetro): puntos que se encienden.
fn dots(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, n: usize, lit: f32, c: u32, boot: f32) {
    let cell = (r - l) / n as f32;
    let rad = ((b - top) / 2.0).min(cell / 2.0 - 1.0).max(1.0);
    let cy = (top + b) / 2.0;
    for i in 0..n {
        let cx = l + (i as f32 + 0.5) * cell;
        let booted = boot >= (i as f32 + 1.0) / n as f32;
        let on = if booted { (lit - i as f32).clamp(0.0, 1.0) } else { 0.0 };
        cv.dot(cx, cy, rad, t.led_off);
        if on > 0.0 {
            let d = (2.0 * rad).round();
            let spr = g.get(d as i32, d as i32, rad, (5.0 * s).round() as i32);
            cv.glow(spr, cx - rad, cy - rad, c, t.glow_a * 0.8 * on);
            cv.round_a(cx - rad, cy - rad, cx + rad, cy + rad, rad, c, on);
        }
    }
}

/// Fuego fatuo con muchos niveles (progreso, volumen): un haz fino que se aclara hacia la punta y brilla.
/// `f` 0..1 es lo encendido; `boot` lo recorta mientras se enciende la card.
fn beam(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, f: f32, c: u32, boot: f32) {
    let th = (b - top).min(3.0 * s).max(1.0);
    let cy = (top + b) / 2.0;
    let (y0, y1) = (cy - th / 2.0, cy + th / 2.0);
    cv.round(l, y0, r, y1, th / 2.0, t.led_off);
    let x = l + (r - l) * f.min(boot).clamp(0.0, 1.0);
    if x - l < 1.0 {
        return;
    }
    // Brillo en tramos de ancho fijo: el desenfoque es lineal, asi la suma es el brillo del haz entero y el
    // cache de sprites queda chico (un ancho fijo y sus restos).
    let step = (12.0 * s).round().max(4.0);
    let pad = (5.0 * s).round() as i32;
    let mut cx = l;
    while cx < x {
        let w = (x - cx).min(step);
        let spr = g.get(w.round().max(1.0) as i32, th.round().max(1.0) as i32, 0.0, pad);
        cv.glow(spr, cx, y0, c, t.glow_a * 0.35);
        cx += step;
    }
    cv.round(l, y0, x, y1, th / 2.0, c);
    let light = theme::mix(c, rgb(255, 255, 255), 0.55);
    let span = (r - l).max(1.0);
    let mut px = l + th / 2.0;
    while px < x - th / 2.0 {
        cv.round_a(px, y0, px + 1.0, y1, 0.0, light, (px - l) / span * 0.8);
        px += 1.0;
    }
    // Chispa en la punta.
    cv.round_a(x - th, cy - th * 0.6, x, cy + th * 0.6, th * 0.6, light, 0.9);
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

    fn fatuo() -> Theme {
        let mut t = Theme::new(rgb(0x0d, 0x0f, 0x14), rgb(0x63, 0x66, 0xf1));
        t.style = Style::Fatuo;
        t
    }

    #[test]
    fn fatuo_led_is_round() {
        let t = fatuo();
        let mut cv = unsafe { Canvas::new(30, 30) };
        cv.pixels().fill(theme::px(t.bg));
        let mut g = glow::Cache::default();
        led(&mut cv, &t, &mut g, 1.0, 15.0, 15.0, 10.0, 1.0, t.accent);
        assert_eq!(at(&mut cv, 15, 15), theme::px(t.lit), "centro encendido");
        let corner = at(&mut cv, 10, 10);
        assert_ne!(corner, theme::px(t.lit), "la esquina del cuadrado no es LED");
    }

    #[test]
    fn fatuo_few_segments_are_dots_many_are_a_bar() {
        let t = fatuo();
        let mut g = glow::Cache::default();
        // 3 niveles: puntos separados (entre dos puntos no hay acento pleno).
        let mut cv = unsafe { Canvas::new(48, 12) };
        cv.pixels().fill(theme::px(t.bg));
        segments(&mut cv, &t, &mut g, 1.0, 0.0, 2.0, 48.0, 10.0, 3, 3.0, t.accent, 1.0);
        assert_ne!(at(&mut cv, 16, 6), theme::px(t.lit), "hueco entre puntos");
        // 14 segmentos: barra continua (sin huecos dentro de lo encendido).
        let mut cv = unsafe { Canvas::new(140, 12) };
        cv.pixels().fill(theme::px(t.bg));
        segments(&mut cv, &t, &mut g, 1.0, 0.0, 4.0, 140.0, 8.0, 14, 14.0, t.accent, 1.0);
        let gap = 10; // donde Instrumento tendria el hueco entre segmentos
        assert!(at(&mut cv, gap, 6) != theme::px(t.bg) && at(&mut cv, gap, 6) != theme::px(t.led_off), "barra continua");
    }

    #[test]
    fn fatuo_card_has_tinted_top() {
        let t = fatuo();
        let mut cv = unsafe { Canvas::new(60, 60) };
        card(&mut cv, &t, 60.0, 60.0, 10.0);
        let top = at(&mut cv, 30, 8);
        let bottom = at(&mut cv, 30, 52);
        assert_ne!(top, bottom, "gradiente");
        assert!((top & 0xff) > (bottom & 0xff), "arriba tira al acento (azul)");
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
    fn active_button_ring_uses_contrasted_accent() {
        let t = Theme::new(rgb(0xf4, 0xf1, 0xea), rgb(0xea, 0xb3, 0x08));
        let mut cv = unsafe { Canvas::new(60, 40) };
        cv.pixels().fill(theme::px(t.bg));
        let mut g = glow::Cache::default();
        button(&mut cv, &t, &mut g, 1.0, 10.0, 10.0, 50.0, 30.0, 4.0, St { active: true, ..St::default() });
        assert_eq!(at(&mut cv, 30, 10), theme::px(t.lit));
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
