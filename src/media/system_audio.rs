use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::core::{Interface, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Vtbl, IAudioCaptureClient, IAudioClient,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    AUDCLNT_S_BUFFER_EMPTY, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

use crate::media::audio::AudioThreadPriorityGuard;
use crate::media::codecs::OpusStereoAudioEncoder;
use crate::net::webrtc::AudioFanoutBus;

pub const SYS_AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const SYS_AUDIO_CHANNELS: usize = 2;
pub const SYS_AUDIO_FRAME_PER_CH: usize = 960; // 20 ms
pub const SYS_AUDIO_FRAME_SAMPLES: usize = SYS_AUDIO_FRAME_PER_CH * SYS_AUDIO_CHANNELS; // 1920 i16
const SILENCE_HOLD_FRAMES: u8 = 25; // 500 ms antes de pausar em silencio digital absoluto
const AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK: AUDIOCLIENT_ACTIVATION_TYPE =
    AUDIOCLIENT_ACTIVATION_TYPE(1);
const VT_BLOB_U16: u16 = 65;
const IID_IAGILE_OBJECT: GUID = GUID::from_u128(0x94ea2b94_e9cc_49e0_c0ff_ee64ca8f5b90);
const E_NOINTERFACE_HR: HRESULT = HRESULT(0x80004002_u32 as i32);
const E_POINTER_HR: HRESULT = HRESULT(0x80004003_u32 as i32);
const E_UNEXPECTED_HR: HRESULT = HRESULT(0x8000FFFF_u32 as i32);
const S_OK_HR: HRESULT = HRESULT(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAudioMode {
    /// Captura todo audio do sistema EXCETO o processo indicado (evita eco da chamada ao compartilhar monitor).
    ExcludeProcessTree(u32),
    /// Captura apenas o audio da arvore de processos da janela selecionada.
    IncludeProcessTree(u32),
}

/// Frame de audio estereo 48 kHz (20 ms = 1920 amostras i16 intercaladas L/R) pronto para Opus.
pub struct CapturedSystemAudioFrame {
    pub samples: [i16; SYS_AUDIO_FRAME_SAMPLES],
    pub frame_seq: u64,
    pub talkspurt_start: bool,
}

/// Processador estereo zero-alloc para audio de tela/janela (downmix/upmix -> 48 kHz estereo 20 ms).
pub struct SystemAudioProcessor {
    in_rate: u32,
    channels: usize,
    step: f64,
    phase: f64,
    prev_l: f32,
    prev_r: f32,
    has_prev: bool,
    accum: [i16; SYS_AUDIO_FRAME_SAMPLES],
    accum_len: usize,
    pub frame_seq: u64,
    active_stream: bool,
    silent_hold: u8,
    force_marker: bool,
    last_cb_at: Option<Instant>,
}

impl SystemAudioProcessor {
    pub fn new(in_rate: u32, channels: usize, initial_seq: u64) -> Self {
        let safe_rate = in_rate.max(8_000);
        Self {
            in_rate: safe_rate,
            channels: channels.max(1),
            step: (safe_rate as f64) / (SYS_AUDIO_SAMPLE_RATE as f64),
            phase: 0.0,
            prev_l: 0.0,
            prev_r: 0.0,
            has_prev: false,
            accum: [0i16; SYS_AUDIO_FRAME_SAMPLES],
            accum_len: 0,
            frame_seq: initial_seq,
            active_stream: false,
            silent_hold: 0,
            force_marker: true,
            last_cb_at: None,
        }
    }

    fn advance_clock_on_gap(&mut self, now: Instant) {
        if let Some(prev_t) = self.last_cb_at {
            let gap_ms = now.saturating_duration_since(prev_t).as_millis() as u64;
            if gap_ms > 120 {
                let missed = (gap_ms / 20).saturating_sub(1);
                if missed > 0 {
                    self.frame_seq = self.frame_seq.wrapping_add(missed);
                    self.accum_len = 0;
                    self.active_stream = false;
                    self.silent_hold = 0;
                    self.force_marker = true;
                }
            }
        }
        self.last_cb_at = Some(now);
    }

    fn emit_if_ready(&mut self, mut emit: impl FnMut(CapturedSystemAudioFrame)) {
        if self.accum_len < SYS_AUDIO_FRAME_SAMPLES {
            return;
        }
        self.accum_len = 0;
        let seq = self.frame_seq;
        self.frame_seq = self.frame_seq.wrapping_add(1);

        let has_signal = self.accum.iter().any(|&s| s != 0);
        if has_signal {
            let is_start = !self.active_stream || self.force_marker;
            self.active_stream = true;
            self.silent_hold = SILENCE_HOLD_FRAMES;
            self.force_marker = false;
            emit(CapturedSystemAudioFrame {
                samples: self.accum,
                frame_seq: seq,
                talkspurt_start: is_start,
            });
        } else if self.silent_hold > 0 {
            self.silent_hold -= 1;
            let is_start = self.force_marker;
            self.force_marker = false;
            if self.silent_hold == 0 {
                self.active_stream = false;
            }
            emit(CapturedSystemAudioFrame {
                samples: self.accum,
                frame_seq: seq,
                talkspurt_start: is_start,
            });
        } else {
            self.active_stream = false;
            self.force_marker = true;
        }
    }

    /// Caminho direto sem conversao de taxa para PCM i16 estereo 48 kHz (WASAPI Process Loopback).
    pub fn process_i16_stereo_48k(
        &mut self,
        data: &[i16],
        now: Instant,
        mut emit: impl FnMut(CapturedSystemAudioFrame),
    ) {
        self.advance_clock_on_gap(now);
        let total = (data.len() / 2) * 2;
        let mut idx = 0;
        while idx < total {
            let need = SYS_AUDIO_FRAME_SAMPLES - self.accum_len;
            let take = (total - idx).min(need);
            self.accum[self.accum_len..self.accum_len + take]
                .copy_from_slice(&data[idx..idx + take]);
            self.accum_len += take;
            idx += take;
            if self.accum_len >= SYS_AUDIO_FRAME_SAMPLES {
                self.emit_if_ready(&mut emit);
            }
        }
    }

    #[inline]
    fn stereo_at_f32(data: &[f32], channels: usize, frame_idx: usize) -> (f32, f32) {
        let base = frame_idx * channels;
        if channels == 1 {
            let m = data[base];
            (m, m)
        } else {
            (data[base], data[base + 1])
        }
    }

    /// Converte blocos intercalados em qualquer taxa/canal (fallback `cpal` loopback) para 48 kHz estereo.
    pub fn process_interleaved_f32(
        &mut self,
        data: &[f32],
        now: Instant,
        mut emit: impl FnMut(CapturedSystemAudioFrame),
    ) {
        self.advance_clock_on_gap(now);
        let ch = self.channels;
        let frame_count = data.len() / ch;
        if frame_count == 0 {
            return;
        }

        if self.in_rate == SYS_AUDIO_SAMPLE_RATE {
            for i in 0..frame_count {
                let (l, r) = Self::stereo_at_f32(data, ch, i);
                self.prev_l = l;
                self.prev_r = r;
                self.has_prev = true;
                self.accum[self.accum_len] = (l.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                self.accum[self.accum_len + 1] = (r.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                self.accum_len += 2;
                if self.accum_len >= SYS_AUDIO_FRAME_SAMPLES {
                    self.emit_if_ready(&mut emit);
                }
            }
            return;
        }

        let (first_l, first_r) = Self::stereo_at_f32(data, ch, 0);
        if !self.has_prev {
            self.prev_l = first_l;
            self.prev_r = first_r;
            self.has_prev = true;
            self.phase = 0.0;
        }
        let limit = (frame_count as f64) - 1.0;
        while self.phase < limit {
            let (l, r) = if self.phase < 0.0 {
                let t = (self.phase + 1.0) as f32;
                (
                    self.prev_l + (first_l - self.prev_l) * t,
                    self.prev_r + (first_r - self.prev_r) * t,
                )
            } else {
                let idx = (self.phase.floor() as usize).min(frame_count.saturating_sub(2));
                let t = (self.phase - (idx as f64)) as f32;
                let (l0, r0) = Self::stereo_at_f32(data, ch, idx);
                let (l1, r1) = Self::stereo_at_f32(data, ch, idx + 1);
                (l0 + (l1 - l0) * t, r0 + (r1 - r0) * t)
            };
            self.accum[self.accum_len] = (l.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            self.accum[self.accum_len + 1] = (r.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            self.accum_len += 2;
            if self.accum_len >= SYS_AUDIO_FRAME_SAMPLES {
                self.emit_if_ready(&mut emit);
            }
            self.phase += self.step;
        }
        let (last_l, last_r) = Self::stereo_at_f32(data, ch, frame_count - 1);
        self.prev_l = last_l;
        self.prev_r = last_r;
        self.phase -= frame_count as f64;
        if self.phase < -1.0 {
            self.phase = -1.0;
        }
    }
}

/// Layout binario exato de `PROPVARIANT` (`VT_BLOB`) na stack sem `Drop` chamando `PropVariantClear`.
#[repr(C)]
struct PropVariantBlob {
    vt: u16,
    w_reserved1: u16,
    w_reserved2: u16,
    w_reserved3: u16,
    cb_size: u32,
    #[cfg(target_pointer_width = "64")]
    _pad: u32,
    p_blob_data: *mut u8,
    #[cfg(target_pointer_width = "32")]
    _pad_tail: u32,
}

struct CompletionShared {
    done_event: HANDLE,
    _activation_params: Box<AUDIOCLIENT_ACTIVATION_PARAMS>,
    result: Mutex<(HRESULT, Option<IAudioClient>)>,
}

impl Drop for CompletionShared {
    fn drop(&mut self) {
        if !self.done_event.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.done_event);
            }
        }
    }
}

#[repr(C)]
struct ProcessLoopbackHandler {
    vtable: *const IActivateAudioInterfaceCompletionHandler_Vtbl,
    refs: AtomicU32,
    shared: Arc<CompletionShared>,
}

unsafe extern "system" fn handler_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    interface: *mut *mut c_void,
) -> HRESULT {
    if interface.is_null() {
        return E_POINTER_HR;
    }
    if iid.is_null() {
        *interface = null_mut();
        return E_POINTER_HR;
    }
    let g = *iid;
    if g == windows::core::IUnknown::IID
        || g == IActivateAudioInterfaceCompletionHandler::IID
        || g == IID_IAGILE_OBJECT
    {
        *interface = this;
        handler_add_ref(this);
        S_OK_HR
    } else {
        *interface = null_mut();
        E_NOINTERFACE_HR
    }
}

unsafe extern "system" fn handler_add_ref(this: *mut c_void) -> u32 {
    let h = &*(this as *const ProcessLoopbackHandler);
    h.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn handler_release(this: *mut c_void) -> u32 {
    let h = &*(this as *const ProcessLoopbackHandler);
    let prev = h.refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ProcessLoopbackHandler));
    }
    prev.saturating_sub(1)
}

