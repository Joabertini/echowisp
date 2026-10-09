//! `echowisp-bridge simular --pid N [--secs S] [--out f.raw]`: prueba de la cadena de audio sin Discord.
//! Captura la app como en vivo y la lee con el mismo camino que songbird (RawAdapter → formato
//! crudo → decodificador) al mismo ritmo: un paquete, dormir hasta la próxima marca de 20 ms.
//! Pensado para un tono senoidal: cuenta saltos (crujidos) y silencios en la salida.
use crate::{audio_mix::{Shared, CHANNELS, RATE}, process_capture};
use songbird::input::{codecs::{get_codec_registry, get_probe}, AudioStream, LiveInput, RawAdapter};
use std::{sync::{atomic::AtomicU32, Arc}, time::{Duration, Instant}};
use symphonia_core::{audio::SampleBuffer, io::MediaSource};

const TICK: Duration = Duration::from_millis(20);

pub fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let (mut pid, mut secs, mut out, mut espera) = (None, 10u64, None::<String>, 0u64);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pid" => pid = args.next().and_then(|v| v.parse::<u32>().ok()),
            "--secs" => secs = args.next().and_then(|v| v.parse().ok()).unwrap_or(10),
            "--out" => out = args.next(),
            // Como en vivo: el bot ya lee antes de que se elija la app.
            "--espera" => espera = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            _ => return Err(format!("argumento desconocido: {a}")),
        }
    }
    let pid = pid.ok_or("falta --pid")?;

    let shared = Shared::new(Arc::new(AtomicU32::new(0)));
    let live = shared.open();
    let mut cap = None;

    let raw: Box<dyn MediaSource> = Box::new(RawAdapter::new(live, RATE, CHANNELS as u32));
    let LiveInput::Parsed(mut parsed) = LiveInput::Raw(AudioStream { input: raw })
        .promote(get_codec_registry(), get_probe()).map_err(|e| format!("promote: {e}"))?
    else { return Err("songbird no parseó la entrada".into()) };

    let ticks = (secs + espera) * 50;
    let source_at = espera * 50;
    let mut pcm: Vec<f32> = Vec::with_capacity(ticks as usize * 960 * CHANNELS);
    let mut reads: Vec<f64> = Vec::with_capacity(ticks as usize);
    let mut late = 0u32;
    let mut deadline = Instant::now();
    for tick in 0..ticks {
        if tick == source_at {
            shared.with(|m| m.add(pid, 1.0));
            cap = Some(process_capture::open(pid, shared.clone(), 1.0)?);
            println!("fuente agregada; atraso acumulado {:.0} ms", Instant::now().saturating_duration_since(deadline).as_secs_f64() * 1000.0);
        }
        let t = Instant::now();
        let pkt = parsed.format.next_packet().map_err(|e| format!("next_packet: {e} (la pista terminaría acá)"))?;
        let decoded = parsed.decoder.decode(&pkt).map_err(|e| format!("decode: {e}"))?;
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buf.copy_interleaved_ref(decoded);
        pcm.extend_from_slice(buf.samples());
        reads.push(t.elapsed().as_secs_f64() * 1000.0);
        deadline += TICK;
        let now = Instant::now();
        if now > deadline { late += 1; } else { std::thread::sleep(deadline - now); }
    }
    println!("atraso final {:.0} ms", Instant::now().saturating_duration_since(deadline).as_secs_f64() * 1000.0);
    drop(cap);
    let stats = shared.stats();
    shared.close();

    if let Some(path) = out {
        let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))?;
    }

    // Analisis del canal izquierdo, sin el primer medio segundo (arranque).
    let skip = (espera as usize * RATE as usize) + RATE as usize / 2;
    let left: Vec<f32> = pcm.iter().step_by(CHANNELS).copied().skip(skip).collect();
    let peak = left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // Senoidal de 440 Hz: |x[n]-2x[n-1]+x[n-2]| <= A*(2*pi*440/48000)^2 ~ 0.0033*A. Mucho mas = salto.
    let threshold = (peak * 0.0033 * 10.0).max(0.01);
    let (mut jumps, mut last_jump) = (0u32, 0usize);
    for n in 2..left.len() {
        if (left[n] - 2.0 * left[n - 1] + left[n - 2]).abs() > threshold && (jumps == 0 || n - last_jump > 48) {
            jumps += 1;
            last_jump = n;
        }
    }
    let silent_ms = left.chunks(48).filter(|c| c.iter().all(|v| v.abs() < peak * 0.05 + 1e-6)).count();
    let analyzed_s = left.len() as f64 / RATE as f64;
    reads.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| reads[((reads.len() - 1) as f64 * p) as usize];

    println!("salida: {analyzed_s:.1} s analizados, pico {peak:.3}");
    println!("saltos (crujidos): {jumps} ({:.1}/s)", jumps as f64 / analyzed_s);
    println!("silencio: {silent_ms} ms ({:.1} %)", silent_ms as f64 / (analyzed_s * 10.0));
    println!("lectura por paquete (ms): mediana {:.2} p99 {:.2} max {:.2}; ticks tarde {late}/{ticks}", pct(0.5), pct(0.99), pct(1.0));
    println!("mezcla: {stats:?}");
    Ok(())
}
