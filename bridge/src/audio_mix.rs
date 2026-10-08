//! Mezcla PCM f32 estéreo a 48 kHz en bloques de 20 ms, en orden de llegada (como la 0.3.1 con
//! VB-Cable). Sin alinear por QPC: medido en 19045, los paquetes de process loopback llegan
//! contiguos, y la alineación con cursor descartaba todo como "viejo" si songbird se atrasaba.
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Seek, SeekFrom},
    sync::{atomic::{AtomicU32, AtomicU64, Ordering::Relaxed}, Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use symphonia_core::io::MediaSource;

pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const FRAMES: usize = 960;
pub const SAMPLES: usize = FRAMES * CHANNELS;
// 100 ms máximo por fuente; lo más viejo se descarta.
const MAX_QUEUED: usize = SAMPLES * 5;
// Arriba de 60 ms en cola (reloj de la placa más rápido que el de Discord) se recortan 2 frames
// por bloque: inaudible, y evita el salto de 100 ms del tope.
const DRIFT_QUEUED: usize = SAMPLES * 3;
const DRIFT_TRIM: usize = 2;
// Una fuente sin paquetes en este tiempo está en pausa: no se la espera.
const IDLE: Duration = Duration::from_millis(100);

/// Contadores en frames, para diagnóstico (simulador y bridge.log).
#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub packets: u64,
    pub overflow_dropped: u64,
    pub drift_trimmed: u64,
    pub short_frames: u64,
    pub timeouts: u64,
}

#[derive(Default)]
pub struct Mixer {
    sources: HashMap<u32, Source>,
    pub stats: Stats,
}

struct Source {
    samples: VecDeque<f32>,
    gain: f32,
    last_push: Option<Instant>,
}

impl Mixer {
    pub fn add(&mut self, pid: u32, gain: f32) {
        self.sources.insert(pid, Source { samples: VecDeque::new(), gain, last_push: None });
    }

    pub fn remove(&mut self, pid: u32) {
        self.sources.remove(&pid);
    }

    pub fn gain(&mut self, pid: u32, gain: f32) {
        if let Some(source) = self.sources.get_mut(&pid) {
            source.gain = gain.clamp(0.0, 1.0);
        }
    }

    pub fn push(&mut self, pid: u32, samples: impl Iterator<Item = f32>) {
        let Some(source) = self.sources.get_mut(&pid) else { return };
        self.stats.packets += 1;
        source.last_push = Some(Instant::now());
        source.samples.extend(samples);
        if source.samples.len() > MAX_QUEUED {
            let excess = (source.samples.len() - MAX_QUEUED).div_ceil(CHANNELS) * CHANNELS;
            self.stats.overflow_dropped += (excess / CHANNELS) as u64;
            source.samples.drain(..excess);
        }
    }

    fn live(source: &Source, now: Instant) -> bool {
        source.last_push.is_some_and(|t| now.duration_since(t) < IDLE)
    }

    /// Hay que emitir ya: hay fuentes sonando y todas tienen un bloque completo.
    fn ready(&self) -> bool {
        let now = Instant::now();
        let mut any = false;
        for s in self.sources.values().filter(|s| Self::live(s, now)) {
            if s.samples.len() < SAMPLES { return false; }
            any = true;
        }
        any
    }

    /// Ninguna fuente está sonando: no hay nada que esperar.
    fn idle(&self) -> bool {
        let now = Instant::now();
        !self.sources.values().any(|s| Self::live(s, now))
    }

    #[cfg(test)]
    pub fn next(&mut self) -> Vec<f32> {
        let mut out = vec![0.0; SAMPLES];
        self.next_into(&mut out);
        out
    }