unsafe extern "system" fn handler_activate_completed(
    this: *mut c_void,
    op_raw: *mut c_void,
) -> HRESULT {
    let h = &*(this as *const ProcessLoopbackHandler);
    let mut final_hr = E_UNEXPECTED_HR;
    let mut client_opt = None;

    if !op_raw.is_null() {
        let op = std::mem::ManuallyDrop::new(IActivateAudioInterfaceAsyncOperation::from_raw(op_raw));
        let mut activate_hr = S_OK_HR;
        let mut unknown = None;
        match op.GetActivateResult(&mut activate_hr, &mut unknown) {
            Ok(()) => {
                if activate_hr.is_err() {
                    final_hr = activate_hr;
                } else if let Some(unk) = unknown {
                    match unk.cast::<IAudioClient>() {
                        Ok(c) => {
                            final_hr = S_OK_HR;
                            client_opt = Some(c);
                        }
                        Err(e) => final_hr = e.code(),
                    }
                }
            }
            Err(e) => final_hr = e.code(),
        }
    }

    if let Ok(mut guard) = h.shared.result.lock() {
        *guard = (final_hr, client_opt);
    }
    let _ = SetEvent(h.shared.done_event);
    S_OK_HR
}

static HANDLER_VTABLE: IActivateAudioInterfaceCompletionHandler_Vtbl =
    IActivateAudioInterfaceCompletionHandler_Vtbl {
        base__: windows::core::IUnknown_Vtbl {
            QueryInterface: handler_query_interface,
            AddRef: handler_add_ref,
            Release: handler_release,
        },
        ActivateCompleted: handler_activate_completed,
    };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasapiBufferPoll {
    Empty,
    ZeroFrames,
    Data,
    Error(HRESULT),
}

