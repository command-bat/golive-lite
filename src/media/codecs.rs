use bytes::Bytes;
use openh264::decoder::{Decoder as OpenH264Decoder, DecoderConfig};
use openh264::encoder::{
    BitRate, Complexity, Encoder as OpenH264Encoder, EncoderConfig, FrameRate, IntraFramePeriod,
    Level, Profile, RateControlMode, SpsPpsStrategy, UsageType, VuiConfig,
};
use openh264::formats::{BgraSliceU8, RgbaSliceU8, YUVBuffer, YUVSource};
use openh264::{nal_units, OpenH264API};
use ropus::{
    Application, Bitrate, Channels, DecodeMode, Decoder as OpusDecoder, Encoder as OpusEncoder,
    FrameDuration, InbandFec, Signal,
};
use std::collections::HashSet;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::rtp::packetizer::Depacketizer;

const MAX_AU_BYTES: usize = 4 * 1024 * 1024; // 4 MB para suportar keyframes 4K sem descartar

fn find_annex_b_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    let len = data.len();
    if from + 3 > len {
        return None;
    }
    let mut i = from;
    while i + 2 < len {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                return Some((i, 3));
            }
            if i + 3 < len && data[i + 2] == 0 && data[i + 3] == 1 {
                return Some((i, 4));
            }
        }
        i += 1;
    }
    None
}

fn split_annex_b_nalus(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let Some((mut pos, mut sc_len)) = find_annex_b_start_code(data, 0) else {
        let trimmed = data;
        if !trimmed.is_empty() {
            out.push(trimmed);
        }
        return out;
    };
    while pos + sc_len <= data.len() {
        let nalu_start = pos + sc_len;
        match find_annex_b_start_code(data, nalu_start) {
            Some((next_pos, next_sc)) => {
                if next_pos > nalu_start {
                    out.push(&data[nalu_start..next_pos]);
                }
                pos = next_pos;
                sc_len = next_sc;
            }
            None => {
                if nalu_start < data.len() {
                    out.push(&data[nalu_start..]);
                }
                break;
            }
        }
    }
    out
}

/// Informacoes estruturais extraidas de uma Access Unit H.264 Annex-B para validacao e diagnostico.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct H264AuInfo {
    pub has_sps: bool,
    pub has_pps: bool,
    pub has_idr: bool,
    pub has_non_idr_slice: bool,
    pub has_b_slice: bool,
    pub has_sei: bool,
    pub has_aud: bool,
    pub profile_idc: Option<u8>,
    pub constraint_flags: Option<u8>,
    pub level_idc: Option<u8>,
    pub slice_types: Vec<u8>,
}

fn nal_payload_to_rbsp_prefix(payload: &[u8], max_rbsp_bytes: usize) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(payload.len().min(max_rbsp_bytes));
    let mut zeros = 0u8;
    for &b in payload {
        if rbsp.len() >= max_rbsp_bytes {
            break;
        }
        if zeros >= 2 && b == 0x03 {
            zeros = 0;
            continue;
        }
        rbsp.push(b);
        if b == 0x00 {
            zeros = zeros.saturating_add(1);
        } else {
            zeros = 0;
        }
    }
    rbsp
}

struct RbspBitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> RbspBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.byte_pos)?;
        let bit = (byte >> (7 - self.bit_pos)) & 1;
        if self.bit_pos == 7 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        } else {
            self.bit_pos += 1;
        }
        Some(bit)
    }

    fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut val = 0u32;
        for _ in 0..count {
            val = (val << 1) | (self.read_bit()? as u32);
        }
        Some(val)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zeros = 0u32;
        while self.read_bit()? == 0 {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return None;
            }
        }
        if leading_zeros == 0 {
            return Some(0);
        }
        let suffix = self.read_bits(leading_zeros as u8)?;
        Some(((1u32 << leading_zeros) - 1).saturating_add(suffix))
    }
}