    /// Mezcla el próximo bloque de 20 ms en `out` (SAMPLES muestras).
    pub fn next_into(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let now = Instant::now();
        for source in self.sources.values_mut() {
            if source.samples.len() > DRIFT_QUEUED {
                source.samples.drain(..DRIFT_TRIM * CHANNELS);
                self.stats.drift_trimmed += DRIFT_TRIM as u64;
            }
            let take = source.samples.len().min(SAMPLES) / CHANNELS * CHANNELS;
            if take < SAMPLES && Self::live(source, now) {
                self.stats.short_frames += ((SAMPLES - take) / CHANNELS) as u64;
            }
            for (o, v) in out.iter_mut().zip(source.samples.drain(..take)) {
                if v.is_finite() { *o += v * source.gain; }
            }
        }
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

/// Mezcla compartida: las capturas escriben, songbird tira de un bloque cuando lo necesita.
/// Sin hilo ni reloj propio: el ritmo lo ponen las capturas (reloj del motor de audio) y Discord.
pub struct Shared {
    mix: Mutex<Mixer>,
    ready: Condvar,
    generation: AtomicU64,
    pub level: Arc<AtomicU32>,
}

const BLOCK: Duration = Duration::from_millis(20);

impl Shared {
    pub fn new(level: Arc<AtomicU32>) -> Arc<Self> {
        Arc::new(Self { mix: Mutex::new(Mixer::default()), ready: Condvar::new(), generation: AtomicU64::new(0), level })
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        f(&mut self.mix.lock().unwrap())
    }

    pub fn push(&self, pid: u32, samples: impl Iterator<Item = f32>) {
        let mut mix = self.mix.lock().unwrap();
        mix.push(pid, samples);
        if mix.ready() { self.ready.notify_one(); }
    }

    pub fn stats(&self) -> Stats { self.mix.lock().unwrap().stats.clone() }

    /// Nueva salida: la anterior recibe EOF en su próxima lectura. Abrir y cerrar solo desde el
    /// hilo de serve, en orden (si no, un cierre atrasado mata la salida nueva).
    pub fn open(self: &Arc<Self>) -> Live {
        let generation = self.generation.fetch_add(1, Relaxed) + 1;
        self.ready.notify_all();
        Live { shared: self.clone(), generation, block: vec![0.0; SAMPLES], pos: SAMPLES * 4, due: None }
    }

    pub fn close(&self) {
        self.generation.fetch_add(1, Relaxed);
        self.ready.notify_all();
    }

    /// Sin fuentes sonando: silencio al instante (songbird no acumula atraso). Con fuentes: espera
    /// el bloque completo hasta `due`, la próxima marca de 20 ms en reloj real.
    fn block(&self, generation: u64, due: Instant, out: &mut [f32]) -> bool {
        let mut mix = self.mix.lock().unwrap();
        if !mix.idle() && !mix.ready() {
            let wait = due.saturating_duration_since(Instant::now());
            let (guard, timeout) = self.ready
                .wait_timeout_while(mix, wait, |m| !m.ready() && !m.idle() && self.generation.load(Relaxed) == generation)
                .unwrap();
            mix = guard;
            if timeout.timed_out() { mix.stats.timeouts += 1; }
        }
        if self.generation.load(Relaxed) != generation { return false; }
        mix.next_into(out);
        drop(mix);
        let peak = out.iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
        let old = f32::from_bits(self.level.load(Relaxed));
        self.level.store(peak.max(old * 0.9).to_bits(), Relaxed);
        true
    }
}

/// Fuente PCM en vivo para songbird (f32 LE intercalado).
pub struct Live {
    shared: Arc<Shared>,
    generation: u64,
    block: Vec<f32>,
    pos: usize, // en bytes dentro de `block`
    due: Option<Instant>,
}

impl Read for Live {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= SAMPLES * 4 {
            let now = Instant::now();
            // Marca de 20 ms en reloj real; si quedó muy atrás (pausa, atraso), se reinicia.
            let due = match self.due {
                Some(d) if d + BLOCK * 5 > now => d + BLOCK,
                _ => now + BLOCK,
            };
            self.due = Some(due);
            if !self.shared.block(self.generation, due, &mut self.block) {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            self.pos = 0;
        }
        // f32 de x86 ya es little-endian: se copia tal cual, sin convertir.
        let bytes = unsafe { std::slice::from_raw_parts(self.block.as_ptr().cast::<u8>(), SAMPLES * 4) };
        let n = buf.len().min(bytes.len() - self.pos);
        buf[..n].copy_from_slice(&bytes[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Seek for Live {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> { Err(io::Error::from(io::ErrorKind::Unsupported)) }
}

impl MediaSource for Live {
    fn is_seekable(&self) -> bool { false }
    fn byte_len(&self) -> Option<u64> { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_two_sources_and_silent_eof() {
        let mut m = Mixer::default();
        m.add(1, 1.0);
        m.add(2, 0.5);
        m.push(1, vec![0.2; SAMPLES].into_iter());
        m.push(2, vec![0.4; SAMPLES].into_iter());
        assert!(m.ready());
        assert!(m.next().iter().all(|v| (*v - 0.4).abs() < 1e-6));
        assert!(m.next().iter().all(|v| *v == 0.0));
    }

    #[test]
    fn clips_nonfinite_and_bounds_queue() {
        let mut m = Mixer::default();
        m.add(1, 1.0);
        m.push(1, vec![0.0; MAX_QUEUED].into_iter());
        m.push(1, [f32::NAN, 4.0].into_iter());
        assert_eq!(m.stats.overflow_dropped, 1);
        let mut last = Vec::new();
        for _ in 0..5 { last = m.next(); }
        // Lo último que entró sale al final, recortado a [-1, 1]; el NaN se ignora.
        let tail = last.iter().rposition(|&v| v != 0.0).unwrap();
        assert_eq!(last[tail], 1.0);
        assert!(last.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn waits_for_every_live_source() {
        let mut m = Mixer::default();
        m.add(1, 1.0);
        m.add(2, 1.0);
        m.push(1, vec![0.1; SAMPLES].into_iter());
        m.push(2, vec![0.1; SAMPLES / 2].into_iter());
        assert!(!m.ready()); // la 2 sigue sonando y le falta medio bloque
        m.push(2, vec![0.1; SAMPLES / 2].into_iter());
        assert!(m.ready());
    }

    #[test]
    fn paused_source_is_not_waited_for() {
        let mut m = Mixer::default();
        m.add(1, 1.0);
        m.add(2, 1.0);
        assert!(m.idle());
        m.push(1, vec![0.1; SAMPLES].into_iter());
        assert!(m.ready()); // la 2 nunca mandó nada: está en pausa
    }

    #[test]
    fn trims_drift_without_jumps() {
        let mut m = Mixer::default();
        m.add(1, 1.0);
        m.push(1, vec![0.1; DRIFT_QUEUED + SAMPLES].into_iter());
        let _ = m.next();
        assert_eq!(m.stats.drift_trimmed, DRIFT_TRIM as u64);
        assert_eq!(m.stats.overflow_dropped, 0);
    }

    #[test]
    fn two_ten_ms_packets_make_one_block() {
        let mut m = Mixer::default();
        m.add(1, 1.0);
        m.push(1, vec![0.3; SAMPLES / 2].into_iter());
        assert!(!m.ready());
        m.push(1, vec![0.3; SAMPLES / 2].into_iter());
        assert!(m.next().iter().all(|&v| (v - 0.3).abs() < 1e-6));
    }
}