/// Classifica o retorno bruto (`HRESULT`) de `IAudioCaptureClient::GetBuffer`.
/// Em `windows-rs`, `AUDCLNT_S_BUFFER_EMPTY` (`0x08890001`) tem bit de sucesso (`.is_ok() == true`),
/// portanto `.ok()` retornaria `Ok(())` com `frames == 0` e causaria spin infinito a 100% de CPU.
#[inline]
fn classify_wasapi_get_buffer(hr: HRESULT, frames: u32) -> WasapiBufferPoll {
    if hr == AUDCLNT_S_BUFFER_EMPTY {
        WasapiBufferPoll::Empty
    } else if hr.is_err() {
        WasapiBufferPoll::Error(hr)
    } else if frames == 0 {
        WasapiBufferPoll::ZeroFrames
    } else {
        WasapiBufferPoll::Data
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(lp_lib_file_name: *const u8) -> isize;
    fn GetProcAddress(h_module: isize, lp_proc_name: *const u8) -> *const c_void;
}

unsafe fn dyn_activate_audio_interface_async(
    device_interface_path: PCWSTR,
    riid: *const GUID,
    activation_params: *const c_void,
    completion_handler: &IActivateAudioInterfaceCompletionHandler,
) -> Result<IActivateAudioInterfaceAsyncOperation, String> {
    static FN_ADDR: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let addr = *FN_ADDR.get_or_init(|| {
        let m = LoadLibraryA(b"mmdevapi.dll\0".as_ptr());
        if m == 0 {
            return 0;
        }
        let p = GetProcAddress(m, b"ActivateAudioInterfaceAsync\0".as_ptr());
        if p.is_null() {
            0
        } else {
            p as usize
        }
    });
    if addr == 0 {
        return Err("ActivateAudioInterfaceAsync indisponivel nesta versao do Windows.".to_string());
    }
    type FnTy = unsafe extern "system" fn(
        PCWSTR,
        *const GUID,
        *const c_void,
        *mut c_void,
        *mut *mut c_void,
    ) -> HRESULT;
    let func: FnTy = std::mem::transmute(addr);
    let mut op_raw: *mut c_void = std::ptr::null_mut();
    let hr = func(
        device_interface_path,
        riid,
        activation_params,
        completion_handler.as_raw(),
        &mut op_raw,
    );
    hr.ok()
        .map_err(|e| format!("ActivateAudioInterfaceAsync falhou: {}", e))?;
    if op_raw.is_null() {
        return Err("ActivateAudioInterfaceAsync retornou operacao nula.".to_string());
    }
    Ok(IActivateAudioInterfaceAsyncOperation::from_raw(op_raw))
}

fn activate_process_loopback(
    mode: SystemAudioMode,
    stop_flag: &AtomicBool,
) -> Result<IAudioClient, String> {
    let (target_pid, loopback_mode) = match mode {
        SystemAudioMode::ExcludeProcessTree(pid) => {
            (pid, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)
        }
        SystemAudioMode::IncludeProcessTree(pid) => {
            (pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)
        }
    };

    let mut params = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: target_pid,
                ProcessLoopbackMode: loopback_mode,
            },
        },
    });

    let params_ptr: *mut u8 = (&mut *params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast::<u8>();

    let done_event = unsafe { CreateEventW(None, true, false, None) }
        .map_err(|e| format!("CreateEventW loopback falhou: {}", e))?;

    let shared = Arc::new(CompletionShared {
        done_event,
        _activation_params: params,
        result: Mutex::new((E_UNEXPECTED_HR, None)),
    });

    let blob = PropVariantBlob {
        vt: VT_BLOB_U16,
        w_reserved1: 0,
        w_reserved2: 0,
        w_reserved3: 0,
        cb_size: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        #[cfg(target_pointer_width = "64")]
        _pad: 0,
        p_blob_data: params_ptr,
        #[cfg(target_pointer_width = "32")]
        _pad_tail: 0,
    };

    let boxed_handler = Box::into_raw(Box::new(ProcessLoopbackHandler {
        vtable: &HANDLER_VTABLE,
        refs: AtomicU32::new(1),
        shared: Arc::clone(&shared),
    }));
    let com_handler =
        unsafe { IActivateAudioInterfaceCompletionHandler::from_raw(boxed_handler as *mut c_void) };

    let pv_ptr = (&blob as *const PropVariantBlob).cast::<c_void>();
    let _op = unsafe {
        dyn_activate_audio_interface_async(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            pv_ptr,
            &com_handler,
        )
    }?;

    let mut activated = false;
    for _ in 0..60 {
        if stop_flag.load(Ordering::Acquire) {
            return Err("Ativacao WASAPI cancelada.".to_string());
        }
        if unsafe { WaitForSingleObject(shared.done_event, 50) } == WAIT_OBJECT_0 {
            activated = true;
            break;
        }
    }
    if !activated {
        return Err("Timeout aguardando ativacao WASAPI Process Loopback.".to_string());
    }

    let mut guard = shared
        .result
        .lock()
        .map_err(|_| "Mutex loopback corrompido.".to_string())?;
    let (hr, client_opt) = (guard.0, guard.1.take());
    if hr.is_err() {
        return Err(format!("Process Loopback recusado: 0x{:08x}", hr.0 as u32));
    }
    client_opt.ok_or_else(|| "Process Loopback retornou IAudioClient nulo.".to_string())
}

