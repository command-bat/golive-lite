use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST, THREAD_PRIORITY_TIME_CRITICAL,
};

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_FRAME_SAMPLES: usize = 960; // 20 ms @ 48 kHz
const HPF_ALPHA: f32 = 0.9896; // ~80 Hz 1-pole high-pass @ 48 kHz
const VAD_HANGOVER_FRAMES: u8 = 25; // 500 ms hangover apos parar de falar
const PREROLL_FRAMES: usize = 2; // 40 ms pre-roll para nao cortar inicio de palavras
const AVRT_PRIORITY_HIGH: i32 = 1;
const AVRT_PRIORITY_CRITICAL: i32 = 2;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(lp_lib_file_name: *const u8) -> isize;
    fn GetProcAddress(h_module: isize, lp_proc_name: *const u8) -> *const c_void;
    fn FreeLibrary(h_lib_module: isize) -> i32;
}

/// Eleva a thread atual para prioridade de audio em tempo real (Win32 + MMCSS "Pro Audio").
pub(crate) struct AudioThreadPriorityGuard {
    avrt_mod: isize,
    task_handle: isize,
}

impl AudioThreadPriorityGuard {
    pub(crate) fn enter_pro_audio(time_critical: bool) -> Self {
        unsafe {
            let prio = if time_critical {
                THREAD_PRIORITY_TIME_CRITICAL
            } else {
                THREAD_PRIORITY_HIGHEST
            };
            let _ = SetThreadPriority(GetCurrentThread(), prio);
        }

        let avrt_mod = unsafe { LoadLibraryA(b"avrt.dll\0".as_ptr()) };
        if avrt_mod == 0 {
            return Self {
                avrt_mod: 0,
                task_handle: 0,
            };
        }

        let set_chars_sym =
            unsafe { GetProcAddress(avrt_mod, b"AvSetMmThreadCharacteristicsW\0".as_ptr()) };
        if set_chars_sym.is_null() {
            unsafe {
                let _ = FreeLibrary(avrt_mod);
            }
            return Self {
                avrt_mod: 0,
                task_handle: 0,
            };
        }

        let task_name: [u16; 10] = [
            b'P' as u16,
            b'r' as u16,
            b'o' as u16,
            b' ' as u16,
            b'A' as u16,
            b'u' as u16,
            b'd' as u16,
            b'i' as u16,
            b'o' as u16,
            0,
        ];
        let mut task_idx = 0u32;
        let set_chars_fn: unsafe extern "system" fn(*const u16, *mut u32) -> isize =
            unsafe { std::mem::transmute(set_chars_sym) };
        let task_handle = unsafe { set_chars_fn(task_name.as_ptr(), &mut task_idx) };

        if task_handle != 0 {
            let set_prio_sym =
                unsafe { GetProcAddress(avrt_mod, b"AvSetMmThreadPriority\0".as_ptr()) };
            if !set_prio_sym.is_null() {
                let set_prio_fn: unsafe extern "system" fn(isize, i32) -> i32 =
                    unsafe { std::mem::transmute(set_prio_sym) };
                let avrt_prio = if time_critical {
                    AVRT_PRIORITY_CRITICAL
                } else {
                    AVRT_PRIORITY_HIGH
                };
                unsafe {
                    let _ = set_prio_fn(task_handle, avrt_prio);
                }
            }
        }

        Self {
            avrt_mod,
            task_handle,
        }
    }
}

impl Drop for AudioThreadPriorityGuard {
    fn drop(&mut self) {
        if self.avrt_mod != 0 {
            if self.task_handle != 0 {
                let sym = unsafe {
                    GetProcAddress(self.avrt_mod, b"AvRevertMmThreadCharacteristics\0".as_ptr())
                };
                if !sym.is_null() {
                    let func: unsafe extern "system" fn(isize) -> i32 =
                        unsafe { std::mem::transmute(sym) };
                    unsafe {
                        let _ = func(self.task_handle);
                    }
                }
            }
            unsafe {
                let _ = FreeLibrary(self.avrt_mod);
            }
        }
    }
}

