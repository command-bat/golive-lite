use crate::media::audio::AudioThreadPriorityGuard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MIXER_CAPACITY_SAMPLES: usize = 9_600; // max 1s @ 48kHz por peer para manter memoria minuscula

struct PeerAudioQueue {
    samples: VecDeque<i16>,
    volume: f32,
    muted: bool,
    last_activity_ms: u64,
    speaking: bool,
}

impl PeerAudioQueue {
    fn new_for_key(source_key: &str) -> Self {
        Self {
            samples: VecDeque::with_capacity(1920),
            volume: 1.0,
            // Audio de transmissoes ("screen:peer", "camera:peer") inicia mutado por padrao como no GoLive Web.
            muted: source_key.contains(':'),
            last_activity_ms: 0,
            speaking: false,
        }
    }
}

/// Mixador de audio de saida com ganho individual por participante (0% a 200%) e Master Deafen.
pub struct AudioMixer {
    queues: Arc<Mutex<HashMap<String, PeerAudioQueue>>>,
    pub deafened: Arc<AtomicBool>,
    stop_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    out_sample_rate: Arc<Mutex<u32>>,
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
            deafened: Arc::new(AtomicBool::new(false)),
            stop_tx: Mutex::new(None),
            out_sample_rate: Arc::new(Mutex::new(48_000)),
        }
    }

    pub fn list_output_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn start(&self, device_name: Option<&str>) -> Result<(), String> {
        self.stop();

        let wanted_name = device_name.map(|s| s.to_string());
        let queues = self.queues.clone();
        let deafened = self.deafened.clone();
        let out_sample_rate = self.out_sample_rate.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

        std::thread::Builder::new()
            .name("golive-audio-out".to_string())
            .spawn(move || {
                let host = cpal::default_host();
                let device = if let Some(ref wanted) = wanted_name {
                    host.output_devices()
                        .ok()
                        .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(wanted)))
                        .or_else(|| host.default_output_device())
                } else {
                    host.default_output_device()
                };

                let Some(device) = device else {
                    let _ = ready_tx.send(Err("Nenhum dispositivo de saida de audio encontrado.".to_string()));
                    return;
                };

                let supported = match device.default_output_config() {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("Falha ao configurar saida de audio: {}", e)));
                        return;
                    }
                };

                let channels = supported.channels() as usize;
                let out_rate = supported.sample_rate().0;
                if let Ok(mut r) = out_sample_rate.lock() {
                    *r = out_rate;
                }

                // Fator de conversao 48kHz -> out_rate
                let ratio = 48_000.0f64 / (out_rate as f64);
                let mut frac: f64 = 0.0;
                let mut last_mixed_sample: f32 = 0.0;
                let mut priority_guard: Option<AudioThreadPriorityGuard> = None;

                let stream_res = match supported.sample_format() {
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        &supported.into(),
                        move |output: &mut [f32], _| {
                            priority_guard.get_or_insert_with(|| {
                                AudioThreadPriorityGuard::enter_pro_audio(false)
                            });
                            fill_output_f32(
                                output,
                                channels,
                                ratio,
                                &mut frac,
                                &mut last_mixed_sample,
                                &queues,
                                &deafened,
                            );
                        },
                        |_| {},
                        None,
                    ),
                    cpal::SampleFormat::I16 => device.build_output_stream(
                        &supported.into(),
                        move |output: &mut [i16], _| {
                            priority_guard.get_or_insert_with(|| {
                                AudioThreadPriorityGuard::enter_pro_audio(false)
                            });
                            let mut tmp = [0.0f32; 2048];
                            let mut idx = 0;
                            while idx < output.len() {
                                let chunk = (output.len() - idx).min(tmp.len());
                                fill_output_f32(
                                    &mut tmp[..chunk],
                                    channels,
                                    ratio,
                                    &mut frac,
                                    &mut last_mixed_sample,
                                    &queues,
                                    &deafened,
                                );
                                for i in 0..chunk {
                                    output[idx + i] = (tmp[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                                }
                                idx += chunk;
                            }
                        },
                        |_| {},
                        None,
                    ),
                    other => {
                        let _ = ready_tx.send(Err(format!(
                            "Formato de audio de saida nao suportado: {:?}",
                            other
                        )));
                        return;
                    }
                };

                let stream = match stream_res {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("Falha ao abrir dispositivo de saida: {}", e)));
                        return;
                    }
                };

                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(format!("Falha ao iniciar playback: {}", e)));
                    return;
                }

                let _ = ready_tx.send(Ok(()));
                // Mantem cpal::Stream vivo nesta thread ate stop() ou queda de AudioMixer.
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|e| format!("Falha ao iniciar thread de audio: {}", e))?;

        match ready_rx.recv() {
            Ok(Ok(())) => {
                if let Ok(mut tx_slot) = self.stop_tx.lock() {
                    *tx_slot = Some(stop_tx);
                }
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Thread de audio encerrou inesperadamente.".to_string()),
        }
    }

    pub fn stop(&self) {
        if let Ok(mut tx_slot) = self.stop_tx.lock() {
            if let Some(tx) = tx_slot.take() {
                let _ = tx.send(());
            }
        }
    }

    pub fn push_samples(&self, source_key: &str, samples: &[i16], now_ms: u64) {
        if let Ok(mut map) = self.queues.lock() {
            let entry = map
                .entry(source_key.to_string())
                .or_insert_with(|| PeerAudioQueue::new_for_key(source_key));
            entry.last_activity_ms = now_ms;
            entry.speaking = !samples.is_empty();
            if self.deafened.load(Ordering::Relaxed) || entry.muted {
                entry.samples.clear();
                return;
            }
            if entry.samples.len() + samples.len() > MIXER_CAPACITY_SAMPLES {
                let drop_n = (entry.samples.len() + samples.len()) - MIXER_CAPACITY_SAMPLES;
                entry.samples.drain(..drop_n.min(entry.samples.len()));
            }
            entry.samples.extend(samples);
        }
    }

    pub fn ensure_source_defaults(&self, source_key: &str, volume: f32, muted: bool) {
        if let Ok(mut map) = self.queues.lock() {
            map.entry(source_key.to_string()).or_insert_with(|| {
                let mut q = PeerAudioQueue::new_for_key(source_key);
                q.volume = volume.clamp(0.0, 2.0);
                q.muted = muted;
                q
            });
        }
    }

    pub fn set_source_volume(&self, source_key: &str, volume: f32) {
        if let Ok(mut map) = self.queues.lock() {
            let entry = map
                .entry(source_key.to_string())
                .or_insert_with(|| PeerAudioQueue::new_for_key(source_key));
            entry.volume = volume.clamp(0.0, 2.0);
        }
    }

    pub fn set_source_muted(&self, source_key: &str, muted: bool) {
        if let Ok(mut map) = self.queues.lock() {
            let entry = map
                .entry(source_key.to_string())
                .or_insert_with(|| PeerAudioQueue::new_for_key(source_key));
            entry.muted = muted;
            if muted {
                entry.samples.clear();
            }
        }
    }

    pub fn is_peer_speaking(&self, source_key: &str, now_ms: u64) -> bool {
        if let Ok(map) = self.queues.lock() {
            if let Some(q) = map.get(source_key) {
                return q.speaking && now_ms.saturating_sub(q.last_activity_ms) < 400;
            }
        }
        false
    }

    pub fn remove_source(&self, source_key: &str) {
        if let Ok(mut map) = self.queues.lock() {
            map.remove(source_key);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.queues.lock() {
            map.clear();
        }
    }
}