struct EventHandleGuard(HANDLE);
impl Drop for EventHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn try_run_process_loopback(
    mode: SystemAudioMode,
    stop_flag: &AtomicBool,
    mut on_frame: impl FnMut(CapturedSystemAudioFrame),
) -> Result<(), String> {
    let client = activate_process_loopback(mode, stop_flag)?;

    let block_align = (SYS_AUDIO_CHANNELS * 2) as u16;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: SYS_AUDIO_CHANNELS as u16,
        nSamplesPerSec: SYS_AUDIO_SAMPLE_RATE,
        nAvgBytesPerSec: SYS_AUDIO_SAMPLE_RATE * (block_align as u32),
        nBlockAlign: block_align,
        wBitsPerSample: 16,
        cbSize: 0,
    };

    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;

    unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 2_000_000, 0, &format, None) }
        .map_err(|e| format!("IAudioClient::Initialize loopback falhou: {}", e))?;

    let capture: IAudioCaptureClient = unsafe { client.GetService() }
        .map_err(|e| format!("GetService<IAudioCaptureClient> falhou: {}", e))?;

    let buffer_ready = unsafe { CreateEventW(None, false, false, None) }
        .map_err(|e| format!("CreateEventW buffer_ready falhou: {}", e))?;
    let _ev_guard = EventHandleGuard(buffer_ready);
    unsafe { client.SetEventHandle(buffer_ready) }
        .map_err(|e| format!("SetEventHandle falhou: {}", e))?;

    unsafe { client.Start() }.map_err(|e| format!("IAudioClient::Start falhou: {}", e))?;

    let mut proc = SystemAudioProcessor::new(SYS_AUDIO_SAMPLE_RATE, SYS_AUDIO_CHANNELS, 0);
    let mut silence = vec![0i16; SYS_AUDIO_FRAME_SAMPLES * 4];
    let mut pcm_scratch = vec![0i16; SYS_AUDIO_FRAME_SAMPLES * 4];

    while !stop_flag.load(Ordering::Acquire) {
        let wait = unsafe { WaitForSingleObject(buffer_ready, 25) };
        if stop_flag.load(Ordering::Acquire) {
            break;
        }
        if wait != WAIT_OBJECT_0 {
            continue;
        }

        for _ in 0..32 {
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            let mut data_ptr: *mut u8 = null_mut();
            let mut frames: u32 = 0;
            let mut buf_flags: u32 = 0;
            let hr = unsafe {
                (Interface::vtable(&capture).GetBuffer)(
                    Interface::as_raw(&capture),
                    &mut data_ptr,
                    &mut frames,
                    &mut buf_flags,
                    null_mut(),
                    null_mut(),
                )
            };
            match classify_wasapi_get_buffer(hr, frames) {
                WasapiBufferPoll::Empty => break,
                WasapiBufferPoll::ZeroFrames => {
                    let _ = unsafe { capture.ReleaseBuffer(0) };
                    break;
                }
                WasapiBufferPoll::Error(err_hr) => {
                    let _ = unsafe { client.Stop() };
                    return Err(format!(
                        "IAudioCaptureClient::GetBuffer falhou: 0x{:08x}",
                        err_hr.0 as u32
                    ));
                }
                WasapiBufferPoll::Data => {}
            }

            let sample_count = (frames as usize) * SYS_AUDIO_CHANNELS;
            if (buf_flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0 || data_ptr.is_null() {
                if silence.len() < sample_count {
                    silence.resize(sample_count, 0);
                }
                proc.process_i16_stereo_48k(&silence[..sample_count], Instant::now(), &mut on_frame);
            } else {
                // Copia segura para buffer alinhado de `i16` evitando UB caso `GetBuffer` retorne ponteiro nao alinhado.
                copy_unaligned_i16_bytes(data_ptr, sample_count, &mut pcm_scratch);
                proc.process_i16_stereo_48k(
                    &pcm_scratch[..sample_count],
                    Instant::now(),
                    &mut on_frame,
                );
            }

            let _ = unsafe { capture.ReleaseBuffer(frames) };
        }
    }

    let _ = unsafe { client.Stop() };
    Ok(())
}