/// Frame de audio capturado pronto para codificacao Opus (PCM 48 kHz mono 20 ms = 960 samples).
#[derive(Debug, Clone)]
pub struct CapturedAudioFrame {
    pub samples: [i16; AUDIO_FRAME_SAMPLES],
    #[allow(dead_code)]
    pub sample_rate: u32,
    pub frame_seq: u64,
    pub talkspurt_start: bool,
}

#[derive(Clone)]
pub struct MicState {
    pub enabled: Arc<AtomicBool>,
    pub speaking: Arc<AtomicBool>,
    /// Ganho multiplicador armazenado como bits f32 (0.01 ..= 2.0)
    pub gain_bits: Arc<AtomicU32>,
    pub vad_threshold_bits: Arc<AtomicU32>,
    pub push_to_talk: Arc<AtomicBool>,
    pub ptt_vk: Arc<AtomicU32>,
    /// Relogio monotonico de frames (20 ms cada) preservado entre reinicios de dispositivo.
    pub frame_seq_clock: Arc<AtomicU64>,
}

impl Default for MicState {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            speaking: Arc::new(AtomicBool::new(false)),
            gain_bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            vad_threshold_bits: Arc::new(AtomicU32::new(0.004f32.to_bits())),
            push_to_talk: Arc::new(AtomicBool::new(false)),
            ptt_vk: Arc::new(AtomicU32::new(0x56)),
            frame_seq_clock: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl MicState {
    pub fn set_gain(&self, gain: f32) {
        self.gain_bits
            .store(gain.clamp(0.01, 2.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_vad_threshold(&self, thresh: f32) {
        self.vad_threshold_bits
            .store(thresh.clamp(0.001, 0.25).to_bits(), Ordering::Relaxed);
    }
}

pub struct MicCapture {
    pub state: MicState,
    stream: Arc<Mutex<Option<cpal::Stream>>>,
    pub selected_device: Arc<Mutex<Option<String>>>,
    pub last_error: Arc<Mutex<Option<String>>>,
    frame_tx: mpsc::SyncSender<CapturedAudioFrame>,
}

impl MicCapture {
    pub fn new(frame_tx: mpsc::SyncSender<CapturedAudioFrame>) -> Self {
        Self {
            state: MicState::default(),
            stream: Arc::new(Mutex::new(None)),
            selected_device: Arc::new(Mutex::new(None)),
            last_error: Arc::new(Mutex::new(None)),
            frame_tx,
        }
    }

    pub fn list_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn start(&self, device_name: Option<&str>) -> Result<(), String> {
        self.stop();

        let host = cpal::default_host();
        let device = if let Some(wanted) = device_name {
            host.input_devices()
                .ok()
                .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(wanted)))
                .or_else(|| host.default_input_device())
        } else {
            host.default_input_device()
        }
        .ok_or_else(|| "Nenhum microfone encontrado neste dispositivo.".to_string())?;

        let supported = device
            .default_input_config()
            .map_err(|e| format!("Falha ao obter configuracao do microfone: {}", e))?;

        let channels = supported.channels() as usize;
        let in_rate = supported.sample_rate().0;
        let state = self.state.clone();
        let tx = self.frame_tx.clone();
        let err_slot = self.last_error.clone();
        let initial_seq = state.frame_seq_clock.load(Ordering::Relaxed);
        let mut proc = MicAudioProcessor::new(in_rate, channels, initial_seq);
        let mut priority_guard: Option<AudioThreadPriorityGuard> = None;

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &supported.into(),
                move |data: &[f32], _| {
                    priority_guard
                        .get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(true));
                    process_cpal_chunk(&mut proc, data, &state, &tx);
                },
                move |err| {
                    if let Ok(mut slot) = err_slot.lock() {
                        *slot = Some(format!("Erro de audio: {}", err));
                    }
                },
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &supported.into(),
                move |data: &[i16], _| {
                    priority_guard
                        .get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(true));
                    process_cpal_converted(
                        &mut proc,
                        data,
                        |s| s as f32 / 32768.0,
                        &state,
                        &tx,
                    );
                },
                move |err| {
                    if let Ok(mut slot) = err_slot.lock() {
                        *slot = Some(format!("Erro de audio: {}", err));
                    }
                },
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &supported.into(),
                move |data: &[u16], _| {
                    priority_guard
                        .get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(true));
                    process_cpal_converted(
                        &mut proc,
                        data,
                        |s| (s as f32 - 32768.0) / 32768.0,
                        &state,
                        &tx,
                    );
                },
                move |err| {
                    if let Ok(mut slot) = err_slot.lock() {
                        *slot = Some(format!("Erro de audio: {}", err));
                    }
                },
                None,
            ),
            cpal::SampleFormat::I32 => device.build_input_stream(
                &supported.into(),
                move |data: &[i32], _| {
                    priority_guard
                        .get_or_insert_with(|| AudioThreadPriorityGuard::enter_pro_audio(true));
                    process_cpal_converted(
                        &mut proc,
                        data,
                        |s| (s as f64 / 2_147_483_648.0) as f32,
                        &state,
                        &tx,
                    );
                },
                move |err| {
                    if let Ok(mut slot) = err_slot.lock() {
                        *slot = Some(format!("Erro de audio: {}", err));
                    }
                },
                None,
            ),
            other => {
                return Err(format!("Formato de microfone nao suportado: {:?}", other));
            }
        }
        .map_err(|e| format!("Nao foi possivel abrir o microfone: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("Falha ao iniciar captura do microfone: {}", e))?;

        self.state.enabled.store(true, Ordering::Relaxed);
        if let Ok(mut slot) = self.stream.lock() {
            *slot = Some(stream);
        }
        if let Ok(mut err) = self.last_error.lock() {
            *err = None;
        }
        Ok(())
    }

    pub fn stop(&self) {
        self.state.enabled.store(false, Ordering::Relaxed);
        self.state.speaking.store(false, Ordering::Relaxed);
        if let Ok(mut slot) = self.stream.lock() {
            *slot = None;
        }
    }
}

