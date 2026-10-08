//! WASAPI process loopback. Cada fuente tiene su propio hilo COM y cola acotada.
use crate::audio_mix::{Shared, CHANNELS, RATE};
use std::{mem::{size_of, ManuallyDrop}, sync::{mpsc::{self, Receiver, Sender}, Arc}, time::Duration};
use windows::{
    core::{implement, Interface, PROPVARIANT, HRESULT},
    Win32::{
        Media::Audio::*,
        Foundation::{CloseHandle, HANDLE},
        System::{Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED}, Threading::{CreateEventW, WaitForSingleObject}, Variant::VT_BLOB},
    },
};

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
            hr.ok().map_err(|e| format!("activación: {e}"))?;
            interface.ok_or("activación sin interfaz")?.cast::<IAudioClient>().map_err(|e| format!("IAudioClient: {e}"))
        })();
        let _ = self.0.send(result);
        Ok(())
    }
}

struct EventGuard(HANDLE);
impl Drop for EventGuard { fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } } }

struct ComGuard;
impl Drop for ComGuard { fn drop(&mut self) { unsafe { CoUninitialize() } } }

pub struct Capture {
    stop: Sender<()>,
    pub ended: Receiver<Result<(), String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

/// Escribe directo en la mezcla compartida, multiplicando por `scale` (deshace el ducking).
pub fn open(pid: u32, mix: Arc<Shared>, scale: f32) -> Result<Capture, String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (ended_tx, ended) = mpsc::channel();
    let (stop, stop_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let result = run(pid, &mix, scale, ready_tx, stop_rx);
        let _ = ended_tx.send(result);
    });
    match ready_rx.recv_timeout(Duration::from_secs(7)) {
        Ok(Ok(())) => Ok(Capture { stop, ended, thread: Some(thread) }),
        Ok(Err(e)) => { let _ = stop.send(()); let _ = thread.join(); Err(e) }
        Err(_) => { let _ = stop.send(()); Err("timeout al iniciar la captura por proceso".into()) }
    }
}

fn run(pid: u32, mix: &Shared, scale: f32, ready: Sender<Result<(), String>>, stop: Receiver<()>) -> Result<(), String> {
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
        let mut variant = Box::new(ManuallyDrop::new(PROPVARIANT::default()));
        let blob = ActivationVariant { vt: VT_BLOB.0, reserved: [0; 3], blob_size: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, blob_ptr: (&*params as *const AUDIOCLIENT_ACTIVATION_PARAMS).cast() };
        std::ptr::write((&mut **variant as *mut PROPVARIANT).cast::<ActivationVariant>(), blob);
        let (tx, rx) = mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Completion(tx).into();
        let operation = match ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&**variant), &handler) {
            Ok(op) => op,
            Err(e) => { let msg = format!("ActivateAudioInterfaceAsync: {e}"); let _ = ready.send(Err(msg.clone())); return Err(msg); }
        };
        let client = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(client)) => client,
            Ok(Err(e)) => { let _ = ready.send(Err(e.clone())); return Err(e); }
            Err(_) => {
                // El callback puede completar tarde y aún leer el BLOB.
                Box::leak(params);
                Box::leak(variant);
                let msg = "timeout de activación de process loopback".to_string();
                let _ = ready.send(Err(msg.clone()));
                return Err(msg);
            }
        };
        drop(operation);
        drop(handler);
        let format = WAVEFORMATEX { wFormatTag: 3, nChannels: CHANNELS as u16, nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * CHANNELS as u32 * 4, nBlockAlign: CHANNELS as u16 * 4,
            wBitsPerSample: 32, cbSize: 0 };
        // Evento de WASAPI: el hilo duerme hasta que hay audio (sin sondeo).
        let event = match CreateEventW(None, false, false, None) {
            Ok(h) => EventGuard(h),
            Err(e) => { let msg = format!("CreateEventW: {e}"); let _ = ready.send(Err(msg.clone())); return Err(msg); }
        };
        let init = (|| {
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 200_000, 0, &format, None)
                .map_err(|e| format!("Initialize process loopback: {e}"))?;
            client.SetEventHandle(event.0).map_err(|e| format!("SetEventHandle: {e}"))?;
            let capture: IAudioCaptureClient = client.GetService().map_err(|e| format!("IAudioCaptureClient: {e}"))?;
            client.Start().map_err(|e| format!("Start process loopback: {e}"))?;
            Ok::<_, String>(capture)
        })();
        let capture = match init {
            Ok(capture) => capture,
            Err(e) => { let _ = ready.send(Err(e.clone())); return Err(e); }
        };
        let _ = ready.send(Ok(()));
        let result = loop {
            if stop.try_recv().is_ok() { break Ok(()); }
            let mut next = match capture.GetNextPacketSize() { Ok(n) => n, Err(e) => break Err(format!("GetNextPacketSize: {e}")) };
            let drained = (|| -> Result<(), String> { while next > 0 {
                let mut data = std::ptr::null_mut();
                let mut frames = 0;
                let mut flags = 0;
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .map_err(|e| format!("GetBuffer: {e}"))?;
                let samples = frames as usize * CHANNELS;
                // Directo del buffer de WASAPI a la mezcla, sin copia intermedia.
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    mix.push(pid, std::iter::repeat_n(0.0, samples));
                } else if data.is_null() && samples > 0 {
                    let _ = capture.ReleaseBuffer(frames);
                    return Err("GetBuffer devolvió datos nulos".to_string());
                } else if samples > 0 {
                    mix.push(pid, std::slice::from_raw_parts(data.cast::<f32>(), samples).iter().map(|&v| v * scale));
                }
                capture.ReleaseBuffer(frames).map_err(|e| format!("ReleaseBuffer: {e}"))?;
                next = capture.GetNextPacketSize().map_err(|e| format!("GetNextPacketSize: {e}"))?;
            } Ok(()) })();
            if let Err(e) = drained { break Err(e); }
            // Despierta con audio; cada 100 ms como máximo para ver si hay que parar.
            let _ = WaitForSingleObject(event.0, 100);
        };
        let _ = client.Stop();
        drop(capture);
        drop(client); // antes que el evento: WASAPI no debe señalar un handle cerrado
        result
    }
}