fn fill_output_f32(
    output: &mut [f32],
    channels: usize,
    ratio: f64,
    frac: &mut f64,
    last_sample: &mut f32,
    queues: &Arc<Mutex<HashMap<String, PeerAudioQueue>>>,
    deafened: &Arc<AtomicBool>,
) {
    if deafened.load(Ordering::Relaxed) {
        output.fill(0.0);
        return;
    }

    let frames = output.len() / channels.max(1);
    let mut guard_opt = None;
    for _ in 0..4 {
        if let Ok(g) = queues.try_lock() {
            guard_opt = Some(g);
            break;
        }
        std::hint::spin_loop();
    }
    let Some(mut guard) = guard_opt else {
        for f in 0..frames {
            *last_sample *= 0.92;
            for c in 0..channels {
                output[f * channels + c] = *last_sample;
            }
        }
        return;
    };

    for f in 0..frames {
        *frac += ratio;
        let advance = *frac as usize;
        *frac -= advance as f64;

        if advance > 0 {
            let mut sum = 0.0f32;
            for queue in guard.values_mut() {
                if queue.muted || queue.samples.is_empty() {
                    continue;
                }
                let mut popped = 0i16;
                for _ in 0..advance {
                    if let Some(s) = queue.samples.pop_front() {
                        popped = s;
                    } else {
                        break;
                    }
                }
                sum += (popped as f32 / 32768.0) * queue.volume;
            }
            // Soft-clipping leve para evitar distorcao quando multiplos participantes falam juntos
            *last_sample = sum.tanh();
        }

        for c in 0..channels {
            output[f * channels + c] = *last_sample;
        }
    }
}
