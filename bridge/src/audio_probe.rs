//! Diagnóstico offline de process loopback. Solo lee PCM y publica estadísticas.
//! El operador controla mute, volumen y endpoint fuera de este proceso.
use std::{
    mem::{size_of, ManuallyDrop},
    sync::mpsc::{channel, Sender},
    time::{Duration, Instant},
};
use windows::{
    core::{implement, Interface, PROPVARIANT, HRESULT},
    Win32::{
        Foundation::{BOOL, S_OK},
        Media::Audio::*,
        System::{Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED}, Variant::VT_BLOB},
    },
};

const MAX_SECONDS: u64 = 30;
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(5);
const PCM_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
// This enum member is in current Windows SDK documentation but absent from windows 0.54 metadata.
const POST_VOLUME_LOOPBACK: AUDCLNT_STREAMOPTIONS = AUDCLNT_STREAMOPTIONS(4);

#[repr(C)]
struct ActivationVariant {
    vt: u16,
    reserved: [u16; 3],
    blob_size: u32,
    blob_ptr: *const u8,
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion(Sender<Result<IAudioClient, String>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion {
    fn ActivateCompleted(&self, operation: Option<&IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let result = (|| unsafe {
            let operation = operation.ok_or("callback sin operación")?;
            let mut hr = HRESULT(0);
            let mut interface = None;
            operation.GetActivateResult(&mut hr, &mut interface).map_err(|e| format!("GetActivateResult: {e}"))?;
            println!("activation_hresult=0x{:08X}", hr.0 as u32);
            hr.ok().map_err(|e| format!("activación: {e}"))?;
            interface.ok_or("activación sin interfaz")?.cast::<IAudioClient>().map_err(|e| format!("IAudioClient: {e}"))
        })();
        let _ = self.0.send(result);
        Ok(())
    }
}

struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) { unsafe { CoUninitialize() } }
}

struct Started<'a>(&'a IAudioClient);
impl Drop for Started<'_> {
    fn drop(&mut self) { let _ = unsafe { self.0.Stop() }; }
}