/// Inspeciona uma Access Unit H.264 Annex-B (SPS profile/constraint/level e tipos de slice P/B/I).
pub fn inspect_h264_au(annex_b: &[u8]) -> H264AuInfo {
    let mut info = H264AuInfo::default();
    for nalu in split_annex_b_nalus(annex_b) {
        if nalu.is_empty() {
            continue;
        }
        let nalu_type = nalu[0] & 0x1F;
        match nalu_type {
            7 => {
                info.has_sps = true;
                if nalu.len() > 1 {
                    let rbsp = nal_payload_to_rbsp_prefix(&nalu[1..], 8);
                    if rbsp.len() >= 3 && info.profile_idc.is_none() {
                        info.profile_idc = Some(rbsp[0]);
                        info.constraint_flags = Some(rbsp[1]);
                        info.level_idc = Some(rbsp[2]);
                    }
                }
            }
            8 => {
                info.has_pps = true;
            }
            6 => {
                info.has_sei = true;
            }
            9 => {
                info.has_aud = true;
            }
            1 | 5 => {
                if nalu_type == 5 {
                    info.has_idr = true;
                } else {
                    info.has_non_idr_slice = true;
                }
                if nalu.len() > 1 {
                    let rbsp = nal_payload_to_rbsp_prefix(&nalu[1..], 24);
                    let mut reader = RbspBitReader::new(&rbsp);
                    if reader.read_ue().is_some() {
                        if let Some(st) = reader.read_ue() {
                            let st_u8 = st.min(u8::MAX as u32) as u8;
                            info.slice_types.push(st_u8);
                            if (st % 5) == 1 {
                                info.has_b_slice = true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    info
}

/// Faz cache de SPS (tipo 7) e PPS (tipo 8) do fluxo H.264 Annex-B e garante que todo IDR (tipo 5)
/// carregue SPS+PPS imediatamente antes, removendo SEI/AUD/Filler para maxima compatibilidade WebRTC.
#[derive(Default)]
pub struct H264ParameterSets {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl H264ParameterSets {
    pub fn new() -> Self {
        Self {
            sps: None,
            pps: None,
        }
    }

    pub fn normalize_access_unit(&mut self, raw_annex_b: &[u8]) -> Option<(Bytes, bool)> {
        self.normalize_access_unit_inner(raw_annex_b, false)
    }

    /// Normaliza uma Access Unit em passada unica e descarta B-slices inline sem alocar/re-escanear via `inspect_h264_au`.
    pub fn normalize_access_unit_no_b_slices(&mut self, raw_annex_b: &[u8]) -> Option<(Bytes, bool)> {
        self.normalize_access_unit_inner(raw_annex_b, true)
    }

    fn normalize_access_unit_inner(
        &mut self,
        raw_annex_b: &[u8],
        reject_b_slices: bool,
    ) -> Option<(Bytes, bool)> {
        let nalus = split_annex_b_nalus(raw_annex_b);
        if nalus.is_empty() {
            return None;
        }

        let mut is_idr = false;
        let mut has_vcl = false;
        let mut kept: Vec<&[u8]> = Vec::with_capacity(nalus.len());

        for nalu in nalus {
            if nalu.is_empty() {
                continue;
            }
            let nalu_type = nalu[0] & 0x1F;
            match nalu_type {
                7 => {
                    self.sps = Some(nalu.to_vec());
                    kept.push(nalu);
                }
                8 => {
                    self.pps = Some(nalu.to_vec());
                    kept.push(nalu);
                }
                6 | 9 | 12 => {
                    // Remove SEI (6), AUD (9) e Filler (12) para manter STAP-A/RTP enxuto e compativel com browsers
                }
                5 => {
                    is_idr = true;
                    has_vcl = true;
                    kept.push(nalu);
                }
                1..=4 => {
                    if reject_b_slices && nalu_type == 1 && nalu.len() > 1 {
                        let rbsp = nal_payload_to_rbsp_prefix(&nalu[1..], 24);
                        let mut reader = RbspBitReader::new(&rbsp);
                        if reader.read_ue().is_some() {
                            if let Some(st) = reader.read_ue() {
                                if (st % 5) == 1 {
                                    return None;
                                }
                            }
                        }
                    }
                    has_vcl = true;
                    kept.push(nalu);
                }
                _ => kept.push(nalu),
            }
        }

        // Nunca emite Access Units compostos apenas por SPS/PPS/SEI sem slice VCL
        if kept.is_empty() || !has_vcl {
            return None;
        }

        let mut est = raw_annex_b.len() + 16;
        if is_idr {
            if let Some(ref sps) = self.sps {
                est += 4 + sps.len();
            }
            if let Some(ref pps) = self.pps {
                est += 4 + pps.len();
            }
        }

        let mut out = Vec::with_capacity(est);
        if is_idr {
            if let Some(ref sps) = self.sps {
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(sps);
            }
            if let Some(ref pps) = self.pps {
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(pps);
            }
            for nalu in kept {
                let nalu_type = nalu[0] & 0x1F;
                if nalu_type == 7 || nalu_type == 8 {
                    continue;
                }
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(nalu);
            }
        } else {
            for nalu in kept {
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(nalu);
            }
        }

        if out.is_empty() {
            None
        } else {
            Some((Bytes::from(out), is_idr))
        }
    }
}

/// Codificador Opus 48 kHz mono (20 ms = 960 samples) para voz WebRTC padrao.
pub struct OpusVoiceEncoder {
    inner: OpusEncoder,
    out_buf: Vec<u8>,
}

impl OpusVoiceEncoder {
    pub fn new() -> Result<Self, String> {
        let inner = OpusEncoder::builder(48_000, Channels::Mono, Application::Voip)
            .bitrate(Bitrate::Bits(40_000))
            .complexity(5)
            .signal(Signal::Voice)
            .frame_duration(FrameDuration::Ms20)
            .inband_fec(InbandFec::Enabled)
            .packet_loss_perc(15)
            .vbr(true)
            .vbr_constraint(true)
            .build()
            .map_err(|e| format!("Falha ao iniciar encoder Opus: {}", e))?;

        Ok(Self {
            inner,
            out_buf: vec![0u8; 4000],
        })
    }

    pub fn complexity(&self) -> u8 {
        self.inner.complexity()
    }

    pub fn packet_loss_perc(&self) -> u8 {
        self.inner.packet_loss_perc()
    }

    pub fn encode_frame(&mut self, pcm_960: &[i16]) -> Option<Bytes> {
        if pcm_960.len() != 960 {
            return None;
        }
        let len = self.inner.encode(pcm_960, &mut self.out_buf).ok()?;
        Some(Bytes::copy_from_slice(&self.out_buf[..len]))
    }
}

/// Codificador Opus 48 kHz estereo (20 ms = 960 frames = 1920 amostras intercaladas) para audio de tela/janela.
pub struct OpusStereoAudioEncoder {
    inner: OpusEncoder,
    out_buf: Vec<u8>,
}

impl OpusStereoAudioEncoder {
    pub fn new() -> Result<Self, String> {
        let inner = OpusEncoder::builder(48_000, Channels::Stereo, Application::Audio)
            .bitrate(Bitrate::Bits(96_000))
            .complexity(5)
            .frame_duration(FrameDuration::Ms20)
            .inband_fec(InbandFec::Enabled)
            .packet_loss_perc(10)
            .vbr(true)
            .vbr_constraint(true)
            .build()
            .map_err(|e| format!("Falha ao iniciar encoder Opus estereo: {}", e))?;

        Ok(Self {
            inner,
            out_buf: vec![0u8; 4000],
        })
    }

    pub fn encode_frame(&mut self, pcm_stereo_1920: &[i16]) -> Option<Bytes> {
        if pcm_stereo_1920.len() != 1920 {
            return None;
        }
        let len = self
            .inner
            .encode(pcm_stereo_1920, &mut self.out_buf)
            .ok()?;
        Some(Bytes::copy_from_slice(&self.out_buf[..len]))
    }
}

/// Decodificador Opus 48 kHz estereo (compatibiliza pacotes mono e estereo dos navegadores) com saida mono i16.
pub struct OpusAudioDecoder {
    inner: OpusDecoder,
    interleaved_scratch: Vec<i16>,
}

impl OpusAudioDecoder {
    pub fn new() -> Result<Self, String> {
        let inner = OpusDecoder::new(48_000, Channels::Stereo)
            .map_err(|e| format!("Falha ao iniciar decoder Opus: {}", e))?;
        let max_per_ch = inner.max_frame_samples_per_channel();
        Ok(Self {
            inner,
            interleaved_scratch: vec![0i16; max_per_ch * 2],
        })
    }

    pub fn decode_packet(&mut self, payload: &[u8], out_mono: &mut Vec<i16>) -> bool {
        if payload.is_empty() {
            return false;
        }
        let Ok(samples_per_ch) =
            self.inner
                .decode(payload, &mut self.interleaved_scratch, DecodeMode::Normal)
        else {
            return false;
        };
        if samples_per_ch == 0 {
            return false;
        }
        out_mono.clear();
        out_mono.reserve(samples_per_ch);
        for i in 0..samples_per_ch {
            let l = self.interleaved_scratch[i * 2] as i32;
            let r = self.interleaved_scratch[i * 2 + 1] as i32;
            out_mono.push(((l + r) / 2) as i16);
        }
        true
    }
}

/// Codificador H.264 Baseline em Anexo-B (OpenH264) com reutilizacao de YUVBuffer e deteccao automatica de configuracao.
pub struct H264VideoEncoder {
    encoder: Option<OpenH264Encoder>,
    yuv_buf: Option<YUVBuffer>,
    yuv_dims: (usize, usize),
    configured_bitrate_bps: u32,
    configured_fps: u32,
    configured_is_screen: bool,
    param_sets: H264ParameterSets,
}

impl H264VideoEncoder {
    pub fn new(is_screen: bool) -> Self {
        Self {
            encoder: None,
            yuv_buf: None,
            yuv_dims: (0, 0),
            configured_bitrate_bps: 0,
            configured_fps: 0,
            configured_is_screen: is_screen,
            param_sets: H264ParameterSets::new(),
        }
    }

    fn ensure_encoder(
        &mut self,
        bitrate_bps: u32,
        fps: u32,
        is_screen: bool,
    ) -> Result<&mut OpenH264Encoder, String> {
        let clamped_fps = fps.clamp(5, 240);
        let clamped_bps = bitrate_bps.clamp(150_000, 32_000_000);
        let needs_rebuild = self.encoder.is_none()
            || self.configured_bitrate_bps != clamped_bps
            || self.configured_fps != clamped_fps
            || self.configured_is_screen != is_screen;

        if needs_rebuild {
            let gop_frames = clamped_fps.saturating_mul(2).clamp(30, 480);
            let threads = std::thread::available_parallelism()
                .map(|n| n.get().clamp(1, 4) as u16)
                .unwrap_or(2);
            let cfg = EncoderConfig::new()
                .bitrate(BitRate::from_bps(clamped_bps))
                .max_frame_rate(FrameRate::from_hz(clamped_fps as f32))
                .rate_control_mode(RateControlMode::Bitrate)
                .usage_type(UsageType::CameraVideoRealTime)
                .profile(Profile::Baseline)
                .level(Level::Level_5_1)
                .sps_pps_strategy(SpsPpsStrategy::ConstantId)
                .skip_frames(false)
                .complexity(Complexity::Low)
                .adaptive_quantization(false)
                .background_detection(false)
                .scene_change_detect(false)
                .num_threads(threads)
                .intra_frame_period(IntraFramePeriod::from_num_frames(gop_frames))
                .vui(VuiConfig::bt601())
                .debug(false);

            let enc = OpenH264Encoder::with_api_config(OpenH264API::from_source(), cfg)
                .map_err(|e| format!("Falha ao criar encoder OpenH264: {:?}", e))?;
            self.encoder = Some(enc);
            self.configured_bitrate_bps = clamped_bps;
            self.configured_fps = clamped_fps;
            self.configured_is_screen = is_screen;
            self.param_sets = H264ParameterSets::new();
        }

        Ok(self.encoder.as_mut().expect("encoder just initialized"))
    }

    fn encode_current_yuv(
        &mut self,
        bitrate_bps: u32,
        fps: u32,
        force_keyframe: bool,
    ) -> Option<(Bytes, bool)> {
        let is_screen = self.configured_is_screen;
        {
            let enc = self.ensure_encoder(bitrate_bps, fps, is_screen).ok()?;
            if force_keyframe {
                enc.force_intra_frame();
            }
        }

        let yuv_ref = self.yuv_buf.as_ref()?;
        let enc_mut = self.encoder.as_mut()?;
        let bs = enc_mut.encode(yuv_ref).ok()?;
        let encoded = bs.to_vec();
        if encoded.is_empty() {
            None
        } else {
            self.param_sets.normalize_access_unit(&encoded)
        }
    }

    /// Converte RGBA (dimensoes pares) em YUV420 e retorna `(annex_b, is_keyframe)`.
    pub fn encode_rgba_with_keyframe(
        &mut self,
        rgba: &[u8],
        width: usize,
        height: usize,
        bitrate_bps: u32,
        fps: u32,
        force_keyframe: bool,
    ) -> Option<(Bytes, bool)> {
        if width < 16 || height < 16 || width % 2 != 0 || height % 2 != 0 {
            return None;
        }
        if rgba.len() != width * height * 4 {
            return None;
        }

        if self.yuv_dims != (width, height) || self.yuv_buf.is_none() {
            self.yuv_buf = Some(YUVBuffer::new(width, height));
            self.yuv_dims = (width, height);
            self.param_sets = H264ParameterSets::new();
        }
        let yuv = self.yuv_buf.as_mut()?;
        yuv.read_rgba8(RgbaSliceU8::new(rgba, (width, height)));

        self.encode_current_yuv(bitrate_bps, fps, force_keyframe)
    }

    /// Converte RGBA (dimensoes pares) em YUV420 reutilizando buffer e retorna quadro H.264 Annex-B.
    pub fn encode_rgba(
        &mut self,
        rgba: &[u8],
        width: usize,
        height: usize,
        bitrate_bps: u32,
        fps: u32,
        force_keyframe: bool,
    ) -> Option<Bytes> {
        self.encode_rgba_with_keyframe(rgba, width, height, bitrate_bps, fps, force_keyframe)
            .map(|(au, _)| au)
    }

    /// Converte BGRA nativo do Windows diretamente em YUV420 e retorna `(annex_b, is_keyframe)`.
    pub fn encode_bgra_with_keyframe(
        &mut self,
        bgra: &[u8],
        width: usize,
        height: usize,
        bitrate_bps: u32,
        fps: u32,
        force_keyframe: bool,
    ) -> Option<(Bytes, bool)> {
        if width < 16 || height < 16 || width % 2 != 0 || height % 2 != 0 {
            return None;
        }
        if bgra.len() != width * height * 4 {
            return None;
        }

        if self.yuv_dims != (width, height) || self.yuv_buf.is_none() {
            self.yuv_buf = Some(YUVBuffer::new(width, height));
            self.yuv_dims = (width, height);
            self.param_sets = H264ParameterSets::new();
        }
        let yuv = self.yuv_buf.as_mut()?;
        yuv.read_bgra8(BgraSliceU8::new(bgra, (width, height)));

        self.encode_current_yuv(bitrate_bps, fps, force_keyframe)
    }

    /// Converte BGRA nativo do Windows diretamente em YUV420 sem copia para RGBA e retorna quadro H.264 Annex-B.
    pub fn encode_bgra(
        &mut self,
        bgra: &[u8],
        width: usize,
        height: usize,
        bitrate_bps: u32,
        fps: u32,
        force_keyframe: bool,
    ) -> Option<Bytes> {
        self.encode_bgra_with_keyframe(bgra, width, height, bitrate_bps, fps, force_keyframe)
            .map(|(au, _)| au)
    }
}

/// Decodificador H.264 Annex-B (OpenH264) com conversao direta YUV->Color32 em buffer reutilizado.
pub struct H264VideoDecoder {
    decoder: OpenH264Decoder,
    x_map_luma: Vec<usize>,
    x_map_uv: Vec<usize>,
    x_frac: Vec<u16>,
    cached_dims: (usize, usize, usize, usize),
}

impl H264VideoDecoder {
    pub fn new() -> Result<Self, String> {
        let cfg = DecoderConfig::new().debug(false);
        let decoder = OpenH264Decoder::with_api_config(OpenH264API::from_source(), cfg)
            .map_err(|e| format!("Falha ao iniciar decoder H.264: {:?}", e))?;
        Ok(Self {
            decoder,
            x_map_luma: Vec::new(),
            x_map_uv: Vec::new(),
            x_frac: Vec::new(),
            cached_dims: (0, 0, 0, 0),
        })
    }

    pub fn decode_annex_b(
        &mut self,
        access_unit: &[u8],
        out_rgba: &mut Vec<u8>,
    ) -> Result<Option<(usize, usize)>, String> {
        let mut last_dims: Option<(usize, usize)> = None;
        for nal in nal_units(access_unit) {
            if nal.is_empty() {
                continue;
            }
            match self.decoder.decode(nal) {
                Ok(Some(yuv)) => {
                    let (w, h) = yuv.dimensions();
                    if w > 0 && h > 0 {
                        let needed = w * h * 4;
                        if out_rgba.len() != needed {
                            out_rgba.resize(needed, 255);
                        }
                        yuv.write_rgba8(out_rgba);
                        last_dims = Some((w, h));
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(format!("Erro de decodificacao H.264: {:?}", e));
                }
            }
        }
        Ok(last_dims)
    }

    #[inline(always)]
    fn yuv_offsets(u: i32, v: i32, bt709: bool) -> (i32, i32, i32) {
        let d = u - 128;
        let e = v - 128;
        if bt709 {
            (459 * e, -55 * d - 136 * e, 541 * d)
        } else {
            (409 * e, -100 * d - 208 * e, 516 * d)
        }
    }

    #[inline(always)]
    fn color_from_y_offsets(y: i32, cr: i32, cg: i32, cb: i32) -> egui::Color32 {
        let c298 = 298 * (y - 16) + 128;
        let r = ((c298 + cr) >> 8).clamp(0, 255) as u8;
        let g = ((c298 + cg) >> 8).clamp(0, 255) as u8;
        let b = ((c298 + cb) >> 8).clamp(0, 255) as u8;
        egui::Color32::from_rgb(r, g, b)
    }

    /// Decodifica um Access Unit apenas para manter a cadeia de referencias (P-frames/IDR) sem converter YUV->Color32.
    pub fn decode_annex_b_reference_only(&mut self, access_unit: &[u8]) -> Result<(), String> {
        for nal in nal_units(access_unit) {
            if nal.is_empty() {
                continue;
            }
            self.decoder
                .decode(nal)
                .map_err(|e| format!("Falha decode H.264 ref: {:?}", e))?;
        }
        Ok(())
    }

    /// Decodifica Annex-B e converte YUV420p diretamente para `Vec<egui::Color32>`
    /// (caminho nativo 1:1 2x2 e downscale bilinear com BT.709 para HD+ / BT.601 para SD).
    /// Retorna `Ok(Some((orig_w, orig_h, dst_w, dst_h)))`.
    pub fn decode_annex_b_to_color32(
        &mut self,
        access_unit: &[u8],
        max_width: usize,
        max_height: usize,
        out_pixels: &mut Vec<egui::Color32>,
    ) -> Result<Option<(usize, usize, usize, usize)>, String> {
        let mut last_dims: Option<(usize, usize, usize, usize)> = None;
        for nal in nal_units(access_unit) {
            if nal.is_empty() {
                continue;
            }
            match self.decoder.decode(nal) {
                Ok(Some(yuv)) => {
                    let (orig_w, orig_h) = yuv.dimensions();
                    if orig_w == 0 || orig_h == 0 {
                        continue;
                    }
                    let (dst_w, dst_h) = compute_scaled_dims(orig_w, orig_h, max_width, max_height);
                    let needed = dst_w * dst_h;
                    out_pixels.clear();
                    out_pixels.resize(needed, egui::Color32::BLACK);

                    let (y_stride, u_stride, v_stride) = yuv.strides();
                    let y_plane = yuv.y();
                    let u_plane = yuv.u();
                    let v_plane = yuv.v();
                    let bt709 = orig_h >= 720;

                    let uv_w = (orig_w + 1) >> 1;
                    let uv_h = (orig_h + 1) >> 1;
                    let slices_ok = y_stride >= orig_w
                        && u_stride >= uv_w
                        && v_stride >= uv_w
                        && y_plane.len() >= (orig_h - 1) * y_stride + orig_w
                        && u_plane.len() >= (uv_h - 1) * u_stride + uv_w
                        && v_plane.len() >= (uv_h - 1) * v_stride + uv_w;

                    if dst_w == orig_w && dst_h == orig_h && slices_ok {
                        // Caminho nativo 1:1 (blocos 2x2 compartilhando croma -> ~2.5x menos operacoes)
                        let mut by = 0usize;
                        while by + 1 < orig_h {
                            let y_row0 = &y_plane[by * y_stride..by * y_stride + orig_w];
                            let y_row1 = &y_plane[(by + 1) * y_stride..(by + 1) * y_stride + orig_w];
                            let uv_y = by >> 1;
                            let u_row = &u_plane[uv_y * u_stride..uv_y * u_stride + uv_w];
                            let v_row = &v_plane[uv_y * v_stride..uv_y * v_stride + uv_w];
                            let (dst0, dst1) = out_pixels[by * dst_w..(by + 2) * dst_w].split_at_mut(dst_w);

                            let pairs = orig_w >> 1;
                            for uv_x in 0..pairs {
                                let bx = uv_x << 1;
                                let (cr, cg, cb) = Self::yuv_offsets(u_row[uv_x] as i32, v_row[uv_x] as i32, bt709);
                                dst0[bx] = Self::color_from_y_offsets(y_row0[bx] as i32, cr, cg, cb);
                                dst0[bx + 1] = Self::color_from_y_offsets(y_row0[bx + 1] as i32, cr, cg, cb);
                                dst1[bx] = Self::color_from_y_offsets(y_row1[bx] as i32, cr, cg, cb);
                                dst1[bx + 1] = Self::color_from_y_offsets(y_row1[bx + 1] as i32, cr, cg, cb);
                            }
                            if (orig_w & 1) != 0 {
                                let bx = orig_w - 1;
                                let uv_x = bx >> 1;
                                let (cr, cg, cb) = Self::yuv_offsets(u_row[uv_x] as i32, v_row[uv_x] as i32, bt709);
                                dst0[bx] = Self::color_from_y_offsets(y_row0[bx] as i32, cr, cg, cb);
                                dst1[bx] = Self::color_from_y_offsets(y_row1[bx] as i32, cr, cg, cb);
                            }
                            by += 2;
                        }
                        if by < orig_h {
                            let y_row = &y_plane[by * y_stride..by * y_stride + orig_w];
                            let uv_y = by >> 1;
                            let u_row = &u_plane[uv_y * u_stride..uv_y * u_stride + uv_w];
                            let v_row = &v_plane[uv_y * v_stride..uv_y * v_stride + uv_w];
                            let dst_row = &mut out_pixels[by * dst_w..(by + 1) * dst_w];
                            for bx in 0..orig_w {
                                let uv_x = bx >> 1;
                                let (cr, cg, cb) = Self::yuv_offsets(u_row[uv_x] as i32, v_row[uv_x] as i32, bt709);
                                dst_row[bx] = Self::color_from_y_offsets(y_row[bx] as i32, cr, cg, cb);
                            }
                        }
                    } else if slices_ok {
                        // Downscale bilinear 2x2 na luminancia (texto nitido e sem serrilhado em tiles)
                        let key = (orig_w, orig_h, dst_w, dst_h);
                        if self.cached_dims != key {
                            self.x_map_luma.resize(dst_w, 0);
                            self.x_map_uv.resize(dst_w, 0);
                            self.x_frac.resize(dst_w, 0);
                            let denom_x = (dst_w.saturating_sub(1)).max(1);
                            let span_x = orig_w.saturating_sub(1);
                            for dx in 0..dst_w {
                                let fx_256 = (dx * span_x * 256) / denom_x;
                                let sx = (fx_256 >> 8).min(span_x);
                                self.x_map_luma[dx] = sx;
                                self.x_map_uv[dx] = (sx >> 1).min(uv_w.saturating_sub(1));
                                self.x_frac[dx] = (fx_256 & 0xFF) as u16;
                            }
                            self.cached_dims = key;
                        }

                        let denom_y = (dst_h.saturating_sub(1)).max(1);
                        let span_y = orig_h.saturating_sub(1);
                        let max_sx = orig_w - 1;

                        for dy in 0..dst_h {
                            let fy_256 = (dy * span_y * 256) / denom_y;
                            let sy0 = (fy_256 >> 8).min(span_y);
                            let sy1 = (sy0 + 1).min(span_y);
                            let yf = (fy_256 & 0xFF) as i32;
                            let inv_yf = 256 - yf;

                            let y_row0 = &y_plane[sy0 * y_stride..sy0 * y_stride + orig_w];
                            let y_row1 = &y_plane[sy1 * y_stride..sy1 * y_stride + orig_w];
                            let uv_y = (sy0 >> 1).min(uv_h - 1);
                            let u_row = &u_plane[uv_y * u_stride..uv_y * u_stride + uv_w];
                            let v_row = &v_plane[uv_y * v_stride..uv_y * v_stride + uv_w];
                            let dst_row = &mut out_pixels[dy * dst_w..(dy + 1) * dst_w];

                            for dx in 0..dst_w {
                                let sx0 = self.x_map_luma[dx];
                                let sx1 = (sx0 + 1).min(max_sx);
                                let xf = self.x_frac[dx] as i32;
                                let inv_xf = 256 - xf;

                                let top = y_row0[sx0] as i32 * inv_xf + y_row0[sx1] as i32 * xf;
                                let bot = y_row1[sx0] as i32 * inv_xf + y_row1[sx1] as i32 * xf;
                                let yv = (top * inv_yf + bot * yf + 32_768) >> 16;

                                let uv_x = self.x_map_uv[dx];
                                let (cr, cg, cb) = Self::yuv_offsets(u_row[uv_x] as i32, v_row[uv_x] as i32, bt709);
                                dst_row[dx] = Self::color_from_y_offsets(yv, cr, cg, cb);
                            }
                        }
                    } else {
                        // Fallback defensivo caso o decoder retorne strides atipicos
                        self.x_map_luma.clear();
                        for dx in 0..dst_w {
                            let sx = ((dx * orig_w) / dst_w).min(orig_w - 1);
                            self.x_map_luma.push(sx);
                        }
                        for dy in 0..dst_h {
                            let sy = ((dy * orig_h) / dst_h).min(orig_h - 1);
                            let y_off = sy * y_stride;
                            let uv_y = sy >> 1;
                            let u_off = uv_y * u_stride;
                            let v_off = uv_y * v_stride;
                            let dst_row = &mut out_pixels[dy * dst_w..(dy + 1) * dst_w];
                            for (px, &sx) in dst_row.iter_mut().zip(self.x_map_luma.iter()) {
                                let yv = y_plane.get(y_off + sx).copied().unwrap_or(16) as i32;
                                let uv = u_plane.get(u_off + (sx >> 1)).copied().unwrap_or(128) as i32;
                                let vv = v_plane.get(v_off + (sx >> 1)).copied().unwrap_or(128) as i32;
                                let (cr, cg, cb) = Self::yuv_offsets(uv, vv, bt709);
                                *px = Self::color_from_y_offsets(yv, cr, cg, cb);
                            }
                        }
                    }

                    last_dims = Some((orig_w, orig_h, dst_w, dst_h));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(format!("Erro de decodificacao H.264: {:?}", e));
                }
            }
        }
        Ok(last_dims)
    }
}

#[inline]
fn compute_scaled_dims(orig_w: usize, orig_h: usize, max_w: usize, max_h: usize) -> (usize, usize) {
    if max_w == 0 || max_h == 0 || (orig_w <= max_w && orig_h <= max_h) {
        return (orig_w.max(1), orig_h.max(1));
    }
    let scale = (max_w as f64 / orig_w as f64).min(max_h as f64 / orig_h as f64);
    let dw = ((orig_w as f64 * scale).round() as usize).max(1).min(max_w);
    let dh = ((orig_h as f64 * scale).round() as usize).max(1).min(max_h);
    (dw, dh)
}

/// Re-agrupa pacotes RTP H.264 (Single NALU, STAP-A, FU-A) em Access Units Annex-B completas com janela curta de reordenacao.
pub struct H264RtpAssembler {
    depacketizer: H264Packet,
    last_seq: Option<u16>,
    expected_seq: Option<u16>,
    current_ts: Option<u32>,
    dropping_partial: bool,
    au_buf: Vec<u8>,
    reorder_buf: Vec<RtpPacket>,
}

impl H264RtpAssembler {
    pub fn new() -> Self {
        Self {
            depacketizer: H264Packet::default(),
            last_seq: None,
            expected_seq: None,
            current_ts: None,
            dropping_partial: false,
            au_buf: Vec::with_capacity(64 * 1024),
            reorder_buf: Vec::with_capacity(16),
        }
    }

    fn process_in_order_packet(&mut self, pkt: &RtpPacket, out_annex_b: &mut Vec<u8>) -> bool {
        if pkt.payload.is_empty() {
            self.last_seq = Some(pkt.header.sequence_number);
            return false;
        }

        let seq = pkt.header.sequence_number;
        let ts = pkt.header.timestamp;
        let mut saw_gap = false;

        if let Some(prev) = self.last_seq {
            if seq != prev.wrapping_add(1) {
                saw_gap = true;
                self.depacketizer = H264Packet::default();
                self.au_buf.clear();
                self.dropping_partial = true;
            }
        }
        self.last_seq = Some(seq);

        if self.dropping_partial {
            if self.depacketizer.is_partition_head(&pkt.payload) {
                self.dropping_partial = false;
                self.current_ts = Some(ts);
            } else {
                return saw_gap;
            }
        }

        if let Some(cur_ts) = self.current_ts {
            if cur_ts != ts {
                if !self.au_buf.is_empty() {
                    out_annex_b.extend_from_slice(&self.au_buf);
                    self.au_buf.clear();
                    if self.au_buf.capacity() > 512 * 1024 {
                        self.au_buf.shrink_to(256 * 1024);
                    }
                }
                self.depacketizer = H264Packet::default();
            }
        }
        self.current_ts = Some(ts);

        match self.depacketizer.depacketize(&pkt.payload) {
            Ok(bytes) => {
                if !bytes.is_empty() {
                    if self.au_buf.len() + bytes.len() <= MAX_AU_BYTES {
                        self.au_buf.extend_from_slice(&bytes);
                    } else {
                        self.au_buf.clear();
                        saw_gap = true;
                    }
                }
            }
            Err(_) => {
                self.depacketizer = H264Packet::default();
                self.au_buf.clear();
                return true;
            }
        }

        if pkt.header.marker && !self.au_buf.is_empty() {
            out_annex_b.extend_from_slice(&self.au_buf);
            self.au_buf.clear();
        }

        saw_gap
    }

    fn drain_contiguous_from(&mut self, mut exp: u16, out_annex_b: &mut Vec<u8>) -> (u16, bool) {
        let mut saw_gap = false;
        while let Some(idx) = self
            .reorder_buf
            .iter()
            .position(|p| p.header.sequence_number == exp)
        {
            let queued = self.reorder_buf.swap_remove(idx);
            saw_gap |= self.process_in_order_packet(&queued, out_annex_b);
            exp = exp.wrapping_add(1);
        }
        (exp, saw_gap)
    }

    fn distinct_ahead_timestamps(&self) -> usize {
        let mut seen = HashSet::with_capacity(4);
        for p in &self.reorder_buf {
            if self.current_ts != Some(p.header.timestamp) {
                seen.insert(p.header.timestamp);
            }
        }
        seen.len()
    }

    /// Processa um pacote RTP e retorna `(Option<access_unit_bytes>, saw_packet_gap)`.
    pub fn push_packet(&mut self, pkt: &RtpPacket) -> (Option<Bytes>, bool) {
        let seq = pkt.header.sequence_number;
        let Some(exp) = self.expected_seq else {
            let mut completed = Vec::new();
            let saw_gap = self.process_in_order_packet(pkt, &mut completed);
            self.expected_seq = Some(seq.wrapping_add(1));
            let res = (!completed.is_empty()).then(|| Bytes::from(completed));
            return (res, saw_gap);
        };

        let diff = seq.wrapping_sub(exp) as i16;
        let mut completed = Vec::new();
        let mut saw_gap = false;

        if diff == 0 {
            saw_gap |= self.process_in_order_packet(pkt, &mut completed);
            let (next_exp, gap2) = self.drain_contiguous_from(exp.wrapping_add(1), &mut completed);
            saw_gap |= gap2;
            self.expected_seq = Some(next_exp);
        } else if diff < 0 && diff > -1000 {
            // Pacote atrasado/duplicado: ignora sem corromper o fluxo
            return (None, false);
        } else if diff > 0 && diff <= 160 {
            if !self
                .reorder_buf
                .iter()
                .any(|p| p.header.sequence_number == seq)
            {
                self.reorder_buf.push(pkt.clone());
            }
            if self.reorder_buf.len() >= 128 || self.distinct_ahead_timestamps() >= 3 {
                if let Some((min_idx, _)) = self
                    .reorder_buf
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| p.header.sequence_number.wrapping_sub(exp))
                {
                    let earliest = self.reorder_buf.swap_remove(min_idx);
                    let earliest_seq = earliest.header.sequence_number;
                    saw_gap |= self.process_in_order_packet(&earliest, &mut completed);
                    let (next_exp, gap2) =
                        self.drain_contiguous_from(earliest_seq.wrapping_add(1), &mut completed);
                    saw_gap |= gap2;
                    self.expected_seq = Some(next_exp);
                }
            }
        } else {
            // Salto grande de sequencia ou reinicio de transmissao
            self.reorder_buf.clear();
            saw_gap |= self.process_in_order_packet(pkt, &mut completed);
            self.expected_seq = Some(seq.wrapping_add(1));
        }

        let res = (!completed.is_empty()).then(|| Bytes::from(completed));
        (res, saw_gap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use webrtc::rtp::packetizer::{new_packetizer, Packetizer};
    use webrtc::rtp::sequence::new_random_sequencer;

    #[test]
    fn test_opus_48khz_roundtrip() {
        let mut enc = OpusVoiceEncoder::new().expect("Opus encoder");
        let mut dec = OpusAudioDecoder::new().expect("Opus decoder");

        let mut pcm_in = vec![0i16; 960];
        for (i, s) in pcm_in.iter_mut().enumerate() {
            let phase = (i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0;
            *s = (phase.sin() * 8000.0) as i16;
        }

        let encoded = enc.encode_frame(&pcm_in).expect("Opus frame encoded");
        assert!(!encoded.is_empty());

        let mut pcm_out = Vec::new();
        assert!(dec.decode_packet(&encoded, &mut pcm_out));
        assert_eq!(pcm_out.len(), 960);
    }

    #[test]
    fn test_h264_rtp_roundtrip() {
        let (w, h) = (128usize, 96usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * 4;
                rgba[idx] = ((x * 2) % 255) as u8;
                rgba[idx + 1] = ((y * 2) % 255) as u8;
                rgba[idx + 2] = 180;
                rgba[idx + 3] = 255;
            }
        }

        let mut enc = H264VideoEncoder::new(true);
        let au = enc
            .encode_rgba(&rgba, w, h, 800_000, 30, true)
            .expect("H264 encode produced Annex-B frame");
        assert!(!au.is_empty());

        let mut packetizer = new_packetizer(
            1200,
            102,
            0x1234_5678,
            Box::<webrtc::rtp::codecs::h264::H264Payloader>::default(),
            Box::new(new_random_sequencer()),
            90_000,
        );
        let packets = packetizer.packetize(&au, 3000).expect("H264 packetize");
        assert!(!packets.is_empty());

        let mut assembler = H264RtpAssembler::new();
        let mut dec = H264VideoDecoder::new().expect("H264 decoder");
        let mut out_rgba = Vec::new();
        let mut decoded_dims = None;

        for pkt in &packets {
            let (maybe_au, gap) = assembler.push_packet(pkt);
            assert!(!gap);
            if let Some(frame_au) = maybe_au {
                if let Some(dims) = dec
                    .decode_annex_b(&frame_au, &mut out_rgba)
                    .expect("decode annex b")
                {
                    decoded_dims = Some(dims);
                }
            }
        }

        assert_eq!(decoded_dims, Some((w, h)));
        assert_eq!(out_rgba.len(), w * h * 4);
    }

    #[test]
    fn test_h264_parameter_sets_injects_sps_pps_and_strips_aud_sei() {
        let mut ps = H264ParameterSets::new();
        let sps = [0x67u8, 0x42, 0xE0, 0x1F, 0xDA, 0x01, 0x40, 0x16, 0xEC, 0x04, 0x40];
        let pps = [0x68u8, 0xCE, 0x3C, 0x80];
        let sei = [0x06u8, 0x05, 0x04, 0xDE, 0xAD, 0xBE, 0xEF, 0x80];
        let aud = [0x09u8, 0x10];
        let filler = [0x0Cu8, 0xFF, 0xFF, 0x80];
        let idr = [0x65u8, 0x88, 0x84, 0x21, 0xAA];
        let pframe = [0x41u8, 0x9A, 0x22, 0x33];

        // 0. AU contendo apenas SPS+PPS (sem VCL) faz cache mas retorna None
        let mut au0 = Vec::new();
        au0.extend_from_slice(&[0, 0, 0, 1]);
        au0.extend_from_slice(&sps);
        au0.extend_from_slice(&[0, 0, 0, 1]);
        au0.extend_from_slice(&pps);
        assert!(ps.normalize_access_unit(&au0).is_none());

        // 1. Primeiro IDR com AUD + SEI + SPS + PPS + Filler: faz cache e remove AUD/SEI/Filler
        let mut au1 = Vec::new();
        au1.extend_from_slice(&[0, 0, 0, 1]);
        au1.extend_from_slice(&aud);
        au1.extend_from_slice(&[0, 0, 0, 1]);
        au1.extend_from_slice(&sei);
        au1.extend_from_slice(&[0, 0, 0, 1]);
        au1.extend_from_slice(&sps);
        au1.extend_from_slice(&[0, 0, 0, 1]);
        au1.extend_from_slice(&pps);
        au1.extend_from_slice(&[0, 0, 0, 1]);
        au1.extend_from_slice(&idr);
        au1.extend_from_slice(&[0, 0, 0, 1]);
        au1.extend_from_slice(&filler);

        let (norm1, is_idr1) = ps.normalize_access_unit(&au1).expect("norm1");
        assert!(is_idr1);
        let nalus1: Vec<u8> = split_annex_b_nalus(&norm1).iter().map(|n| n[0] & 0x1F).collect();
        assert_eq!(nalus1, vec![7, 8, 5]);

        // 2. P-frame nao deve receber SPS/PPS injetados
        let mut au2 = Vec::new();
        au2.extend_from_slice(&[0, 0, 1]);
        au2.extend_from_slice(&pframe);
        let (norm2, is_idr2) = ps.normalize_access_unit(&au2).expect("norm2");
        assert!(!is_idr2);
        let nalus2: Vec<u8> = split_annex_b_nalus(&norm2).iter().map(|n| n[0] & 0x1F).collect();
        assert_eq!(nalus2, vec![1]);

        // 3. Segundo IDR sem SPS/PPS deve receber SPS+PPS injetados antes do slice IDR
        let mut au3 = Vec::new();
        au3.extend_from_slice(&[0, 0, 0, 1]);
        au3.extend_from_slice(&idr);
        let (norm3, is_idr3) = ps.normalize_access_unit(&au3).expect("norm3");
        assert!(is_idr3);
        let nalus3 = split_annex_b_nalus(&norm3);
        let types3: Vec<u8> = nalus3.iter().map(|n| n[0] & 0x1F).collect();
        assert_eq!(types3, vec![7, 8, 5]);
        assert_eq!(nalus3[0], &sps);
        assert_eq!(nalus3[1], &pps);
    }

    #[test]
    fn test_inspect_h264_au_detects_profile_level_and_b_slice() {
        // SPS: profile=0x42 (66 Baseline), constraint=0xE0, level=0x33 (51 = Level 5.1)
        let sps = [0x67u8, 0x42, 0xE0, 0x33, 0xDA, 0x01, 0x40];
        let pps = [0x68u8, 0xCE, 0x3C, 0x80];
        // IDR with first_mb_in_slice=0 (1), slice_type=7 (0001000) -> 1_0001_000 = 0x88, 0x00
        let idr = [0x65u8, 0x88, 0x00];
        let mut au_idr = Vec::new();
        for n in [&sps[..], &pps[..], &idr[..]] {
            au_idr.extend_from_slice(&[0, 0, 0, 1]);
            au_idr.extend_from_slice(n);
        }
        let info_idr = inspect_h264_au(&au_idr);
        assert!(info_idr.has_sps);
        assert!(info_idr.has_pps);
        assert!(info_idr.has_idr);
        assert!(!info_idr.has_b_slice);
        assert_eq!(info_idr.profile_idc, Some(0x42));
        assert_eq!(info_idr.constraint_flags, Some(0xE0));
        assert_eq!(info_idr.level_idc, Some(51));
        assert_eq!(info_idr.slice_types, vec![7]);

        // P-slice: first_mb_in_slice=0 (1), slice_type=0 (1) -> 1100_0000 = 0xC0
        let p_slice = [0x41u8, 0xC0, 0x00];
        let mut au_p = Vec::new();
        au_p.extend_from_slice(&[0, 0, 0, 1]);
        au_p.extend_from_slice(&p_slice);
        let info_p = inspect_h264_au(&au_p);
        assert!(info_p.has_non_idr_slice);
        assert!(!info_p.has_b_slice);
        assert_eq!(info_p.slice_types, vec![0]);

        // B-slice: first_mb_in_slice=0 (1), slice_type=1 (010) -> 1010_0000 = 0xA0
        let b_slice = [0x01u8, 0xA0, 0x00];
        let mut au_b = Vec::new();
        au_b.extend_from_slice(&[0, 0, 0, 1]);
        au_b.extend_from_slice(&b_slice);
        let info_b = inspect_h264_au(&au_b);
        assert!(info_b.has_non_idr_slice);
        assert!(info_b.has_b_slice);
        assert_eq!(info_b.slice_types, vec![1]);
    }

    #[test]
    fn test_h264_encoder_bgra_4k_and_no_frame_drop() {
        // Verifica ausencia de descarte de quadros em sequencia rapida e suporte a 3840x2160
        let (w, h) = (3840usize, 2160usize);
        let mut bgra = vec![40u8; w * h * 4];
        let mut enc = H264VideoEncoder::new(true);

        for i in 0..3u32 {
            // Injeta movimento simples em uma faixa para exercer controle de taxa sem descartar frame
            let start = (i as usize) * w * 4 * 8;
            for p in bgra[start..start + w * 4 * 8].chunks_exact_mut(4) {
                p[0] = (i * 60) as u8;
                p[1] = 180;
                p[2] = 220;
                p[3] = 255;
            }
            let (out, is_kf) = enc
                .encode_bgra_with_keyframe(&bgra, w, h, 16_000_000, 240, i == 0)
                .unwrap_or_else(|| panic!("frame {} at 4K 240fps must not be skipped", i));
            let info = inspect_h264_au(&out);
            assert!(!info.has_b_slice, "OpenH264 Baseline must not emit B-slices");
            if i == 0 {
                assert!(is_kf && info.has_sps && info.has_pps && info.has_idr);
            }
        }
    }

    #[test]
    fn test_opus_voice_encoder_tuned_for_low_cpu_and_fec() {
        let mut enc = OpusVoiceEncoder::new().expect("OpusVoiceEncoder::new");
        assert_eq!(enc.complexity(), 5);
        assert_eq!(enc.packet_loss_perc(), 15);

        let mut dec = OpusAudioDecoder::new().expect("OpusAudioDecoder::new");
        let mut pcm_in = [0i16; 960];
        for (i, s) in pcm_in.iter_mut().enumerate() {
            let phase = (i as f32) * 2.0 * std::f32::consts::PI * 220.0 / 48_000.0;
            *s = (phase.sin() * 12_000.0) as i16;
        }
        // Prime encoder & check non-silent output
        let mut decoded = Vec::new();
        for _ in 0..3 {
            let pkt = enc.encode_frame(&pcm_in).expect("encode_frame");
            assert!(!pkt.is_empty());
            assert!(dec.decode_packet(&pkt, &mut decoded));
            assert_eq!(decoded.len(), 960);
        }
        let energy: i64 = decoded.iter().map(|&s| (s as i64).abs()).sum();
        assert!(energy > 10_000, "decoded Opus signal must be non-silent");
    }

    #[test]
    fn test_opus_stereo_audio_encoder_roundtrip() {
        let mut enc = OpusStereoAudioEncoder::new().expect("OpusStereoAudioEncoder::new");
        let mut dec = OpusAudioDecoder::new().expect("OpusAudioDecoder::new");
        let mut pcm_stereo = [0i16; 1920];
        for i in 0..960 {
            let l = ((i as f32) * 2.0 * std::f32::consts::PI * 330.0 / 48_000.0).sin();
            let r = ((i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin();
            pcm_stereo[i * 2] = (l * 10_000.0) as i16;
            pcm_stereo[i * 2 + 1] = (r * 10_000.0) as i16;
        }
        let mut decoded = Vec::new();
        for _ in 0..3 {
            let pkt = enc.encode_frame(&pcm_stereo).expect("stereo encode_frame");
            assert!(!pkt.is_empty());
            assert!(dec.decode_packet(&pkt, &mut decoded));
            assert_eq!(decoded.len(), 960);
        }
        let energy: i64 = decoded.iter().map(|&s| (s as i64).abs()).sum();
        assert!(energy > 10_000, "decoded stereo Opus signal must be non-silent");
    }

    #[test]
    fn test_normalize_access_unit_no_b_slices_rejects_b_and_keeps_idr() {
        let mut ps = H264ParameterSets::default();
        // SPS + PPS + IDR (valid AU)
        let idr_au = [
            0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0x8D, 0x8D, 0x40, 0x50,
            0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80,
            0, 0, 0, 1, 0x65, 0x88, 0x84, 0x21,
        ];
        let res = ps.normalize_access_unit_no_b_slices(&idr_au);
        let (norm, is_idr) = res.expect("IDR AU without B-slice should succeed");
        assert!(is_idr);
        assert!(!norm.is_empty());

        // B-slice (slice_type=1 -> Exp-Golomb: first_mb_in_slice=0 ('1'), slice_type=1 ('010') -> 0b1010_0000 = 0xA0)
        let b_au = [0, 0, 0, 1, 0x01, 0xA0, 0x00, 0x01];
        assert!(
            ps.normalize_access_unit_no_b_slices(&b_au).is_none(),
            "B-slice AU must be rejected inline by normalize_access_unit_no_b_slices"
        );
    }

    #[test]
    fn test_h264_decoder_scaled_color32_downscales_and_reuses_buffer() {
        let (w, h) = (320usize, 180usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * 4;
                rgba[idx] = 200;
                rgba[idx + 1] = 80;
                rgba[idx + 2] = 40;
                rgba[idx + 3] = 255;
            }
        }

        let mut enc = H264VideoEncoder::new(true);
        let au = enc
            .encode_rgba(&rgba, w, h, 800_000, 30, true)
            .expect("H264 encode");
        let mut dec = H264VideoDecoder::new().expect("H264 decoder");
        let mut pixels = Vec::with_capacity(640 * 360);
        let orig_ptr = pixels.as_ptr();

        let dims = dec
            .decode_annex_b_to_color32(&au, 160, 90, &mut pixels)
            .expect("scaled decode")
            .expect("decoded frame");
        assert_eq!(dims, (320, 180, 160, 90));
        assert_eq!(pixels.len(), 160 * 90);
        assert_eq!(pixels.as_ptr(), orig_ptr, "must reuse preallocated Vec without realloc");
    }

    #[test]
    fn test_h264_rtp_assembler_reorders_swapped_packets_without_gap() {
        let (w, h) = (320usize, 180usize);
        let mut rgba = vec![20u8; w * h * 4];
        for i in (0..rgba.len()).step_by(4) {
            rgba[i] = ((i / 4) % 251) as u8;
            rgba[i + 1] = 140;
            rgba[i + 2] = 210;
            rgba[i + 3] = 255;
        }
        let mut enc = H264VideoEncoder::new(true);
        let au = enc
            .encode_rgba(&rgba, w, h, 1_500_000, 30, true)
            .expect("H264 encode");

        let mut packetizer = new_packetizer(
            180,
            102,
            0x2222_3333,
            Box::<webrtc::rtp::codecs::h264::H264Payloader>::default(),
            Box::new(new_random_sequencer()),
            90_000,
        );
        let mut packets = packetizer.packetize(&au, 3000).expect("packetize");
        assert!(packets.len() >= 3, "need >= 3 packets to test reorder");
        packets.swap(1, 2);

        let mut assembler = H264RtpAssembler::new();
        let mut dec = H264VideoDecoder::new().expect("decoder");
        let mut out_rgba = Vec::new();
        let mut decoded = None;

        for pkt in &packets {
            let (maybe_au, gap) = assembler.push_packet(pkt);
            assert!(!gap, "reordered packet within window must not trigger gap");
            if let Some(buf) = maybe_au {
                decoded = dec.decode_annex_b(&buf, &mut out_rgba).expect("decode");
            }
        }
        assert_eq!(decoded, Some((w, h)));
    }
}
