// Captura WASAPI de un dispositivo de entrada (p. ej. "CABLE Output" de VB-Cable) y entrega
// muestras f32 intercaladas. Tambien una fuente para songbird que lee de esa captura en vivo.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Mutex,
    },
};
use symphonia_core::io::MediaSource;

pub fn input_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub fn output_devices() -> Vec<String> {
    cpal::default_host()
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub struct Capture {
    _stream: cpal::Stream,
    pub rate: u32,
    pub channels: u16,
}

fn find(devs: impl Iterator<Item = cpal::Device>, name: &str) -> Option<cpal::Device> {
    let want = name.to_lowercase();
    let mut devs: Vec<_> = devs.collect();
    // Coincidencia exacta primero ("CABLE Output" no debe tomar "CABLE Output 16ch").
    if let Some(i) = devs.iter().position(|d| d.name().map(|n| n.to_lowercase() == want).unwrap_or(false)) {
        return Some(devs.swap_remove(i));
    }
    devs.into_iter().find(|d| d.name().map(|n| n.to_lowercase().contains(&want)).unwrap_or(false))
}

/// Abre el dispositivo de entrada y llama a `on_data` con cada bloque capturado.
pub fn open(name: &str, mut on_data: impl FnMut(&[f32]) + Send + 'static) -> Result<Capture, String> {
    let host = cpal::default_host();
    let dev = find(host.input_devices().map_err(|e| e.to_string())?, name)
        .ok_or_else(|| format!("no encontré el dispositivo de entrada \"{name}\""))?;
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let (rate, channels) = (cfg.sample_rate().0, cfg.channels());
    let err = |e| eprintln!("captura: {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_input_stream(&cfg.into(), move |d: &[f32], _| on_data(d), err, None),
        cpal::SampleFormat::I16 => {
            let mut buf = Vec::new();
            dev.build_input_stream(&cfg.into(), move |d: &[i16], _| {
                buf.clear();
                buf.extend(d.iter().map(|&s| s as f32 / 32768.0));
                on_data(&buf);
            }, err, None)
        }
        f => return Err(format!("formato de muestra no soportado: {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Capture { _stream: stream, rate, channels })
}

/// Tono de prueba en un dispositivo de salida (para verificar el cable sin que suene en los parlantes).
pub fn tone(name: &str, hz: f32) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let dev = find(host.output_devices().map_err(|e| e.to_string())?, name)
        .ok_or_else(|| format!("no encontré el dispositivo de salida \"{name}\""))?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    if cfg.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("salida en {:?}, el tono solo sabe f32", cfg.sample_format()));
    }
    let (rate, ch) = (cfg.sample_rate().0 as f32, cfg.channels() as usize);
    let mut t = 0f32;
    let s = dev
        .build_output_stream(&cfg.into(), move |d: &mut [f32], _| {
            for frame in d.chunks_mut(ch) {
                let v = (t * hz * std::f32::consts::TAU).sin() * 0.5;
                frame.fill(v);
                t += 1.0 / rate;
            }
        }, |e| eprintln!("tono: {e}"), None)
        .map_err(|e| e.to_string())?;
    s.play().map_err(|e| e.to_string())?;
    Ok(s)
}

/// Fuente en vivo para songbird: bytes f32 LE intercalados, sin fin. Lectura bloqueante: el
/// mezclador de songbird pide de a 20 ms y la captura los entrega a su ritmo.
pub struct Live {
    rx: Mutex<Receiver<Vec<f32>>>, // Mutex: songbird exige Sync
    pending: Vec<u8>,
    pos: usize,
}

/// Bloques en cola como maximo (~10 ms c/u): si Discord se atrasa se descarta lo nuevo
/// en vez de acumular latencia.
const MAX_BLOCKS: usize = 20;

pub fn live() -> (SyncSender<Vec<f32>>, Live) {
    let (tx, rx) = sync_channel(MAX_BLOCKS);
    (tx, Live { rx: Mutex::new(rx), pending: Vec::new(), pos: 0 })
}

/// Envia sin bloquear el hilo de audio; si la cola esta llena, el bloque se pierde.
pub fn push(tx: &SyncSender<Vec<f32>>, d: &[f32]) {
    let _ = tx.try_send(d.to_vec());
}

impl Read for Live {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.pending.len() {
            let block = self.rx.lock().unwrap().recv().map_err(|_| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            self.pending = block.iter().flat_map(|s| s.to_le_bytes()).collect();
            self.pos = 0;
        }
        let n = buf.len().min(self.pending.len() - self.pos);
        buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Seek for Live {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

impl MediaSource for Live {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}