#[derive(Default)]
struct Levels { packets: u64, frames: u64, samples: u64, nonzero: u64, sum_sq: f64, peak: f32 }
impl Levels {
    fn add(&mut self, samples: &[f32], frames: u32) {
        self.packets += 1;
        self.frames += frames as u64;
        for &v in samples {
            self.samples += 1;
            if v != 0.0 { self.nonzero += 1; }
            self.sum_sq += (v as f64) * (v as f64);
            self.peak = self.peak.max(v.abs());
        }
    }
    fn add_silence(&mut self, frames: u32) {
        self.packets += 1;
        self.frames += frames as u64;
        self.samples += frames as u64 * CHANNELS as u64;
    }
    fn rms(&self) -> f64 { if self.samples == 0 { 0.0 } else { (self.sum_sq / self.samples as f64).sqrt() } }
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let usage = "uso: echowisp-bridge audio-probe --pid <PID> [--seconds 5] [--post-volume]";
    let mut pid = None;
    let mut seconds = 5;
    let mut post_volume = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pid" => pid = Some(args.next().ok_or(usage)?.parse::<u32>().map_err(|_| usage)?),
            "--seconds" => seconds = args.next().ok_or(usage)?.parse::<u64>().map_err(|_| usage)?,
            "--post-volume" => post_volume = true,
            _ => return Err(usage.into()),
        }
    }
    let pid = pid.filter(|&p| p != 0).ok_or(usage)?;
    if !(1..=MAX_SECONDS).contains(&seconds) { return Err(format!("--seconds debe ser 1..={MAX_SECONDS}")); }
    println!("audio-probe pid={pid} seconds={seconds} stream_option={}", if post_volume { "post-volume" } else { "default-pre-volume" });
    println!("Solo lectura: no cambia mute, volumen ni dispositivo; no guarda PCM.");

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| format!("CoInitializeEx: {e}"))?;
        let _com = ComGuard;
        let params = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 { ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            } },
        });
        // VT_BLOB points to stack storage retained until completion or timeout. The variant
        // does not own this pointer, so suppress its destructor (PropVariantClear).
        let mut variant = Box::new(ManuallyDrop::new(PROPVARIANT::default()));
        assert!(size_of::<ActivationVariant>() <= size_of::<PROPVARIANT>());
        let blob = ActivationVariant { vt: VT_BLOB.0, reserved: [0; 3], blob_size: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, blob_ptr: (&*params as *const AUDIOCLIENT_ACTIVATION_PARAMS).cast() };
        std::ptr::write((&mut **variant as *mut PROPVARIANT).cast::<ActivationVariant>(), blob);

        let (tx, rx) = channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Completion(tx).into();
        let operation = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&**variant), &handler)
            .map_err(|e| format!("ActivateAudioInterfaceAsync inmediato: {e}"))?;
        let activated = rx.recv_timeout(ACTIVATION_TIMEOUT);
        if activated.is_err() {
            // La API puede completar tarde: conservar ambos buffers hasta que salga el proceso.
            Box::leak(params);
            Box::leak(variant);
            return Err("timeout de activación (5 s)".into());
        }
        let client = activated.unwrap()?;
        drop(operation);
        drop(handler);
        // El cliente de process loopback no expone IAudioClient2: solo se pide si se quiere post-volume.
        let props = AudioClientProperties { cbSize: size_of::<AudioClientProperties>() as u32, bIsOffload: BOOL(0), eCategory: AudioCategory_Other,
            Options: if post_volume { POST_VOLUME_LOOPBACK } else { AUDCLNT_STREAMOPTIONS_NONE } };
        if post_volume {
            let client2: IAudioClient2 = client.cast().map_err(|e| format!("post-volume no disponible (IAudioClient2: {e})"))?;
            client2.SetClientProperties(&props).map_err(|e| format!("SetClientProperties: {e}"))?;
            println!("set_client_properties_hresult=0x{:08X}", S_OK.0 as u32);
        } else {
            println!("set_client_properties=not_called (Windows default)");
        }

        let format = WAVEFORMATEX { wFormatTag: 3, nChannels: CHANNELS, nSamplesPerSec: PCM_RATE,
            nAvgBytesPerSec: PCM_RATE * CHANNELS as u32 * size_of::<f32>() as u32,
            nBlockAlign: CHANNELS * size_of::<f32>() as u16, wBitsPerSample: 32, cbSize: 0 };
        let tag = format.wFormatTag;
        let channels = format.nChannels;
        let rate = format.nSamplesPerSec;
        let bits = format.wBitsPerSample;
        println!("requested_format=IEEE_FLOAT rate={rate} channels={channels} bits={bits} tag={tag}");
        // Process loopback exige LOOPBACK (ejemplo ApplicationLoopback de Microsoft); sin el: AUDCLNT_E_INVALID_STREAM_FLAG.
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, 200_000, 0, &format, None)
            .map_err(|e| format!("Initialize: {e}"))?;
        println!("initialize_hresult=0x{:08X}", S_OK.0 as u32);
        let capture: IAudioCaptureClient = client.GetService().map_err(|e| format!("IAudioCaptureClient: {e}"))?;
        client.Start().map_err(|e| format!("Start: {e}"))?;
        let _started = Started(&client);
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut levels = Levels::default();
        let mut silent_frames = 0u64;
        while Instant::now() < deadline {
            let mut next = capture.GetNextPacketSize().map_err(|e| format!("GetNextPacketSize: {e}"))?;
            while next > 0 && Instant::now() < deadline {
                let mut data = std::ptr::null_mut();
                let mut frames = 0;
                let mut flags = 0;
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).map_err(|e| format!("GetBuffer: {e}"))?;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    silent_frames += frames as u64;
                    levels.add_silence(frames);
                } else {
                    let count = frames as usize * CHANNELS as usize;
                    if count == 0 {
                        levels.add_silence(0);
                    } else if data.is_null() {
                        capture.ReleaseBuffer(frames).map_err(|e| format!("ReleaseBuffer: {e}"))?;
                        return Err("GetBuffer devolvió datos nulos sin SILENT".into());
                    } else {
                        let samples = std::slice::from_raw_parts(data.cast::<f32>(), count);
                        levels.add(samples, frames);
                    }
                }
                capture.ReleaseBuffer(frames).map_err(|e| format!("ReleaseBuffer: {e}"))?;
                next = capture.GetNextPacketSize().map_err(|e| format!("GetNextPacketSize: {e}"))?;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let rms = levels.rms();
        let dbfs = if rms > 0.0 { 20.0 * rms.log10() } else { f64::NEG_INFINITY };
        println!("packets={} frames={} silent_frames={} samples={} nonzero={} peak={:.8} rms={:.8} rms_dbfs={:.2}",
            levels.packets, levels.frames, silent_frames, levels.samples, levels.nonzero, levels.peak, rms, dbfs);
        println!("La audibilidad local se verifica escuchando la fuente; RMS solo mide captura. Comparar la misma señal y condiciones por separado.");
    }
    Ok(())
}
