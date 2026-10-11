// Brillo del acento: una mascara del rectangulo redondeado, desenfocada una vez y cacheada por tamaño.
// El color se aplica al copiarla (Canvas::glow), asi cambiar el acento no la invalida.

pub struct Sprite {
    pub w: i32,
    pub h: i32,
    pub pad: i32,
    pub a: Vec<f32>,
}

pub fn rr_glow(w: i32, h: i32, rad: f32, pad: i32) -> Sprite {
    let (sw, sh) = (w + 2 * pad, h + 2 * pad);
    let mut a = vec![0.0f32; (sw * sh) as usize];
    let (cx, cy, hw, hh) = (sw as f32 / 2.0, sh as f32 / 2.0, w as f32 / 2.0, h as f32 / 2.0);
    for y in 0..sh {
        for x in 0..sw {
            // Distancia con signo (adentro negativa): con radio 0 el interior cubre del todo.
            let qx = (x as f32 + 0.5 - cx).abs() - (hw - rad);
            let qy = (y as f32 + 0.5 - cy).abs() - (hh - rad);
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rad;
            a[(y * sw + x) as usize] = (0.5 - d).clamp(0.0, 1.0);
        }
    }
    let r = (pad / 2).max(1);
    for _ in 0..3 {
        blur_h(&mut a, sw, sh, r);
        blur_v(&mut a, sw, sh, r);
    }
    Sprite { w: sw, h: sh, pad, a }
}

fn blur_h(a: &mut [f32], w: i32, h: i32, r: i32) {
    let mut row = vec![0.0f32; w as usize];
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        let o = (y * w) as usize;
        for x in 0..w {
            let mut s = 0.0;
            for k in -r..=r {
                let xx = x + k;
                if xx >= 0 && xx < w {
                    s += a[o + xx as usize];
                }
            }
            row[x as usize] = s / n;
        }
        a[o..o + w as usize].copy_from_slice(&row);
    }
}

fn blur_v(a: &mut [f32], w: i32, h: i32, r: i32) {
    let mut col = vec![0.0f32; h as usize];
    let n = (2 * r + 1) as f32;
    for x in 0..w {
        for y in 0..h {
            let mut s = 0.0;
            for k in -r..=r {
                let yy = y + k;
                if yy >= 0 && yy < h {
                    s += a[(yy * w + x) as usize];
                }
            }
            col[y as usize] = s / n;
        }
        for y in 0..h {
            a[(y * w + x) as usize] = col[y as usize];
        }
    }
}

#[derive(Default)]
pub struct Cache {
    items: Vec<((i32, i32, i32, i32), Sprite)>,
}

impl Cache {
    pub fn get(&mut self, w: i32, h: i32, rad: f32, pad: i32) -> &Sprite {
        let key = (w, h, (rad * 4.0).round() as i32, pad);
        if let Some(i) = self.items.iter().position(|(k, _)| *k == key) {
            return &self.items[i].1;
        }
        self.items.push((key, rr_glow(w, h, rad, pad)));
        &self.items.last().unwrap().1
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_bright_edges_dark() {
        let s = rr_glow(10, 10, 3.0, 6);
        assert_eq!((s.w, s.h), (22, 22));
        let at = |x: i32, y: i32| s.a[(y * s.w + x) as usize];
        assert!(at(11, 11) > 0.6);
        assert!(at(0, 0) < 0.02);
        assert!((at(3, 11) - at(18, 11)).abs() < 1e-3); // simetrico
    }

    #[test]
    fn square_corners_fill_center() {
        let s = rr_glow(10, 10, 0.0, 4);
        assert!(s.a[(s.h / 2 * s.w + s.w / 2) as usize] > 0.8);
    }

    #[test]
    fn sprite_scales_with_size() {
        let a = rr_glow(8, 8, 2.0, 4);
        let b = rr_glow(12, 12, 3.0, 6); // misma forma a escala 1,5
        assert_eq!((b.w, b.h), (24, 24));
        assert!(b.w > a.w);
    }

    #[test]
    fn cache_reuses_by_size() {
        let mut c = Cache::default();
        let p1 = c.get(10, 6, 2.0, 5) as *const Sprite;
        let p2 = c.get(10, 6, 2.0, 5) as *const Sprite;
        assert_eq!(p1, p2);
        c.get(11, 6, 2.0, 5);
        assert_eq!(c.len(), 2);
    }
}
