// Animaciones de la card: solo existen mientras algo cambia. Quien llama pasa el tiempo en ms y repinta
// la union de rectangulos que devuelve `step`; con la lista vacia se apaga el timer (0 % de CPU en reposo).

pub type Rect = (i32, i32, i32, i32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Hover(u32),
    Flash(u32),
    Title,
    Seg(u32),
    Boot,
}

struct Anim {
    key: Key,
    start: f64,
    dur: f32,
    from: f32,
    to: f32,
    rect: Rect,
}

impl Anim {
    fn at(&self, now: f64) -> f32 {
        let t = (((now - self.start) as f32) / self.dur).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * ease_out(t)
    }
    fn done(&self, now: f64) -> bool {
        (now - self.start) as f32 >= self.dur
    }
}

#[derive(Default)]
pub struct Anims {
    list: Vec<Anim>,
}

impl Anims {
    /// `from` NAN arranca desde el valor actual de esa animacion (o desde `to` si no habia).
    pub fn start(&mut self, key: Key, now: f64, dur_ms: f32, from: f32, to: f32, rect: Rect) {
        let cur = self.value(key, now);
        self.list.retain(|a| a.key != key);
        let from = if from.is_nan() { cur.unwrap_or(to) } else { from };
        self.list.push(Anim { key, start: now, dur: dur_ms.max(1.0), from, to, rect });
    }

    pub fn value(&self, key: Key, now: f64) -> Option<f32> {
        self.list.iter().find(|a| a.key == key).map(|a| a.at(now))
    }

    /// Union de lo que hay que repintar (incluye las que terminan ahora, para pintar su estado final).
    pub fn step(&mut self, now: f64) -> Option<Rect> {
        let mut u: Option<Rect> = None;
        for a in &self.list {
            u = Some(union(u, a.rect));
        }
        self.list.retain(|a| !a.done(now));
        u
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn finish_all(&mut self) {
        self.list.clear();
    }
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn union(a: Option<Rect>, b: Rect) -> Rect {
    match a {
        None => b,
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
    }
}

/// Brillo del LED que respira: ciclo de 2,4 s entre 0,55 y 1.
pub fn breath(now: f64) -> f32 {
    let ph = (now % 2400.0) / 2400.0 * std::f64::consts::TAU;
    (0.55 + 0.45 * (0.5 - 0.5 * ph.cos())) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    const R: Rect = (0, 0, 10, 10);

    #[test]
    fn value_eases_from_to() {
        let mut a = Anims::default();
        a.start(Key::Title, 0.0, 100.0, 0.0, 1.0, R);
        assert_eq!(a.value(Key::Title, 0.0), Some(0.0));
        let mid = a.value(Key::Title, 50.0).unwrap();
        assert!(mid > 0.5 && mid < 1.0); // ease-out: pasa la mitad antes
        assert_eq!(a.value(Key::Title, 150.0), Some(1.0));
    }

    #[test]
    fn restart_continues_from_current_value() {
        let mut a = Anims::default();
        a.start(Key::Hover(1), 0.0, 100.0, 0.0, 1.0, R);
        let v = a.value(Key::Hover(1), 50.0).unwrap();
        a.start(Key::Hover(1), 50.0, 100.0, f32::NAN, 0.0, R); // NAN = desde donde esta
        assert!((a.value(Key::Hover(1), 50.0).unwrap() - v).abs() < 1e-4);
    }

    #[test]
    fn step_returns_union_and_drops_finished() {
        let mut a = Anims::default();
        a.start(Key::Flash(1), 0.0, 100.0, 1.0, 0.0, (0, 0, 10, 10));
        a.start(Key::Flash(2), 0.0, 300.0, 1.0, 0.0, (20, 5, 30, 15));
        assert_eq!(a.step(50.0), Some((0, 0, 30, 15)));
        assert_eq!(a.step(150.0), Some((0, 0, 30, 15))); // la 1 termina: ultimo repintado
        assert_eq!(a.step(160.0), Some((20, 5, 30, 15)));
        assert_eq!(a.step(400.0), Some((20, 5, 30, 15)));
        assert!(a.is_empty());
        assert_eq!(a.step(410.0), None);
    }

    #[test]
    fn finish_all_empties() {
        let mut a = Anims::default();
        a.start(Key::Boot, 0.0, 300.0, 0.0, 1.0, R);
        a.finish_all();
        assert!(a.is_empty());
        assert_eq!(a.value(Key::Boot, 10.0), None);
    }

    #[test]
    fn breath_stays_in_range() {
        for i in 0..100 {
            let b = breath(i as f64 * 37.0);
            assert!((0.55..=1.0).contains(&b));
        }
    }
}