#[inline]
unsafe fn copy_unaligned_i16_bytes_raw(src: *const u8, sample_count: usize, dst: &mut Vec<i16>) {
    if dst.len() < sample_count {
        dst.resize(sample_count, 0);
    }
    if sample_count > 0 && !src.is_null() {
        std::ptr::copy_nonoverlapping(src, dst.as_mut_ptr() as *mut u8, sample_count * 2);
    }
}

fn copy_unaligned_i16_bytes(src: *const u8, sample_count: usize, dst: &mut Vec<i16>) {
    unsafe { copy_unaligned_i16_bytes_raw(src, sample_count, dst) }
}

fn run_cpal_endpoint_loopback(
    stop_flag: &AtomicBool,
    on_frame: impl FnMut(CapturedSystemAudioFrame) + Send + 'static,
) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "Nenhum dispositivo de saida para loopback de audio.".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("Falha ao consultar formato de saida para loopback: {}", e))?;

    let channels = supported.channels() as usize;
    let in_rate = supported.sample_rate().0;
    let mut proc = SystemAudioProcessor::new(in_rate, channels, 0);
    let mut cb_prio: Option<AudioThreadPriorityGuard> = None;
    let on_frame_shared = Arc::new(Mutex::new(on_frame));

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let emit = Arc::clone(&on_frame_shared);
            device.build_input_stream(
                &supported.into(),
                move |data: &[f32], _| {
                    cb_prio.get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(false));
                    if let Ok(mut f) = emit.lock() {
                        proc.process_interleaved_f32(data, Instant::now(), &mut *f);
                    }
                },
                |_| {},
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let emit = Arc::clone(&on_frame_shared);
            device.build_input_stream(
                &supported.into(),
                move |data: &[i16], _| {
                    cb_prio.get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(false));
                    if let Ok(mut f) = emit.lock() {
                        convert_and_feed(&mut proc, data, |s| s as f32 / 32768.0, &mut *f);
                    }
                },
                |_| {},
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let emit = Arc::clone(&on_frame_shared);
            device.build_input_stream(
                &supported.into(),
                move |data: &[u16], _| {
                    cb_prio.get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(false));
                    if let Ok(mut f) = emit.lock() {
                        convert_and_feed(
                            &mut proc,
                            data,
                            |s| (s as f32 - 32768.0) / 32768.0,
                            &mut *f,
                        );
                    }
                },
                |_| {},
                None,
            )
        }
        cpal::SampleFormat::I32 => {
            let emit = Arc::clone(&on_frame_shared);
            device.build_input_stream(
                &supported.into(),
                move |data: &[i32], _| {
                    cb_prio.get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(false));
                    if let Ok(mut f) = emit.lock() {
                        convert_and_feed(
                            &mut proc,
                            data,
                            |s| (s as f64 / 2_147_483_648.0) as f32,
                            &mut *f,
                        );
                    }
                },
                |_| {},
                None,
            )
        }
        other => {
            return Err(format!("Formato de loopback nao suportado: {:?}", other));
        }
    }
    .map_err(|e| format!("Falha ao abrir loopback de sistema: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("Falha ao iniciar loopback de sistema: {}", e))?;

    while !stop_flag.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(stream);
    Ok(())
}

fn convert_and_feed<T: Copy>(
    proc: &mut SystemAudioProcessor,
    data: &[T],
    convert: impl Fn(T) -> f32,
    mut emit: impl FnMut(CapturedSystemAudioFrame),
) {
    let ch = proc.channels.max(1);
    let mut tmp = [0.0f32; 2048];
    let step_elems = ((tmp.len() / ch) * ch).max(ch);
    let mut idx = 0;
    while idx < data.len() {
        let chunk = (data.len() - idx).min(step_elems);
        for i in 0..chunk {
            tmp[i] = convert(data[idx + i]);
        }
        proc.process_interleaved_f32(&tmp[..chunk], Instant::now(), &mut emit);
        idx += chunk;
    }
}

/// Inicia captura dedicada do audio de sistema/janela em alta prioridade, codificando Opus estereo fora do Tokio.
pub fn spawn_system_audio_worker(
    slot: String,
    mode: SystemAudioMode,
    stop_flag: Arc<AtomicBool>,
    audio_bus: AudioFanoutBus,
) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(format!("golive-sysaudio-{}", slot))
        .spawn(move || {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let com_init = hr.0 == 0 || hr.0 == 1;
            let _prio = AudioThreadPriorityGuard::enter_pro_audio(false);

            let mut enc_opt = OpusStereoAudioEncoder::new().ok();
            let slot_enc = slot.clone();
            let bus_enc = audio_bus.clone();

            let mut encode_and_send = move |frame: CapturedSystemAudioFrame| {
                if !bus_enc.has_targets(&slot_enc) {
                    return;
                }
                if enc_opt.is_none() {
                    enc_opt = OpusStereoAudioEncoder::new().ok();
                }
                let Some(ref mut enc) = enc_opt else {
                    return;
                };
                if let Some(encoded) = enc.encode_frame(&frame.samples) {
                    bus_enc.send_encoded_audio(
                        &slot_enc,
                        encoded,
                        frame.frame_seq,
                        frame.talkspurt_start,
                    );
                }
            };

            crate::diag_info!("sysaudio", "worker iniciado slot={} mode={:?}", slot, mode);
            let res = try_run_process_loopback(mode, &stop_flag, &mut encode_and_send);
            if let Err(ref err) = res {
                if !stop_flag.load(Ordering::Relaxed) {
                    crate::diag_warn!(
                        "sysaudio",
                        "Process Loopback falhou no slot={} ({}); tentando fallback endpoint",
                        slot,
                        err
                    );
                    // Fallback para loopback do dispositivo de saida no Windows 10 antigo
                    let slot_fb = slot.clone();
                    let bus_fb = audio_bus;
                    let mut fb_enc = OpusStereoAudioEncoder::new().ok();
                    if let Err(fb_err) = run_cpal_endpoint_loopback(&stop_flag, move |frame| {
                        if !bus_fb.has_targets(&slot_fb) {
                            return;
                        }
                        if fb_enc.is_none() {
                            fb_enc = OpusStereoAudioEncoder::new().ok();
                        }
                        if let Some(ref mut enc) = fb_enc {
                            if let Some(encoded) = enc.encode_frame(&frame.samples) {
                                bus_fb.send_encoded_audio(
                                    &slot_fb,
                                    encoded,
                                    frame.frame_seq,
                                    frame.talkspurt_start,
                                );
                            }
                        }
                    }) {
                        crate::diag_warn!("sysaudio", "fallback endpoint loopback falhou slot={}: {}", slot, fb_err);
                    }
                }
            }
            crate::diag_info!("sysaudio", "worker encerrado slot={}", slot);

            if com_init {
                unsafe { CoUninitialize() };
            }
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_propvariant_blob_matches_windows_propvariant_size() {
        assert_eq!(
            size_of::<PropVariantBlob>(),
            size_of::<windows::core::PROPVARIANT>()
        );
        let blob = PropVariantBlob {
            vt: VT_BLOB_U16,
            w_reserved1: 0,
            w_reserved2: 0,
            w_reserved3: 0,
            cb_size: 16,
            #[cfg(target_pointer_width = "64")]
            _pad: 0,
            p_blob_data: null_mut(),
            #[cfg(target_pointer_width = "32")]
            _pad_tail: 0,
        };
        assert_eq!(blob.vt, 65);
        assert_eq!(blob.cb_size, 16);
    }

    #[test]
    fn test_system_audio_processor_resamples_stereo_and_suppresses_pure_silence() {
        let mut proc = SystemAudioProcessor::new(44_100, 2, 0);
        let t0 = Instant::now();
        let mut out = Vec::new();
        let mut sample_idx = 0usize;

        // 100 ms de som suave a 44.1 kHz estereo (10 callbacks de 441 frames)
        for cb in 0..10u64 {
            let mut buf = Vec::with_capacity(441 * 2);
            for _ in 0..441 {
                let l = ((sample_idx as f32) * 2.0 * std::f32::consts::PI * 440.0 / 44_100.0).sin()
                    * 0.02;
                let r = ((sample_idx as f32) * 2.0 * std::f32::consts::PI * 550.0 / 44_100.0).sin()
                    * 0.02;
                buf.push(l);
                buf.push(r);
                sample_idx += 1;
            }
            proc.process_interleaved_f32(&buf, t0 + Duration::from_millis(cb * 10), |f| out.push(f));
        }

        assert!(
            out.len() >= 4,
            "quiet audio must be preserved cleanly, got {} frames",
            out.len()
        );
        assert!(out[0].talkspurt_start);
        assert!(!out[1].talkspurt_start);

        // 35 frames de silencio digital puro @ 48 kHz direto -> primeiros 25 emitem, depois pausa
        let mut proc48 = SystemAudioProcessor::new(48_000, 2, 0);
        let mut out48 = Vec::new();
        let tone = vec![500i16; 1920];
        let zeros = vec![0i16; 1920];

        proc48.process_i16_stereo_48k(&tone, t0, |f| out48.push(f));
        for s in 1..36u64 {
            proc48.process_i16_stereo_48k(&zeros, t0 + Duration::from_millis(s * 20), |f| {
                out48.push(f)
            });
        }
        assert_eq!(out48.len(), 1 + SILENCE_HOLD_FRAMES as usize);
        assert_eq!(out48.last().unwrap().frame_seq, SILENCE_HOLD_FRAMES as u64);

        // Som retorna no frame 36 -> frame_seq == 36 e talkspurt_start == true
        proc48.process_i16_stereo_48k(&tone, t0 + Duration::from_millis(36 * 20), |f| {
            out48.push(f)
        });
        let resumed = out48.last().unwrap();
        assert_eq!(resumed.frame_seq, 36);
        assert!(resumed.talkspurt_start);
    }

    #[test]
    fn test_copy_unaligned_i16_bytes_into_processor() {
        // Simula ponteiro desalinhado (offset +1 byte) retornado por drivers de audio.
        let sample_vals: Vec<i16> = (0..1920).map(|i| ((i % 200) - 100) * 100).collect();
        let mut raw = vec![0u8; 1 + sample_vals.len() * 2];
        for (i, s) in sample_vals.iter().enumerate() {
            let bytes = s.to_le_bytes();
            raw[1 + i * 2] = bytes[0];
            raw[1 + i * 2 + 1] = bytes[1];
        }
        let unaligned_ptr = unsafe { raw.as_ptr().add(1) };
        assert_eq!((unaligned_ptr as usize) % 2, 1);

        let mut scratch = Vec::new();
        copy_unaligned_i16_bytes(unaligned_ptr, sample_vals.len(), &mut scratch);
        assert_eq!(&scratch[..sample_vals.len()], sample_vals.as_slice());

        let mut proc48 = SystemAudioProcessor::new(48_000, 2, 0);
        let mut emitted = Vec::new();
        proc48.process_i16_stereo_48k(&scratch[..sample_vals.len()], Instant::now(), |f| {
            emitted.push(f);
        });
        assert_eq!(emitted.len(), 1);
    }

    #[test]
    fn test_classify_wasapi_get_buffer_handles_s_buffer_empty_success_hresult() {
        // Documenta o comportamento critico do Windows/windows-rs:
        // AUDCLNT_S_BUFFER_EMPTY (0x08890001) possui bit de sucesso (.is_ok() == true),
        // entao .ok() no binding gerava loop infinito quando frames == 0.
        assert!(AUDCLNT_S_BUFFER_EMPTY.is_ok());
        assert_eq!(
            classify_wasapi_get_buffer(AUDCLNT_S_BUFFER_EMPTY, 0),
            WasapiBufferPoll::Empty
        );
        assert_eq!(
            classify_wasapi_get_buffer(AUDCLNT_S_BUFFER_EMPTY, 480),
            WasapiBufferPoll::Empty
        );
        assert_eq!(
            classify_wasapi_get_buffer(S_OK_HR, 0),
            WasapiBufferPoll::ZeroFrames
        );
        assert_eq!(
            classify_wasapi_get_buffer(S_OK_HR, 480),
            WasapiBufferPoll::Data
        );
        assert_eq!(
            classify_wasapi_get_buffer(E_UNEXPECTED_HR, 0),
            WasapiBufferPoll::Error(E_UNEXPECTED_HR)
        );
    }
}