fn process_cpal_chunk(
    proc: &mut MicAudioProcessor,
    data: &[f32],
    state: &MicState,
    tx: &mpsc::SyncSender<CapturedAudioFrame>,
) {
    let enabled = state.enabled.load(Ordering::Relaxed);
    let push_to_talk = state.push_to_talk.load(Ordering::Relaxed);
    let ptt_active = if push_to_talk {
        let vk = state.ptt_vk.load(Ordering::Relaxed) as i32;
        is_vk_pressed(vk)
    } else {
        true
    };
    let gain = f32::from_bits(state.gain_bits.load(Ordering::Relaxed));
    let vad_thresh = f32::from_bits(state.vad_threshold_bits.load(Ordering::Relaxed));

    let speaking_now = proc.process_interleaved_f32(
        data,
        Instant::now(),
        enabled,
        push_to_talk,
        ptt_active,
        gain,
        vad_thresh,
        |frame| {
            let _ = tx.try_send(frame);
        },
    );
    state.speaking.store(speaking_now, Ordering::Relaxed);
    state
        .frame_seq_clock
        .store(proc.frame_seq, Ordering::Relaxed);
}

fn process_cpal_converted<T: Copy>(
    proc: &mut MicAudioProcessor,
    data: &[T],
    convert: impl Fn(T) -> f32,
    state: &MicState,
    tx: &mpsc::SyncSender<CapturedAudioFrame>,
) {
    let channels = proc.channels.max(1);
    let mut tmp = [0.0f32; 2048];
    let usable_cap = (tmp.len() / channels) * channels;
    let step_elems = usable_cap.max(channels);
    let mut idx = 0;
    while idx < data.len() {
        let chunk = (data.len() - idx).min(step_elems);
        for i in 0..chunk {
            tmp[i] = convert(data[idx + i]);
        }
        process_cpal_chunk(proc, &tmp[..chunk], state, tx);
        idx += chunk;
    }
}

