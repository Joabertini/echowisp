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
    let s = rad / super::RADIUS;
    let glow = (t.glow_a / 0.7).min(1.0); // 1.0 en oscuros, menos en claros
    let mut line = t.accent;
    match t.style {
        Style::Fatuo => {
            // Borde teñido por la luz, fondo que arriba tira al acento, motas sueltas y un glow que recorre
            // todo el perimetro (en vez de una luz fija detras del play, que no tiene sentido en el panel).
            cv.round(0.0, 0.0, w, h, rad, theme::mix(t.hairline, t.accent, 0.3));
            cv.vgrad(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, theme::mix(t.bg, t.accent, if t.light { 0.07 } else { 0.11 }), t.bg);
            cv.border_glow(0.0, 0.0, w, h, rad, 26.0 * s, t.lit, 0.30 * glow);
            for (fx, fy) in [(0.18, 0.1), (0.84, 0.2), (0.72, 0.86), (0.1, 0.66), (0.92, 0.55)] {
                cv.round_a(w * fx - 1.3 * s, h * fy - 1.3 * s, w * fx + 1.3 * s, h * fy + 1.3 * s, 1.3 * s, theme::mix(t.lit, rgb(255, 255, 255), 0.5), 0.35 * glow);
            }
            line = theme::mix(t.accent, rgb(255, 255, 255), 0.45);
        }
        Style::Brasa => {
            // Calido desde abajo, como una pieza metalica ahumada (sin grano: se veia como ruido de camara).
            cv.round(0.0, 0.0, w, h, rad, theme::mix(t.hairline, t.lit, 0.35));
            cv.vgrad(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, theme::mix(t.bg, t.ink, 0.04), theme::mix(t.bg, rgb(0, 0, 0), 0.25));
            cv.radial(w / 2.0, h * 1.02, w * 0.8, h * 0.6, t.lit, 0.32 * glow);
            line = theme::mix(t.lit, rgb(255, 255, 255), 0.3);
        }
        Style::Neon => {
            // Fondo plano con una rejilla tenue y el borde encendido.
            cv.round(0.0, 0.0, w, h, rad, theme::mix(t.bg, t.lit, 0.5));
            cv.round(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, t.bg);
            let step = (16.0 * s).round().max(4.0);
            let mut x = step;
            while x < w {
                cv.round_a(x, 1.0, x + 1.0, h - 1.0, 0.0, t.lit, 0.06);
                x += step;
            }
            let mut y = step;
            while y < h {
                cv.round_a(1.0, y, w - 1.0, y + 1.0, 0.0, t.lit, 0.06);
                y += step;
            }
            cv.ring(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, 3.0 * s, t.lit, 0.07);
            line = t.lit;
        }
        _ => cv.vgrad(1.0, 1.0, w - 1.0, h - 1.0, rad - 1.0, t.top, t.bg),
    }
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
    if t.style == Style::Brasa {
        // Pieza facetada: relleno ahumado, filo calido y, elegida, un bloque encendido.
        let cut = (5.0 * s).min((b - top) / 3.0).max(1.0);
        let k = if st.active { 1.0 } else { 0.0 } + st.flash;
        if k > 0.0 {
            let spr = g.get((r - l).round() as i32, (b - top).round() as i32, cut, (8.0 * s).round() as i32);
            cv.glow(spr, l, top, t.lit, t.glow_a * 0.6 * k.min(1.5));
        }
        if st.active {
            cv.bevel_a(l, top, r, b, cut, theme::mix(t.lit, rgb(0, 0, 0), 0.12), 1.0);
            cv.round_a(l + cut, top + 1.0, r - cut, top + 2.0, 0.0, rgb(255, 255, 255), 0.25);
        } else {
            cv.bevel_a(l, top, r, b, cut, theme::mix(t.tab, t.lit, 0.05 + 0.08 * st.hover), 1.0);
            cv.bevel_ring(l, top, r, b, cut, 1.0, t.lit, 0.3 + 0.4 * st.hover + 0.4 * st.flash.min(1.0));
            cv.round_a(l + cut, top + 1.0, r - cut, top + 2.0, 0.0, t.ink, 0.07);
        }
        return;
    }
    if t.style == Style::Neon {
        // Contorno hueco que se enciende entero; elegido, doble y con brillo.
        let rad = (3.0 * s).min((b - top) / 2.0);
        let k = if st.active { 1.0 } else { 0.0 } + st.flash;
        let gl = 0.25 * st.hover + 0.5 * k;
        if gl > 0.0 {
            let spr = g.get((r - l).round() as i32, (b - top).round() as i32, rad, (7.0 * s).round() as i32);
            cv.glow(spr, l, top, t.lit, t.glow_a * gl.min(1.2));
        }
        if st.active {
            cv.round_a(l, top, r, b, rad, t.lit, 0.12);
        }
        cv.ring(l, top, r, b, rad, if st.active { 2.0 * s.max(1.0) } else { 1.0 }, t.lit, if st.active { 1.0 } else { (0.45 + 0.45 * st.hover + 0.4 * st.flash).min(1.0) });
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
    if t.style == Style::Brasa && n > 7 {
        return wick(cv, t, g, s, l, top, r, b, lit / n as f32, c, boot);
    }
    if matches!(t.style, Style::Brasa | Style::Neon) {
        return cells(cv, t, g, s, l, top, r, b, n, lit, c, boot);
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
    if t.style == Style::Brasa {
        // Brasa facetada.
        let cut = size * 0.3;
        cv.bevel_a(l, top, l + size, top + size, cut, t.led_off, 1.0);
        if on > 0.0 {
            let spr = g.get(size.round() as i32, size.round() as i32, cut, (6.0 * s).round() as i32);
            cv.glow(spr, l, top, c, t.glow_a * on);
            cv.bevel_a(l, top, l + size, top + size, cut, c, on.min(1.0));
        }
        return;
    }
    if t.style == Style::Neon {
        // Cuadradito hueco que se llena.
        cv.ring(l, top, l + size, top + size, s, 1.0, t.led_off, 1.0);
        if on > 0.0 {
            let spr = g.get(size.round() as i32, size.round() as i32, s, (6.0 * s).round() as i32);
            cv.glow(spr, l, top, c, t.glow_a * on);
            cv.round_a(l, top, l + size, top + size, s, c, on.min(1.0));
        }
        return;
    }
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
    if t.style == Style::Brasa {
        let cut = (rad * 0.9).min((b - top) / 3.0).max(1.0);
        cv.bevel_a(l, top, r, b, cut, theme::mix(t.bg, rgb(0, 0, 0), 0.45), 1.0);
        cv.bevel_ring(l, top, r, b, cut, 1.0, t.lit, 0.22);
        return;
    }
    if t.style == Style::Neon {
        cv.ring(l, top, r, b, (rad * 0.4).max(1.0), 1.0, t.lit, 0.3);
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
    if t.style == Style::Brasa {
        let hh = b - top;
        let cut = hh * 0.3;
        if on {
            let spr = g.get((r - l).round() as i32, hh.round() as i32, cut, (6.0 * s).round() as i32);
            cv.glow(spr, l, top, t.lit, t.glow_a * 0.4);
        }
        cv.bevel_a(l, top, r, b, cut, if on { theme::mix(t.tab, t.lit, 0.35) } else { theme::mix(t.tab, t.hover, st.hover) }, 1.0);
        cv.bevel_ring(l, top, r, b, cut, 1.0, t.lit, if on { 0.8 } else { 0.3 });
        let k = hh - 6.0 * s;
        let x = if on { r - 3.0 * s - k } else { l + 3.0 * s };
        cv.bevel_a(x, top + 3.0 * s, x + k, top + 3.0 * s + k, k * 0.3, if on { theme::mix(t.lit, rgb(255, 255, 255), 0.65) } else { t.dim }, 1.0);
        return;
    }
    if t.style == Style::Neon {
        let hh = b - top;
        let rad = (hh / 2.5).max(1.0);
        cv.ring(l, top, r, b, rad, 1.0, t.lit, if on { 1.0 } else { 0.35 });
        let k = hh - 7.0 * s;
        let x = if on { r - 3.5 * s - k } else { l + 3.5 * s };
        if on {
            let spr = g.get(k.round() as i32, k.round() as i32, s, (5.0 * s).round() as i32);
            cv.glow(spr, x, top + 3.5 * s, t.lit, t.glow_a * 0.8);
        }
        cv.round_a(x, top + 3.5 * s, x + k, top + 3.5 * s + k, s, if on { t.lit } else { t.dim }, 1.0);
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

/// Color del glifo del play segun el estilo (sobre su fondo: tecla, disco de luz o bloque encendido).
pub fn play_glyph(t: &Theme) -> u32 {
    match t.style {
        Style::Minimal => t.bg,
        Style::Instrumento => t.lit,
        Style::Fatuo => theme::mix(t.lit, rgb(0, 0, 0), 0.78),
        Style::Brasa => {
            let fill = theme::mix(t.lit, rgb(0, 0, 0), 0.12);
            if theme::contrast(rgb(0, 0, 0), fill) >= theme::contrast(rgb(255, 255, 255), fill) { rgb(0, 0, 0) } else { rgb(255, 255, 255) }
        }
        Style::Neon => t.ink,
    }
}

/// Color del titulo: Brasa y Neon lo tiñen un poco con el acento.
pub fn title_color(t: &Theme) -> u32 {
    match t.style {
        Style::Brasa => theme::mix(t.ink, t.lit, 0.2),
        Style::Neon => theme::mix(t.ink, t.lit, 0.4),
        _ => t.ink,
    }
}

/// Celdas de Brasa (facetadas) y Neon (huecas): `n` casillas, `lit` encendidas.
fn cells(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, n: usize, lit: f32, c: u32, boot: f32) {
    let gap = (2.0 * s).max(1.0);
    let sw = (r - l - gap * (n as f32 - 1.0)) / n as f32;
    let brasa = t.style == Style::Brasa;
    let cut = (1.5 * s).min(sw / 3.0);
    for i in 0..n {
        let x = l + i as f32 * (sw + gap);
        let booted = boot >= (i as f32 + 1.0) / n as f32;
        let on = if booted { (lit - i as f32).clamp(0.0, 1.0) } else { 0.0 };
        if brasa {
            cv.bevel_a(x, top, x + sw, b, cut, t.led_off, 1.0);
        } else {
            cv.ring(x, top, x + sw, b, 0.0, 1.0, t.led_off, 1.0);
        }
        if on > 0.0 {
            let spr = g.get(sw.round() as i32, (b - top).round() as i32, if brasa { cut } else { 0.0 }, (4.0 * s).round() as i32);
            cv.glow(spr, x, top, c, t.glow_a * 0.6 * on);
            if brasa {
                cv.bevel_a(x, top, x + sw, b, cut, c, on);
            } else {
                cv.round_a(x, top, x + sw, b, 0.0, c, on * 0.85);
                cv.ring(x, top, x + sw, b, 0.0, 1.0, c, on);
            }
        }
    }
}

/// Mecha de Brasa (progreso, volumen): la parte encendida pasa de acento a amarillo hacia la punta, que arde.
fn wick(cv: &mut Canvas, t: &Theme, g: &mut glow::Cache, s: f32, l: f32, top: f32, r: f32, b: f32, f: f32, c: u32, boot: f32) {
    let th = (b - top).min(4.0 * s).max(1.0);
    let cy = (top + b) / 2.0;
    let (y0, y1) = (cy - th / 2.0, cy + th / 2.0);
    cv.round(l, y0, r, y1, th / 2.0, t.led_off);
    let x = l + (r - l) * f.min(boot).clamp(0.0, 1.0);
    if x - l < 1.0 {
        return;
    }
    let step = (12.0 * s).round().max(4.0);
    let pad = (5.0 * s).round() as i32;
    let mut gx = l;
    while gx < x {
        let w = (x - gx).min(step);
        let spr = g.get(w.round().max(1.0) as i32, th.round().max(1.0) as i32, 0.0, pad);
        cv.glow(spr, gx, y0, c, t.glow_a * 0.4);
        gx += step;
    }
    let hot = rgb(255, 200, 90);
    let span = (x - l).max(1.0);
    let mut px = l;
    while px < x {
        let k = ((px - l) / span).powf(1.2) * 0.85;
        cv.round_a(px, y0, px + 1.0, y1, 0.0, theme::mix(c, hot, k), 1.0);
        px += 1.0;
    }
    cv.round_a(x - th, cy - th * 0.8, x + th * 0.2, cy + th * 0.8, th * 0.8, theme::mix(c, rgb(255, 255, 255), 0.7), 0.9);
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
        let mut cv = unsafe { Canvas::new(60, 400) }; // alta: abajo ya no llega la luz del play
        card(&mut cv, &t, 60.0, 400.0, 10.0);
        let top = at(&mut cv, 30, 8);
        let bottom = at(&mut cv, 30, 392);
        assert_ne!(top, bottom, "gradiente");
        assert!((top & 0xff) > (bottom & 0xff), "arriba tira al acento (azul)");
    }

    fn styled(st: Style) -> Theme {
        let mut t = Theme::new(rgb(0x14, 0x10, 0x10), rgb(0xff, 0x5a, 0x1a));
        t.style = st;
        t
    }

    fn sheet(t: &Theme, w: i32, h: i32) -> Canvas {
        let mut cv = unsafe { Canvas::new(w, h) };
        cv.pixels().fill(theme::px(t.bg));
        cv
    }

    #[test]
    fn brasa_button_is_faceted_and_neon_is_hollow() {
        let (br, ne) = (styled(Style::Brasa), styled(Style::Neon));
        let mut g = glow::Cache::default();
        let mut cv = sheet(&br, 60, 40);
        button(&mut cv, &br, &mut g, 1.0, 10.0, 10.0, 50.0, 30.0, 6.0, St::default());
        assert_eq!(at(&mut cv, 10, 10), theme::px(br.bg), "esquina cortada");
        assert_ne!(at(&mut cv, 30, 20), theme::px(br.bg), "relleno");
        let mut cv = sheet(&ne, 60, 40);
        button(&mut cv, &ne, &mut g, 1.0, 10.0, 10.0, 50.0, 30.0, 6.0, St::default());
        assert_ne!(at(&mut cv, 30, 10), theme::px(ne.bg), "contorno");
        let inner = at(&mut cv, 30, 20);
        assert!((inner & 0xff).abs_diff(theme::px(ne.bg) & 0xff) < 12, "hueco por dentro");
    }

    #[test]
    fn neon_active_button_lights_its_whole_outline() {
        let ne = styled(Style::Neon);
        let mut g = glow::Cache::default();
        let mut cv = sheet(&ne, 60, 40);
        let mut idle = sheet(&ne, 60, 40);
        button(&mut idle, &ne, &mut g, 1.0, 10.0, 10.0, 50.0, 30.0, 6.0, St::default());
        button(&mut cv, &ne, &mut g, 1.0, 10.0, 10.0, 50.0, 30.0, 6.0, St { active: true, ..St::default() });
        let (a, b) = (at(&mut cv, 30, 10), at(&mut idle, 30, 10));
        assert!((a >> 16 & 0xff) + (a >> 8 & 0xff) + (a & 0xff) > (b >> 16 & 0xff) + (b >> 8 & 0xff) + (b & 0xff));
    }

    #[test]
    fn brasa_card_glows_from_below_and_neon_has_a_grid() {
        let br = styled(Style::Brasa);
        let mut cv = unsafe { Canvas::new(80, 120) };
        card(&mut cv, &br, 80.0, 120.0, 10.0);
        let (top, bottom) = (at(&mut cv, 40, 20), at(&mut cv, 40, 112));
        assert!((bottom >> 16 & 0xff) > (top >> 16 & 0xff), "mas calido abajo");
        let ne = styled(Style::Neon);
        let mut cv = unsafe { Canvas::new(80, 120) };
        card(&mut cv, &ne, 80.0, 120.0, 16.0);
        let (line, between) = (at(&mut cv, 32, 60), at(&mut cv, 24, 60)); // escala 1: paso de 16, x=32 es linea
        assert_ne!(line, between, "rejilla");
    }

    #[test]
    fn brasa_card_background_has_no_per_pixel_noise() {
        // El grano por pixel se veia como ruido de camara: la version final no lo usa.
        let br = styled(Style::Brasa);
        let mut cv = unsafe { Canvas::new(80, 400) };
        card(&mut cv, &br, 80.0, 400.0, 10.0);
        let (a, b) = (at(&mut cv, 40, 150) & 0xff, at(&mut cv, 41, 150) & 0xff);
        assert!(a.abs_diff(b) <= 1, "pixeles vecinos casi iguales, sin grano: {a} vs {b}");
    }

    #[test]
    fn fatuo_card_has_a_border_glow_that_works_on_any_card_shape() {
        let f = fatuo();
        // Forma del panel (angosta y alta), nada que ver con donde esta el play en la card principal:
        // el glow tiene que seguir el borde de ESTE rectangulo, no una posicion fija.
        let mut cv = unsafe { Canvas::new(90, 600) };
        card(&mut cv, &f, 90.0, 600.0, 14.0);
        let (near_top, mid) = (at(&mut cv, 45, 6) & 0xff, at(&mut cv, 45, 300) & 0xff);
        let (near_bottom, _) = (at(&mut cv, 45, 594) & 0xff, 0);
        assert!(near_top > mid, "arriba, cerca de su propio borde: {near_top:x} vs {mid:x}");
        assert!(near_bottom > mid, "abajo, cerca de su propio borde: {near_bottom:x} vs {mid:x}");
    }

    #[test]
    fn neon_segments_are_hollow_when_off_and_brasa_wick_warms_to_the_tip() {
        let ne = styled(Style::Neon);
        let mut g = glow::Cache::default();
        let mut cv = sheet(&ne, 70, 12);
        segments(&mut cv, &ne, &mut g, 1.0, 0.0, 2.0, 70.0, 10.0, 5, 0.0, ne.accent, 1.0);
        assert!((at(&mut cv, 6, 6) & 0xff).abs_diff(theme::px(ne.bg) & 0xff) < 12, "apagado hueco");
        let br = styled(Style::Brasa);
        let mut cv = sheet(&br, 140, 12);
        segments(&mut cv, &br, &mut g, 1.0, 0.0, 3.0, 140.0, 9.0, 14, 14.0, br.accent, 1.0);
        let (a, b) = (at(&mut cv, 8, 6), at(&mut cv, 130, 6));
        assert!((b >> 8 & 0xff) > (a >> 8 & 0xff), "la punta es mas clara/amarilla");
    }

    #[test]
    fn play_glyph_reads_on_every_style() {
        for st in [Style::Instrumento, Style::Fatuo, Style::Brasa, Style::Neon] {
            for bg in [rgb(0, 0, 0), rgb(0xf4, 0xf1, 0xea)] {
                for acc in [rgb(0xff, 0x5a, 0x1a), rgb(0xea, 0xb3, 0x08), rgb(0x22, 0xc5, 0x5e)] {
                    let mut t = Theme::new(bg, acc);
                    t.style = st;
                    let fill = match st {
                        Style::Brasa => theme::mix(t.lit, rgb(0, 0, 0), 0.15),
                        Style::Fatuo => theme::mix(t.lit, rgb(255, 255, 255), 0.45),
                        _ => t.tab,
                    };
                    assert!(theme::contrast(play_glyph(&t), fill) >= 3.0, "{:?} {:x} {:x}", st, bg, acc);
                }
            }
        }
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