/// Processador DSP zero-alloc para microfone (48 kHz mono, 20 ms/frame, pre-roll + hangover + sequencia monotonica).
pub struct MicAudioProcessor {
    in_rate: u32,
    channels: usize,
    step: f64,
    phase: f64,
    prev_mono: f32,
    has_prev: bool,
    hpf_prev_x: f32,
    hpf_prev_y: f32,
    accum: [i16; AUDIO_FRAME_SAMPLES],
    accum_len: usize,
    preroll: [[i16; AUDIO_FRAME_SAMPLES]; PREROLL_FRAMES],
    preroll_seqs: [u64; PREROLL_FRAMES],
    preroll_write: usize,
    preroll_len: usize,
    in_talkspurt: bool,
    hangover_frames: u8,
    pub frame_seq: u64,
    force_talkspurt_marker: bool,
    last_callback_at: Option<Instant>,
}

impl MicAudioProcessor {
    pub fn new(in_rate: u32, channels: usize, initial_seq: u64) -> Self {
        let safe_in_rate = in_rate.max(8_000);
        Self {
            in_rate: safe_in_rate,
            channels: channels.max(1),
            step: (safe_in_rate as f64) / (AUDIO_SAMPLE_RATE as f64),
            phase: 0.0,
            prev_mono: 0.0,
            has_prev: false,
            hpf_prev_x: 0.0,
            hpf_prev_y: 0.0,
            accum: [0i16; AUDIO_FRAME_SAMPLES],
            accum_len: 0,
            preroll: [[0i16; AUDIO_FRAME_SAMPLES]; PREROLL_FRAMES],
            preroll_seqs: [0u64; PREROLL_FRAMES],
            preroll_write: 0,
            preroll_len: 0,
            in_talkspurt: false,
            hangover_frames: 0,
            frame_seq: initial_seq,
            force_talkspurt_marker: true,
            last_callback_at: None,
        }
    }

    #[inline]
    fn mono_at(data: &[f32], channels: usize, frame_idx: usize) -> f32 {
        let base = frame_idx * channels;
        if channels == 1 {
            return data[base];
        }
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += data[base + c];
        }
        sum / (channels as f32)
    }

    #[inline]
    fn apply_voice_filter_and_limit(&mut self, sample: f32, gain: f32) -> i16 {
        // Filtro passa-alta 1-pole (~80 Hz) remove DC offset e ruido subgrave que engana o VAD
        let hp = HPF_ALPHA * (self.hpf_prev_y + sample - self.hpf_prev_x);
        self.hpf_prev_x = sample;
        self.hpf_prev_y = hp;

        let g = hp * gain;
        let abs_g = g.abs();
        let limited = if abs_g <= 0.85 {
            g
        } else {
            g.signum() * (0.85 + 0.15 * ((abs_g - 0.85) / 0.15).tanh())
        };
        (limited.clamp(-1.0, 1.0) * 32767.0).round() as i16
    }

    fn push_preroll(&mut self, seq: u64) {
        self.preroll[self.preroll_write] = self.accum;
        self.preroll_seqs[self.preroll_write] = seq;
        self.preroll_write = (self.preroll_write + 1) % PREROLL_FRAMES;
        if self.preroll_len < PREROLL_FRAMES {
            self.preroll_len += 1;
        }
    }

    fn drain_preroll(&mut self, mut emit: impl FnMut(CapturedAudioFrame)) -> bool {
        if self.preroll_len == 0 {
            return false;
        }
        let start = (self.preroll_write + PREROLL_FRAMES - self.preroll_len) % PREROLL_FRAMES;
        let count = self.preroll_len;
        self.preroll_len = 0;
        for k in 0..count {
            let slot = (start + k) % PREROLL_FRAMES;
            let is_first = k == 0;
            emit(CapturedAudioFrame {
                samples: self.preroll[slot],
                sample_rate: AUDIO_SAMPLE_RATE,
                frame_seq: self.preroll_seqs[slot],
                talkspurt_start: is_first || self.force_talkspurt_marker,
            });
            self.force_talkspurt_marker = false;
        }
        true
    }

    fn on_frame_ready(
        &mut self,
        enabled: bool,
        push_to_talk: bool,
        ptt_active: bool,
        vad_thresh: f32,
        mut emit: impl FnMut(CapturedAudioFrame),
    ) {
        let seq = self.frame_seq;
        self.frame_seq = self.frame_seq.wrapping_add(1);

        if !enabled {
            self.in_talkspurt = false;
            self.hangover_frames = 0;
            self.preroll_len = 0;
            self.force_talkspurt_marker = true;
            return;
        }

        if push_to_talk {
            if ptt_active {
                let is_start = !self.in_talkspurt || self.force_talkspurt_marker;
                self.in_talkspurt = true;
                self.hangover_frames = 0;
                self.preroll_len = 0;
                self.force_talkspurt_marker = false;
                emit(CapturedAudioFrame {
                    samples: self.accum,
                    sample_rate: AUDIO_SAMPLE_RATE,
                    frame_seq: seq,
                    talkspurt_start: is_start,
                });
            } else if self.in_talkspurt {
                let mut faded = self.accum;
                apply_linear_fade_out(&mut faded);
                self.in_talkspurt = false;
                self.hangover_frames = 0;
                self.preroll_len = 0;
                self.force_talkspurt_marker = true;
                emit(CapturedAudioFrame {
                    samples: faded,
                    sample_rate: AUDIO_SAMPLE_RATE,
                    frame_seq: seq,
                    talkspurt_start: false,
                });
            }
            return;
        }

        // Modo VAD com histerese, pre-roll (40ms) e fade-out na cauda
        let mut sum_sq = 0.0f64;
        for &s in self.accum.iter() {
            let f = s as f64 / 32768.0;
            sum_sq += f * f;
        }
        let rms = (sum_sq / (AUDIO_FRAME_SAMPLES as f64)).sqrt() as f32;

        if !self.in_talkspurt {
            if rms >= vad_thresh {
                let emitted_preroll = self.drain_preroll(&mut emit);
                let is_start = !emitted_preroll || self.force_talkspurt_marker;
                self.force_talkspurt_marker = false;
                self.in_talkspurt = true;
                self.hangover_frames = VAD_HANGOVER_FRAMES;
                emit(CapturedAudioFrame {
                    samples: self.accum,
                    sample_rate: AUDIO_SAMPLE_RATE,
                    frame_seq: seq,
                    talkspurt_start: is_start,
                });
            } else {
                self.push_preroll(seq);
            }
        } else if rms >= vad_thresh * 0.65 {
            self.hangover_frames = VAD_HANGOVER_FRAMES;
            let is_start = self.force_talkspurt_marker;
            self.force_talkspurt_marker = false;
            emit(CapturedAudioFrame {
                samples: self.accum,
                sample_rate: AUDIO_SAMPLE_RATE,
                frame_seq: seq,
                talkspurt_start: is_start,
            });
        } else if self.hangover_frames > 1 {
            self.hangover_frames -= 1;
            let is_start = self.force_talkspurt_marker;
            self.force_talkspurt_marker = false;
            emit(CapturedAudioFrame {
                samples: self.accum,
                sample_rate: AUDIO_SAMPLE_RATE,
                frame_seq: seq,
                talkspurt_start: is_start,
            });
        } else {
            // Ultimo frame de hangover: fade-out suave ate zero para evitar clique
            let mut faded = self.accum;
            apply_linear_fade_out(&mut faded);
            let is_start = self.force_talkspurt_marker;
            self.force_talkspurt_marker = true;
            self.hangover_frames = 0;
            self.in_talkspurt = false;
            self.preroll_len = 0;
            emit(CapturedAudioFrame {
                samples: faded,
                sample_rate: AUDIO_SAMPLE_RATE,
                frame_seq: seq,
                talkspurt_start: is_start,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_interleaved_f32(
        &mut self,
        data: &[f32],
        now: Instant,
        enabled: bool,
        push_to_talk: bool,
        ptt_active: bool,
        gain: f32,
        vad_thresh: f32,
        mut emit: impl FnMut(CapturedAudioFrame),
    ) -> bool {
        if let Some(prev_t) = self.last_callback_at {
            let gap_ms = now.saturating_duration_since(prev_t).as_millis() as u64;
            if gap_ms > 100 {
                let missed = (gap_ms / 20).saturating_sub(1);
                if missed > 0 {
                    self.frame_seq = self.frame_seq.wrapping_add(missed);
                    self.accum_len = 0;
                    self.preroll_len = 0;
                    self.in_talkspurt = false;
                    self.hangover_frames = 0;
                    self.force_talkspurt_marker = true;
                }
            }
        }
        self.last_callback_at = Some(now);

        let ch = self.channels;
        let frame_count = data.len() / ch;
        if frame_count == 0 {
            return self.in_talkspurt && enabled;
        }

        if self.in_rate == AUDIO_SAMPLE_RATE {
            for i in 0..frame_count {
                let mono = Self::mono_at(data, ch, i);
                self.prev_mono = mono;
                self.has_prev = true;
                let s = if enabled && (!push_to_talk || ptt_active) {
                    self.apply_voice_filter_and_limit(mono, gain)
                } else {
                    0
                };
                self.accum[self.accum_len] = s;
                self.accum_len += 1;
                if self.accum_len >= AUDIO_FRAME_SAMPLES {
                    self.accum_len = 0;
                    self.on_frame_ready(enabled, push_to_talk, ptt_active, vad_thresh, &mut emit);
                }
            }
        } else {
            let first_mono = Self::mono_at(data, ch, 0);
            if !self.has_prev {
                self.prev_mono = first_mono;
                self.has_prev = true;
                self.phase = 0.0;
            }
            let limit = (frame_count as f64) - 1.0;
            while self.phase < limit {
                let mono = if self.phase < 0.0 {
                    let t = (self.phase + 1.0) as f32;
                    self.prev_mono + (first_mono - self.prev_mono) * t
                } else {
                    let idx = (self.phase.floor() as usize).min(frame_count.saturating_sub(2));
                    let t = (self.phase - (idx as f64)) as f32;
                    let s0 = Self::mono_at(data, ch, idx);
                    let s1 = Self::mono_at(data, ch, idx + 1);
                    s0 + (s1 - s0) * t
                };
                let s = if enabled && (!push_to_talk || ptt_active) {
                    self.apply_voice_filter_and_limit(mono, gain)
                } else {
                    0
                };
                self.accum[self.accum_len] = s;
                self.accum_len += 1;
                if self.accum_len >= AUDIO_FRAME_SAMPLES {
                    self.accum_len = 0;
                    self.on_frame_ready(enabled, push_to_talk, ptt_active, vad_thresh, &mut emit);
                }
                self.phase += self.step;
            }
            self.prev_mono = Self::mono_at(data, ch, frame_count - 1);
            self.phase -= frame_count as f64;
            if self.phase < -1.0 {
                self.phase = -1.0;
            }
        }

        self.in_talkspurt && enabled
    }
}

#[inline]
fn apply_linear_fade_out(buf: &mut [i16; AUDIO_FRAME_SAMPLES]) {
    for (i, s) in buf.iter_mut().enumerate() {
        let factor = (AUDIO_FRAME_SAMPLES - i) as f32 / (AUDIO_FRAME_SAMPLES as f32);
        *s = ((*s as f32) * factor).round() as i16;
    }
}

fn is_vk_pressed(vk: i32) -> bool {
    unsafe {
        (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) as u16 & 0x8000) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_resampler_44100_to_48000_linear_continuity() {
        let mut proc = MicAudioProcessor::new(44_100, 2, 0);
        let base_t = Instant::now();
        let mut emitted = Vec::new();
        let mut sample_idx = 0usize;

        // Alimenta 500 ms em callbacks de 10 ms (441 frames estereo @ 44.1 kHz)
        for cb in 0..50 {
            let mut chunk = Vec::with_capacity(441 * 2);
            for _ in 0..441 {
                let phase = (sample_idx as f32) * 2.0 * std::f32::consts::PI * 440.0 / 44_100.0;
                let v = phase.sin() * 0.4;
                chunk.push(v);
                chunk.push(v);
                sample_idx += 1;
            }
            proc.process_interleaved_f32(
                &chunk,
                base_t + Duration::from_millis(cb * 10),
                true,
                false,
                true,
                1.0,
                0.001,
                |f| emitted.push(f),
            );
        }

        // 500 ms @ 20 ms/frame -> 24 ou 25 frames completos
        assert!(
            (24..=25).contains(&emitted.len()),
            "expected 24..=25 frames in 500ms, got {}",
            emitted.len()
        );

        // Ignora primeiro frame de transiente do HPF; garante continuidade suave nas fronteiras entre callbacks/frames
        let max_adj_delta_per_sample = (2.0 * std::f32::consts::PI * 440.0 / 48_000.0) * 0.4 * 32767.0;
        let allowed_jump = (max_adj_delta_per_sample * 1.6) as i32;
        for w in emitted[1..].windows(2) {
            let boundary_jump = (w[1].samples[0] as i32 - w[0].samples[AUDIO_FRAME_SAMPLES - 1] as i32).abs();
            assert!(
                boundary_jump <= allowed_jump,
                "boundary jump {} exceeds smooth sine delta {}",
                boundary_jump,
                allowed_jump
            );
        }
    }

    #[test]
    fn test_resampler_48000_fast_path_passthrough() {
        let mut proc = MicAudioProcessor::new(48_000, 1, 100);
        let base_t = Instant::now();
        let mut emitted = Vec::new();
        let mut buf = Vec::with_capacity(960 * 5);
        for i in 0..(960 * 5) {
            let phase = (i as f32) * 2.0 * std::f32::consts::PI * 1000.0 / 48_000.0;
            buf.push(phase.sin() * 0.35);
        }
        proc.process_interleaved_f32(&buf, base_t, true, false, true, 1.0, 0.004, |f| {
            emitted.push(f);
        });
        assert_eq!(emitted.len(), 5);
        assert_eq!(emitted[0].frame_seq, 100);
        assert_eq!(emitted[4].frame_seq, 104);
        assert!(emitted[0].talkspurt_start);
        assert!(!emitted[1].talkspurt_start);
    }

    #[test]
    fn test_vad_preroll_hangover_fadeout_and_monotonic_seq() {
        let mut proc = MicAudioProcessor::new(48_000, 1, 0);
        let base_t = Instant::now();
        let mut emitted = Vec::new();
        let silence = vec![0.0f32; 960];
        let speech: Vec<f32> = (0..960)
            .map(|i| ((i as f32) * 2.0 * std::f32::consts::PI * 300.0 / 48_000.0).sin() * 0.2)
            .collect();

        // 5 frames de silencio inicial (seqs 0..=4): nenhum emitido
        for step in 0..5u64 {
            proc.process_interleaved_f32(
                &silence,
                base_t + Duration::from_millis(step * 20),
                true,
                false,
                true,
                1.0,
                0.04,
                |f| emitted.push(f),
            );
        }
        assert!(emitted.is_empty(), "initial silence must not emit packets");

        // 1 frame de fala (seq 5): deve emitir 2 frames de pre-roll (seqs 3 e 4) + fala (seq 5)
        proc.process_interleaved_f32(
            &speech,
            base_t + Duration::from_millis(100),
            true,
            false,
            true,
            1.0,
            0.04,
            |f| emitted.push(f),
        );
        assert_eq!(emitted.len(), 3);
        assert_eq!(emitted[0].frame_seq, 3);
        assert!(emitted[0].talkspurt_start);
        assert_eq!(emitted[1].frame_seq, 4);
        assert!(!emitted[1].talkspurt_start);
        assert_eq!(emitted[2].frame_seq, 5);
        assert!(!emitted[2].talkspurt_start);

        // 35 frames de silencio: hangover dura 25 frames (seqs 6..=30), ultimo com fade-out, depois para (seqs 31..=40)
        for step in 6..41u64 {
            proc.process_interleaved_f32(
                &silence,
                base_t + Duration::from_millis(step * 20),
                true,
                false,
                true,
                1.0,
                0.04,
                |f| emitted.push(f),
            );
        }
        assert_eq!(emitted.len(), 3 + 25);
        assert_eq!(emitted.last().unwrap().frame_seq, 30);

        // Nova fala em seq 41: pre-roll deve puxar seqs 39 e 40 com talkspurt_start=true no primeiro!
        let before = emitted.len();
        proc.process_interleaved_f32(
            &speech,
            base_t + Duration::from_millis(41 * 20),
            true,
            false,
            true,
            1.0,
            0.04,
            |f| emitted.push(f),
        );
        assert_eq!(emitted.len() - before, 3);
        assert_eq!(emitted[before].frame_seq, 39);
        assert!(emitted[before].talkspurt_start);
        assert_eq!(emitted[before + 1].frame_seq, 40);
        assert!(!emitted[before + 1].talkspurt_start);
        assert_eq!(emitted[before + 2].frame_seq, 41);
        assert!(!emitted[before + 2].talkspurt_start);
    }

    #[test]
    fn test_ptt_emits_cleanly_and_advances_seq_when_released() {
        let mut proc = MicAudioProcessor::new(48_000, 1, 0);
        let base_t = Instant::now();
        let mut emitted = Vec::new();
        let tone: Vec<f32> = (0..960)
            .map(|i| ((i as f32) * 2.0 * std::f32::consts::PI * 400.0 / 48_000.0).sin() * 0.25)
            .collect();

        // Solto por 3 frames (0, 1, 2): nao emite
        for s in 0..3u64 {
            proc.process_interleaved_f32(
                &tone,
                base_t + Duration::from_millis(s * 20),
                true,
                true,
                false,
                1.0,
                0.01,
                |f| emitted.push(f),
            );
        }
        assert!(emitted.is_empty());

        // Pressionado por 2 frames (3, 4)
        for s in 3..5u64 {
            proc.process_interleaved_f32(
                &tone,
                base_t + Duration::from_millis(s * 20),
                true,
                true,
                true,
                1.0,
                0.01,
                |f| emitted.push(f),
            );
        }
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].frame_seq, 3);
        assert!(emitted[0].talkspurt_start);
        assert_eq!(emitted[1].frame_seq, 4);
        assert!(!emitted[1].talkspurt_start);

        // Solto no frame 5 -> 1 frame de fade-out (seq 5), depois para (6, 7)
        for s in 5..8u64 {
            proc.process_interleaved_f32(
                &tone,
                base_t + Duration::from_millis(s * 20),
                true,
                true,
                false,
                1.0,
                0.01,
                |f| emitted.push(f),
            );
        }
        assert_eq!(emitted.len(), 3);
        assert_eq!(emitted[2].frame_seq, 5);
        assert!(!emitted[2].talkspurt_start);
        assert_eq!(emitted[2].samples[AUDIO_FRAME_SAMPLES - 1].abs(), 0);

        // Pressionado de novo no frame 8 -> seq 8, talkspurt_start=true
        proc.process_interleaved_f32(
            &tone,
            base_t + Duration::from_millis(8 * 20),
            true,
            true,
            true,
            1.0,
            0.01,
            |f| emitted.push(f),
        );
        assert_eq!(emitted.len(), 4);
        assert_eq!(emitted[3].frame_seq, 8);
        assert!(emitted[3].talkspurt_start);
    }
}
