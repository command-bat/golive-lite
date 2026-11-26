use bytes::Bytes;
use serde::Serialize;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};
use webrtc::api::interceptor_registry::{configure_nack, configure_rtcp_reports, configure_twcc};
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS};
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice::candidate::{CandidatePairState, CandidateType};
use webrtc::ice::mdns::MulticastDnsMode;
use webrtc::ice::network_type::NetworkType;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
use webrtc::ice_transport::ice_gathering_state::RTCIceGatheringState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::offer_answer_options::RTCOfferOptions;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::signaling_state::RTCSignalingState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::rtcp::transport_feedbacks::transport_layer_nack::TransportLayerNack;
use webrtc::rtp::codecs::h264::H264Payloader;
use webrtc::rtp::extension::HeaderExtension;
use webrtc::rtp::header::Header as RtpHeader;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::rtp::packetizer::Payloader;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTCRtpHeaderExtensionCapability, RTPCodecType,
};
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::{RTCPFeedback, RTCRtpTransceiverInit};
use webrtc::stats::{ICECandidatePairStats, ICECandidateStats, StatsReport, StatsReportType};
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::util::marshal::{Marshal, MarshalSize};

use crate::media::audio::{AudioThreadPriorityGuard, CapturedAudioFrame};
use crate::media::audio_mixer::AudioMixer;
use crate::media::codecs::{
    H264RtpAssembler, H264VideoDecoder, OpusAudioDecoder, OpusVoiceEncoder,
};
use crate::net::protocol::{IceServerConfig, SignalData};
use crate::net::signaling::{now_epoch_ms, SignalingCommand};

const CONNECT_TIMEOUT_SECS: u64 = 12;
const RETRY_BACKOFF_SECS: [u64; 5] = [2, 4, 8, 12, 15];
const STATS_POLL_SECS: u64 = 2;
const DIAG_REQUEST_INTERVAL_MS: u128 = 5_000;
const DIAG_LEASE_SECS: u64 = 10;
const DIAG_MAX_RESPONDERS_PER_CHANNEL: usize = 8;
const SENDER_SILENCE_SECS: u64 = 8;
const NOT_CONNECTED_SECS: u64 = 10;
const KEYFRAME_PLI_COOLDOWN_MS: u64 = 500;
const KEYFRAME_CONGESTION_COOLDOWN_MS: u64 = 800;
const H264_RTP_MTU: usize = 1_188;
const PLAYOUT_DELAY_URI: &str = "http://www.webrtc.org/experiments/rtp-hdrext/playout-delay";

/// Extensao RTP `playout-delay` com min=0ms e max=0ms para instruir Chrome/Electron
/// a renderizar cada quadro imediatamente sem acumular 100ms+ no jitter buffer.
#[derive(Debug, Clone, Copy, Default)]
struct ZeroPlayoutDelayExt;

impl MarshalSize for ZeroPlayoutDelayExt {
    fn marshal_size(&self) -> usize {
        3
    }
}

impl Marshal for ZeroPlayoutDelayExt {
    fn marshal_to(&self, buf: &mut [u8]) -> Result<usize, webrtc::util::Error> {
        if buf.len() < 3 {
            return Err(webrtc::rtp::Error::ErrBufferTooSmall.into());
        }
        buf[0] = 0;
        buf[1] = 0;
        buf[2] = 0;
        Ok(3)
    }
}

/// Registro compartilhado de pedidos de keyframe (IDR) por canal ("screen", "screen2", "camera").
pub type KeyframeSignals = Arc<std::sync::Mutex<HashMap<String, Arc<AtomicBool>>>>;
pub type KeyframeCooldowns = Arc<std::sync::Mutex<HashMap<String, Arc<AtomicU64>>>>;

pub fn new_keyframe_signals() -> KeyframeSignals {
    Arc::new(std::sync::Mutex::new(HashMap::new()))
}

pub fn new_keyframe_cooldowns() -> KeyframeCooldowns {
    Arc::new(std::sync::Mutex::new(HashMap::new()))
}

pub fn channel_keyframe_flag(signals: &KeyframeSignals, channel: &str) -> Arc<AtomicBool> {
    let mut map = signals.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(channel.to_string())
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

pub fn channel_keyframe_cooldown(cooldowns: &KeyframeCooldowns, channel: &str) -> Arc<AtomicU64> {
    let mut map = cooldowns.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(channel.to_string())
        .or_insert_with(|| Arc::new(AtomicU64::new(0)))
        .clone()
}

/// Agenda pedido de IDR com cooldown minimo (`min_gap_ms`) para evitar tempestades de keyframes.
pub fn request_keyframe_coalesced(
    flag: &AtomicBool,
    last_req_ms: &AtomicU64,
    min_gap_ms: u64,
) -> bool {
    let now_ms = now_epoch_ms().max(1);
    loop {
        let prev = last_req_ms.load(Ordering::Acquire);
        if prev != 0 && min_gap_ms > 0 && now_ms.saturating_sub(prev) < min_gap_ms {
            return false;
        }
        if last_req_ms
            .compare_exchange(prev, now_ms, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            flag.store(true, Ordering::Release);
            return true;
        }
    }
}

pub const VIEW_POLICY_HIDDEN: u8 = 0;
pub const VIEW_POLICY_GRID_MULTI: u8 = 1;
pub const VIEW_POLICY_GRID_LARGE: u8 = 2;
pub const VIEW_POLICY_FOCUSED: u8 = 3;

pub fn quality_tier_for_policy(policy: u8) -> (&'static str, bool) {
    match policy {
        VIEW_POLICY_FOCUSED => ("2160p120", false),
        VIEW_POLICY_GRID_LARGE => ("1440p60", false),
        VIEW_POLICY_GRID_MULTI => ("1080p60", false),
        _ => ("576p15", true),
    }
}

/// Mapa compartilhado de politica de renderizacao por stream remoto `(channel, peer_id)`.
/// Lido a custo ~zero (1 atomic load) na thread de decodificacao H.264 para pular decodificacao
/// no Modo Eco (`HIDDEN`) ou limitar resolucao/FPS na grade (`GRID`).
#[derive(Clone, Default)]
pub struct VideoRenderPolicyMap {
    inner: Arc<std::sync::Mutex<HashMap<(String, String), Arc<AtomicU8>>>>,
}

impl VideoRenderPolicyMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create(&self, channel: &str, peer_id: &str) -> Arc<AtomicU8> {
        let key = (channel.to_string(), peer_id.to_string());
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .entry(key)
            .or_insert_with(|| Arc::new(AtomicU8::new(VIEW_POLICY_HIDDEN)))
            .clone()
    }

    pub fn set(&self, channel: &str, peer_id: &str, mode: u8) {
        let entry = self.get_or_create(channel, peer_id);
        entry.store(mode, Ordering::Relaxed);
    }

    pub fn remove(&self, channel: &str, peer_id: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&(channel.to_string(), peer_id.to_string()));
    }

    pub fn remove_peer(&self, peer_id: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|(_, pid), _| pid != peer_id);
    }

    pub fn clear(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
    }
}

#[derive(Debug, Clone)]
pub struct DecodedRemoteVideo {
    pub channel: String,
    pub peer_id: String,
    pub width: usize,
    pub height: usize,
    pub orig_width: usize,
    pub orig_height: usize,
    pub pixels: Vec<egui::Color32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsTarget {
    Remote { channel: String, peer_id: String },
    Own { channel: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RouteKind {
    Direct,
    RelayLocal,
    RelayRemote,
    RelayBoth,
    #[default]
    Unknown,
}

impl RouteKind {
    pub fn is_relayed(self) -> bool {
        matches!(
            self,
            Self::RelayLocal | Self::RelayRemote | Self::RelayBoth
        )
    }

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Direct => "Direta",
            Self::RelayLocal => "Via TURN (seu lado)",
            Self::RelayRemote => "Via TURN (lado da outra pessoa)",
            Self::RelayBoth => "Via TURN (os dois lados)",
            Self::Unknown => "Conectando…",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteStats {
    pub kind: RouteKind,
    pub local_type: String,
    pub remote_type: String,
    pub protocol: Option<String>,
    pub relay_protocol: Option<String>,
    pub relay_url: Option<String>,
    pub rtt_ms: Option<u32>,
    pub available_outgoing_kbps: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecvVideoMetrics {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub kbps: u32,
    pub loss: f32,
    pub drop_share: f32,
    pub freezes: u32,
    pub freeze_seconds: f32,
    pub codec: Option<String>,
    pub hardware: Option<bool>,
    pub decoder: Option<String>,
    pub frames_decoded_total: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendVideoMetrics {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub kbps: u32,
    pub limitation: String,
    pub cpu_limited_share: f32,
    pub bandwidth_limited_share: f32,
    pub encoder: Option<String>,
    pub hardware: Option<bool>,
    pub codec: Option<String>,
    pub remote_loss: f32,
    pub nack_per_second: f32,
    pub pli_per_second: f32,
    pub target_kbps: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSenderDiag {
    pub send: Option<SendVideoMetrics>,
    pub route: Option<RouteStats>,
    pub cores: Option<u32>,
    pub tier: Option<String>,
    pub profile: Option<String>,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CauseSeverity {
    High,
    Medium,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagCause {
    pub id: &'static str,
    pub severity: CauseSeverity,
    pub message_pt: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTileStatsSnapshot {
    pub channel: String,
    pub peer_id: String,
    pub route: RouteStats,
    pub recv: Option<RecvVideoMetrics>,
    pub remote_diag: Option<RemoteSenderDiag>,
    pub sender_silent: bool,
    pub ice_state: String,
    pub not_connected: bool,
    pub turn_configured: bool,
    pub causes: Vec<DiagCause>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnViewerStatsRow {
    pub peer_id: String,
    pub route: RouteStats,
    pub send: Option<SendVideoMetrics>,
    pub paused: bool,
    pub ice_state: String,
    pub not_connected: bool,
    pub worst_cause: Option<DiagCause>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnStreamStatsSnapshot {
    pub channel: String,
    pub capture_width: u32,
    pub capture_height: u32,
    pub capture_fps: u32,
    pub total_kbps: u32,
    pub codec: String,
    pub encoder: String,
    pub hardware: bool,
    pub cores: Option<u32>,
    pub turn_configured: bool,
    pub viewers: Vec<OwnViewerStatsRow>,
}

#[derive(Debug, Clone)]
pub enum TileStatsSnapshot {
    Remote(RemoteTileStatsSnapshot),
    Own(OwnStreamStatsSnapshot),
}

#[derive(Debug)]
pub enum WebRtcCommand {
    SetIceServers(Vec<IceServerConfig>),
    SetSelfId(String),
    SetWatchedStats(Option<StatsTarget>),
    UpdateLocalStreamMeta {
        channel: String,
        width: u32,
        height: u32,
        target_fps: u32,
        target_bitrate_kbps: u32,
        encoder_name: String,
        hardware: bool,
    },
    StartChannel {
        channel: String,
        peers: Vec<String>,
        with_audio: bool,
    },
    StopChannel {
        channel: String,
    },
    SyncPeers {
        peers: Vec<String>,
    },
    HandleSignal {
        from: String,
        data: SignalData,
    },
    StopWatchingPeer {
        channel: String,
        peer_id: String,
    },
    CleanupPeerChannel {
        channel: String,
        peer_id: String,
    },
    ResumeWatchingPeer {
        channel: String,
        peer_id: String,
    },
    SendVideoSample {
        channel: String,
        data: Bytes,
        is_keyframe: bool,
        pts: Duration,
    },
    CloseAll,
    MarkSendConnected {
        channel: String,
        peer_id: String,
        expected_pc_id: u64,
    },
    RetrySendPc {
        channel: String,
        peer_id: String,
        expected_pc_id: u64,
    },
    ReopenSendPc {
        channel: String,
        peer_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum WebRtcEvent {
    PeerStoppedSharing { channel: String, peer_id: String },
    PeerResuming { channel: String, peer_id: String },
    StatsUpdated(TileStatsSnapshot),
}

const MAX_RECYCLED_FRAME_BUFS: usize = 1;
const MAX_RECYCLED_FRAME_PIXELS: usize = 1920 * 1080;

/// Mailbox O(1) de ultimo frame remoto por `(canal, peer)` com reciclagem de buffers `Vec<Color32>`
/// para evitar realocacoes constantes e manter consumo minimo de RAM.
#[derive(Clone)]
pub struct RemoteVideoMailbox {
    inner: Arc<std::sync::Mutex<HashMap<(String, String), DecodedRemoteVideo>>>,
    recycle_pool: Arc<std::sync::Mutex<Vec<Vec<egui::Color32>>>>,
}

impl RemoteVideoMailbox {
    fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(HashMap::new())),
            recycle_pool: Arc::new(std::sync::Mutex::new(Vec::with_capacity(
                MAX_RECYCLED_FRAME_BUFS,
            ))),
        }
    }

    pub fn take_recycled_buf(&self) -> Vec<egui::Color32> {
        let mut pool = self.recycle_pool.lock().unwrap_or_else(|e| e.into_inner());
        pool.pop().unwrap_or_default()
    }

    pub fn recycle_buf(&self, mut buf: Vec<egui::Color32>) {
        if buf.capacity() == 0 || buf.capacity() > MAX_RECYCLED_FRAME_PIXELS {
            return;
        }
        buf.clear();
        let mut pool = self.recycle_pool.lock().unwrap_or_else(|e| e.into_inner());
        if pool.len() < MAX_RECYCLED_FRAME_BUFS {
            pool.push(buf);
        }
    }

    pub fn push(&self, frame: DecodedRemoteVideo) {
        let key = (frame.channel.clone(), frame.peer_id.clone());
        let replaced = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.insert(key, frame)
        };
        if let Some(old) = replaced {
            self.recycle_buf(old.pixels);
        }
    }

    pub fn keys_for_peer(&self, peer_id: &str) -> Vec<String> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .keys()
            .filter(|(_, pid)| pid == peer_id)
            .map(|(ch, _)| ch.clone())
            .collect()
    }

    pub fn remove(&self, channel: &str, peer_id: &str) {
        let (removed, empty_now) = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let r = guard.remove(&(channel.to_string(), peer_id.to_string()));
            if guard.is_empty() {
                guard.shrink_to_fit();
            }
            (r, guard.is_empty())
        };
        if empty_now {
            let mut pool = self.recycle_pool.lock().unwrap_or_else(|e| e.into_inner());
            pool.clear();
            pool.shrink_to_fit();
            drop(removed);
        } else if let Some(old) = removed {
            self.recycle_buf(old.pixels);
        }
    }

    pub fn clear(&self) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.clear();
            guard.shrink_to_fit();
        }
        let mut pool = self.recycle_pool.lock().unwrap_or_else(|e| e.into_inner());
        pool.clear();
        pool.shrink_to_fit();
    }

    pub fn shrink_idle(&self) {
        let empty = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_empty() {
                guard.shrink_to_fit();
                true
            } else {
                false
            }
        };
        if empty {
            let mut pool = self.recycle_pool.lock().unwrap_or_else(|e| e.into_inner());
            pool.clear();
            pool.shrink_to_fit();
        }
    }

    pub fn drain_latest(&self) -> Vec<DecodedRemoteVideo> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_empty() {
            return Vec::new();
        }
        std::mem::take(&mut *guard).into_values().collect()
    }
}

/// Buffer que garante que candidatos ICE locais nunca ultrapassem a mensagem SDP (`offer` ou `answer`) na sinalizacao.
struct PendingLocalIce {
    ready: bool,
    mid: String,
    ufrag: Option<String>,
    queued: Vec<String>,
}

impl PendingLocalIce {
    fn new() -> Self {
        Self {
            ready: false,
            mid: "0".to_string(),
            ufrag: None,
            queued: Vec::new(),
        }
    }
}

#[derive(Default)]
struct RecvCounters {
    bytes_received: AtomicU64,
    packets_received: AtomicU64,
    packets_lost: AtomicU64,
    frames_received: AtomicU64,
    frames_dropped: AtomicU64,
    frames_decoded: AtomicU64,
    freeze_count: AtomicU32,
    freeze_duration_ms: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
}

#[derive(Clone, Copy)]
struct RecvCounterSample {
    at: Instant,
    bytes_received: u64,
    packets_received: u64,
    packets_lost: u64,
    frames_received: u64,
    frames_dropped: u64,
    frames_decoded: u64,
    freeze_count: u32,
    freeze_duration_ms: u64,
    width: u32,
    height: u32,
}

impl RecvCounters {
    fn snapshot(&self) -> RecvCounterSample {
        RecvCounterSample {
            at: Instant::now(),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            packets_received: self.packets_received.load(Ordering::Relaxed),
            packets_lost: self.packets_lost.load(Ordering::Relaxed),
            frames_received: self.frames_received.load(Ordering::Relaxed),
            frames_dropped: self.frames_dropped.load(Ordering::Relaxed),
            frames_decoded: self.frames_decoded.load(Ordering::Relaxed),
            freeze_count: self.freeze_count.load(Ordering::Relaxed),
            freeze_duration_ms: self.freeze_duration_ms.load(Ordering::Relaxed),
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
struct SendCounters {
    bytes_sent: AtomicU64,
    frames_sent: AtomicU64,
    nack_count: AtomicU64,
    pli_count: AtomicU64,
}

#[derive(Clone, Copy)]
struct SendCounterSample {
    at: Instant,
    bytes_sent: u64,
    frames_sent: u64,
    nack_count: u64,
    pli_count: u64,
}

impl SendCounters {
    fn snapshot(&self) -> SendCounterSample {
        SendCounterSample {
            at: Instant::now(),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            frames_sent: self.frames_sent.load(Ordering::Relaxed),
            nack_count: self.nack_count.load(Ordering::Relaxed),
            pli_count: self.pli_count.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Default)]
struct LocalStreamMeta {
    width: u32,
    height: u32,
    target_fps: u32,
    target_bitrate_kbps: u32,
    encoder_name: String,
    hardware: bool,
}

#[derive(Clone)]
struct VideoFanoutFrame {
    data: Bytes,
    is_keyframe: bool,
    pts: Duration,
}

#[derive(Clone)]
struct VideoFanoutTarget {
    tx: mpsc::Sender<VideoFanoutFrame>,
    need_key: Arc<AtomicBool>,
    channel_kf_flag: Arc<AtomicBool>,
    channel_kf_last_ms: Arc<AtomicU64>,
}

type VideoFanoutMap = HashMap<String, Vec<VideoFanoutTarget>>;

#[derive(Clone)]
pub struct VideoFanoutBus {
    inner: Arc<std::sync::RwLock<VideoFanoutMap>>,
}

impl VideoFanoutBus {
    fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn has_targets(&self, channel: &str) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel)
            .is_some_and(|v| !v.is_empty())
    }

    pub fn send_encoded_video(
        &self,
        channel: &str,
        data: Bytes,
        is_keyframe: bool,
        pts: Duration,
    ) {
        let targets = self
            .inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel)
            .cloned();
        if let Some(targets) = targets {
            let frame = VideoFanoutFrame {
                data,
                is_keyframe,
                pts,
            };
            for target in targets {
                if let Err(tokio::sync::mpsc::error::TrySendError::Full(_)) =
                    target.tx.try_send(frame.clone())
                {
                    target.need_key.store(true, Ordering::Release);
                    request_keyframe_coalesced(
                        &target.channel_kf_flag,
                        &target.channel_kf_last_ms,
                        KEYFRAME_CONGESTION_COOLDOWN_MS,
                    );
                }
            }
        }
    }
}

#[derive(Clone)]
struct AudioFanoutPacket {
    data: Bytes,
    frame_seq: u64,
    marker: bool,
}

#[derive(Clone)]
struct AudioFanoutTarget {
    tx: mpsc::Sender<AudioFanoutPacket>,
}

type AudioFanoutMap = HashMap<String, Vec<AudioFanoutTarget>>;

#[derive(Clone)]
pub struct AudioFanoutBus {
    inner: Arc<std::sync::RwLock<AudioFanoutMap>>,
}

impl AudioFanoutBus {
    fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn has_targets(&self, channel: &str) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel)
            .is_some_and(|v| !v.is_empty())
    }

    pub fn send_encoded_audio(&self, channel: &str, data: Bytes, frame_seq: u64, marker: bool) {
        let targets = self
            .inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel)
            .cloned();
        if let Some(targets) = targets {
            let pkt = AudioFanoutPacket {
                data,
                frame_seq,
                marker,
            };
            for target in targets {
                let _ = target.tx.try_send(pkt.clone());
            }
        }
    }
}

struct SendPeerEntry {
    pc_id: u64,
    pc: Arc<RTCPeerConnection>,
    local_ice: Arc<std::sync::Mutex<PendingLocalIce>>,
    audio_track: Option<Arc<TrackLocalStaticRTP>>,
    audio_tx: Option<mpsc::Sender<AudioFanoutPacket>>,
    video_track: Option<Arc<TrackLocalStaticRTP>>,
    video_tx: Option<mpsc::Sender<VideoFanoutFrame>>,
    peer_need_key: Option<Arc<AtomicBool>>,
    send_counters: Arc<SendCounters>,
    unconnected_since: Arc<std::sync::Mutex<Option<Instant>>>,
    last_ice_restart_at: Option<Instant>,
    paused_by_viewer: bool,
    remote_desc_set: bool,
    connected: bool,
}

struct RecvPeerEntry {
    pc: Arc<RTCPeerConnection>,
    remote_desc_set: bool,
    local_ice: Arc<std::sync::Mutex<PendingLocalIce>>,
    recv_counters: Arc<RecvCounters>,
    unconnected_since: Arc<std::sync::Mutex<Option<Instant>>>,
}

struct DiagResponderLease {
    origin_id: String,
    expires_at: Instant,
}

struct WebRtcState {
    api: Arc<API>,
    self_id: Option<String>,
    ice_servers: Vec<RTCIceServer>,
    turn_configured: bool,
    next_pc_id: u64,
    active_send_channels: HashSet<String>,
    channel_has_audio: HashMap<String, bool>,
    known_peers: HashSet<String>,
    viewer_stopped_peers: HashSet<(String, String)>,
    send_retry_attempts: HashMap<(String, String), u32>,
    send_pcs: HashMap<(String, String), SendPeerEntry>,
    recv_pcs: HashMap<(String, String), RecvPeerEntry>,
    stopped_by_us: HashSet<(String, String)>,
    pending_recv_ice: HashMap<(String, String), Vec<RTCIceCandidateInit>>,
    pending_send_ice: HashMap<(String, String), Vec<RTCIceCandidateInit>>,
    keyframe_signals: KeyframeSignals,
    kf_cooldowns: KeyframeCooldowns,
    local_stream_meta: HashMap<String, LocalStreamMeta>,
    watched_stats: Option<StatsTarget>,
    watch_started_at: Option<Instant>,
    last_diag_request_at: Option<Instant>,
    latest_remote_diag: Option<((String, String), RemoteSenderDiag)>,
    last_remote_snapshot: Option<RemoteTileStatsSnapshot>,
    prev_recv_sample: Option<((String, String), RecvCounterSample)>,
    prev_send_samples: HashMap<(String, String, u64), SendCounterSample>,
    prev_diag_send_samples: HashMap<(String, String, u64), SendCounterSample>,
    diag_responders: HashMap<(String, String), DiagResponderLease>,
    video_fanout: VideoFanoutBus,
    audio_fanout: AudioFanoutBus,
    remote_video_mailbox: RemoteVideoMailbox,
    policy_map: VideoRenderPolicyMap,
}

impl WebRtcState {
    fn request_channel_keyframe(&self, channel: &str, min_gap_ms: u64) {
        if channel == "mic" {
            return;
        }
        let flag = channel_keyframe_flag(&self.keyframe_signals, channel);
        let last_ms = channel_keyframe_cooldown(&self.kf_cooldowns, channel);
        request_keyframe_coalesced(&flag, &last_ms, min_gap_ms);
    }

    fn refresh_media_fanout(&self) {
        let mut video_map: VideoFanoutMap = HashMap::new();
        let mut audio_map: AudioFanoutMap = HashMap::new();
        for ((ch, _), entry) in &self.send_pcs {
            if entry.paused_by_viewer || !entry.remote_desc_set || !entry.connected {
                continue;
            }
            if let Some(ref tx) = entry.audio_tx {
                audio_map
                    .entry(ch.clone())
                    .or_default()
                    .push(AudioFanoutTarget { tx: tx.clone() });
            }
            if ch != "mic" {
                if let (Some(ref tx), Some(ref need_key)) = (&entry.video_tx, &entry.peer_need_key)
                {
                    let kf_flag = channel_keyframe_flag(&self.keyframe_signals, ch);
                    let kf_last_ms = channel_keyframe_cooldown(&self.kf_cooldowns, ch);
                    video_map.entry(ch.clone()).or_default().push(VideoFanoutTarget {
                        tx: tx.clone(),
                        need_key: need_key.clone(),
                        channel_kf_flag: kf_flag,
                        channel_kf_last_ms: kf_last_ms,
                    });
                }
            }
        }
        {
            let mut guard = self
                .video_fanout
                .inner
                .write()
                .unwrap_or_else(|e| e.into_inner());
            *guard = video_map;
        }
        {
            let mut guard = self.audio_fanout.inner.write().unwrap_or_else(|e| e.into_inner());
            *guard = audio_map;
        }
    }
}

/// Servidores STUN padrao sempre mantidos como base de conectividade.
pub fn default_stun_servers() -> Vec<RTCIceServer> {
    vec![
        RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            ..Default::default()
        },
        RTCIceServer {
            urls: vec!["stun:stun.cloudflare.com:3478".to_string()],
            ..Default::default()
        },
    ]
}

/// Normaliza os ICE servers recebidos do backend para a API `webrtc 0.11` (`Password`, UDP TURN e STUN sem query).
pub fn sanitize_ice_servers(list: Vec<IceServerConfig>) -> (Vec<RTCIceServer>, bool) {
    let mut out = default_stun_servers();
    let mut seen_stun: HashSet<String> = out
        .iter()
        .flat_map(|s| s.urls.iter().cloned())
        .collect();
    let mut has_turn = false;

    for server in list {
        let user = server.username.unwrap_or_default().trim().to_string();
        let cred = server.credential.unwrap_or_default().trim().to_string();
        let mut stun_urls = Vec::new();
        let mut turn_urls = Vec::new();

        for raw in server.urls {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("stun:") {
                let clean = trimmed.split('?').next().unwrap_or(trimmed).to_string();
                if seen_stun.insert(clean.clone()) {
                    stun_urls.push(clean);
                }
            } else if lower.starts_with("turn:") {
                // `webrtc-ice 0.11` suporta TURN sobre UDP (`turn:` sem `?transport=tcp`),
                // e a porta 53 deve ser ignorada (igual a `cloudflareTurn.ts`/`ownTurn.ts`) para nao travar gathering.
                let is_port_53 = lower.contains(":53?") || lower.ends_with(":53");
                if !lower.contains("transport=tcp") && !is_port_53 {
                    turn_urls.push(trimmed.to_string());
                }
            }
        }

        if !stun_urls.is_empty() {
            out.push(RTCIceServer {
                urls: stun_urls,
                ..Default::default()
            });
        }
        if !turn_urls.is_empty() && !user.is_empty() && !cred.is_empty() {
            has_turn = true;
            out.push(RTCIceServer {
                urls: turn_urls,
                username: user,
                credential: cred,
                credential_type: RTCIceCredentialType::Password,
            });
        }
    }

    (out, has_turn)
}

async fn create_peer_connection(
    api: &API,
    ice_servers: Vec<RTCIceServer>,
) -> Option<Arc<RTCPeerConnection>> {
    if let Ok(pc) = api
        .new_peer_connection(RTCConfiguration {
            ice_servers,
            ..Default::default()
        })
        .await
    {
        return Some(Arc::new(pc));
    }

    // Fallback automatico para STUN puro caso alguma credencial TURN dinamica seja rejeitada.
    api.new_peer_connection(RTCConfiguration {
        ice_servers: default_stun_servers(),
        ..Default::default()
    })
    .await
    .ok()
    .map(Arc::new)
}

/// Extrai o primeiro `a=mid:` (padrao `"0"`) e `a=ice-ufrag:` de um SDP local.
pub fn extract_sdp_mid_and_ufrag(sdp: &str) -> (String, Option<String>) {
    let mut mid: Option<String> = None;
    let mut ufrag: Option<String> = None;
    for raw_line in sdp.lines() {
        let line = raw_line.trim();
        if mid.is_none() {
            if let Some(val) = line.strip_prefix("a=mid:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    mid = Some(trimmed.to_string());
                }
            }
        }
        if ufrag.is_none() {
            if let Some(val) = line.strip_prefix("a=ice-ufrag:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    ufrag = Some(trimmed.to_string());
                }
            }
        }
        if mid.is_some() && ufrag.is_some() {
            break;
        }
    }
    (mid.unwrap_or_else(|| "0".to_string()), ufrag)
}

/// Serializa candidato ICE em formato compativel com navegadores WebRTC (sem `sdpMid: null` nem `usernameFragment: null`).
pub fn format_ice_candidate_json(candidate: &str, sdp_mid: &str, ufrag: Option<&str>) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("candidate".to_string(), Value::String(candidate.to_string()));
    obj.insert("sdpMid".to_string(), Value::String(sdp_mid.to_string()));
    obj.insert("sdpMLineIndex".to_string(), Value::from(0u16));
    if let Some(u) = ufrag.map(str::trim).filter(|s| !s.is_empty()) {
        obj.insert("usernameFragment".to_string(), Value::String(u.to_string()));
    }
    Value::Object(obj)
}

/// Extrai hostname `.local` e porta de um candidato mDNS gerado por navegadores Windows/Chrome.
pub fn extract_mdns_host_and_port(candidate: &str) -> Option<(String, u16)> {
    let body = candidate.strip_prefix("candidate:").unwrap_or(candidate);
    let parts: Vec<&str> = body.split_ascii_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }
    let host = parts[4];
    let port: u16 = parts[5].parse().ok()?;
    if host.to_ascii_lowercase().ends_with(".local") {
        Some((host.to_string(), port))
    } else {
        None
    }
}

/// Substitui o endereco host de uma linha `candidate:...` por um IP resolvido via DNS/mDNS do SO.
pub fn rewrite_candidate_host(candidate: &str, new_ip: &str) -> Option<String> {
    let has_prefix = candidate.starts_with("candidate:");
    let body = candidate.strip_prefix("candidate:").unwrap_or(candidate);
    let mut parts: Vec<String> = body.split_ascii_whitespace().map(str::to_string).collect();
    if parts.len() < 6 {
        return None;
    }
    parts[4] = new_ip.to_string();
    let joined = parts.join(" ");
    if has_prefix {
        Some(format!("candidate:{}", joined))
    } else {
        Some(joined)
    }
}

/// Filtra IPs locais para ICE: aceita apenas IPv4 roteavel/privado (evita bug IPv6 prflx no `webrtc-ice` 0.11 e APIPA 169.254.x.x).
pub fn is_usable_local_ice_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_unspecified()
                && !v4.is_multicast()
                && !v4.is_broadcast()
        }
        IpAddr::V6(_) => false,
    }
}

async fn add_remote_ice_with_mdns_fallback(pc: &Arc<RTCPeerConnection>, cand: RTCIceCandidateInit) {
    if cand.candidate.trim().is_empty() {
        return;
    }

    let _ = pc.add_ice_candidate(cand.clone()).await;

    if let Some((host, port)) = extract_mdns_host_and_port(&cand.candidate) {
        let pc_weak = Arc::downgrade(pc);
        let cand_clone = cand.clone();
        tokio::spawn(async move {
            let Ok(Ok(addrs)) =
                tokio::time::timeout(Duration::from_secs(2), tokio::net::lookup_host((host.as_str(), port)))
                    .await
            else {
                return;
            };
            let mut ips: Vec<IpAddr> = Vec::new();
            for addr in addrs {
                let ip = addr.ip();
                if is_usable_local_ice_ip(ip) && !ips.contains(&ip) {
                    ips.push(ip);
                }
            }
            let Some(pc_ref) = pc_weak.upgrade() else {
                return;
            };
            for ip in ips {
                if let Some(rewritten) = rewrite_candidate_host(&cand_clone.candidate, &ip.to_string()) {
                    let next = RTCIceCandidateInit {
                        candidate: rewritten,
                        sdp_mid: cand_clone.sdp_mid.clone(),
                        sdp_mline_index: cand_clone.sdp_mline_index,
                        username_fragment: cand_clone.username_fragment.clone(),
                    };
                    let _ = pc_ref.add_ice_candidate(next).await;
                }
            }
        });
    }
}

fn candidate_type_str(t: CandidateType) -> &'static str {
    match t {
        CandidateType::Host => "host",
        CandidateType::ServerReflexive => "srflx",
        CandidateType::PeerReflexive => "prflx",
        CandidateType::Relay => "relay",
        CandidateType::Unspecified => "unknown",
    }
}

pub fn classify_route(local_type: &str, remote_type: &str) -> RouteKind {
    if local_type == "unknown" && remote_type == "unknown" {
        return RouteKind::Unknown;
    }
    match (local_type == "relay", remote_type == "relay") {
        (true, true) => RouteKind::RelayBoth,
        (true, false) => RouteKind::RelayLocal,
        (false, true) => RouteKind::RelayRemote,
        (false, false) => RouteKind::Direct,
    }
}

fn read_route_and_remote_inbound(report: &StatsReport) -> (RouteStats, Option<f32>, Option<u32>) {
    let mut pairs: Vec<&ICECandidatePairStats> = Vec::new();
    let mut local_by_id: HashMap<&str, &ICECandidateStats> = HashMap::new();
    let mut remote_by_id: HashMap<&str, &ICECandidateStats> = HashMap::new();
    let mut remote_inbound_rtt: Option<u32> = None;
    let mut remote_loss: Option<f32> = None;

    for stat in report.reports.values() {
        match stat {
            StatsReportType::CandidatePair(p) => pairs.push(p),
            StatsReportType::LocalCandidate(c) => {
                local_by_id.insert(c.id.as_str(), c);
            }
            StatsReportType::RemoteCandidate(c) => {
                remote_by_id.insert(c.id.as_str(), c);
            }
            StatsReportType::RemoteInboundRTP(ri) => {
                let frac = (ri.fraction_lost as f32).clamp(0.0, 1.0);
                remote_loss = Some(remote_loss.map_or(frac, |prev| prev.max(frac)));
                if let Some(rtt) = ri.round_trip_time {
                    if rtt > 0.0 && remote_inbound_rtt.is_none() {
                        remote_inbound_rtt = Some((rtt * 1000.0).round() as u32);
                    }
                }
            }
            _ => {}
        }
    }

    let selected_pair = pairs
        .iter()
        .copied()
        .filter(|p| p.nominated && p.state == CandidatePairState::Succeeded)
        .max_by_key(|p| {
            (
                p.bytes_sent + p.bytes_received,
                p.current_round_trip_time > 0.0,
            )
        })
        .or_else(|| {
            pairs
                .iter()
                .copied()
                .filter(|p| p.state == CandidatePairState::Succeeded)
                .max_by_key(|p| {
                    (
                        p.nominated,
                        p.bytes_sent + p.bytes_received,
                        p.current_round_trip_time > 0.0,
                    )
                })
        });

    let Some(pair) = selected_pair else {
        return (
            RouteStats {
                kind: RouteKind::Unknown,
                local_type: "unknown".to_string(),
                remote_type: "unknown".to_string(),
                rtt_ms: remote_inbound_rtt,
                ..Default::default()
            },
            remote_loss,
            remote_inbound_rtt,
        );
    };

    let local = local_by_id.get(pair.local_candidate_id.as_str()).copied();
    let remote = remote_by_id.get(pair.remote_candidate_id.as_str()).copied();
    let local_type = local.map_or("unknown", |c| candidate_type_str(c.candidate_type));
    let remote_type = remote.map_or("unknown", |c| candidate_type_str(c.candidate_type));
    let protocol = local.and_then(|c| {
        if c.network_type.is_udp() {
            Some("udp".to_string())
        } else if c.network_type.is_tcp() {
            Some("tcp".to_string())
        } else {
            None
        }
    });
    let relay_protocol = local.and_then(|c| {
        if c.candidate_type == CandidateType::Relay && !c.relay_protocol.is_empty() {
            Some(c.relay_protocol.to_ascii_lowercase())
        } else {
            None
        }
    });
    let relay_url = local.and_then(|c| {
        if c.candidate_type == CandidateType::Relay && !c.url.is_empty() {
            Some(c.url.clone())
        } else {
            None
        }
    });
    let pair_rtt = if pair.current_round_trip_time > 0.0 {
        Some((pair.current_round_trip_time * 1000.0).round() as u32)
    } else {
        remote_inbound_rtt
    };
    let available_outgoing_kbps = if pair.available_outgoing_bitrate > 0.0 {
        Some((pair.available_outgoing_bitrate / 1000.0).round() as u32)
    } else {
        None
    };

    let route = RouteStats {
        kind: classify_route(local_type, remote_type),
        local_type: local_type.to_string(),
        remote_type: remote_type.to_string(),
        protocol,
        relay_protocol,
        relay_url,
        rtt_ms: pair_rtt,
        available_outgoing_kbps,
    };
    (route, remote_loss, remote_inbound_rtt)
}

/// Diagnostica causas provaveis de degradacao espelhando os limiares oficiais do GoLive.
pub fn diagnose_connection(
    route: Option<&RouteStats>,
    send: Option<&SendVideoMetrics>,
    recv: Option<&RecvVideoMetrics>,
) -> Vec<DiagCause> {
    let mut causes: Vec<DiagCause> = Vec::new();

    if let Some(r) = route {
        if r.kind.is_relayed() {
            match r.relay_protocol.as_deref() {
                Some("tcp") | Some("tls") => causes.push(DiagCause {
                    id: "turn-tcp",
                    severity: CauseSeverity::High,
                    message_pt: "A conexão passa por um servidor intermediário (TURN) via TCP, o que causa travadas.",
                }),
                _ => causes.push(DiagCause {
                    id: "turn",
                    severity: CauseSeverity::Medium,
                    message_pt: "A conexão passa por um servidor intermediário (TURN) em vez de ser direta.",
                }),
            }
        }
    }

    if let Some(s) = send {
        let cpu = s.limitation == "cpu" || s.cpu_limited_share >= 0.30;
        let bandwidth = s.limitation == "bandwidth" || s.bandwidth_limited_share >= 0.30;
        if cpu {
            causes.push(DiagCause {
                id: "sender-cpu",
                severity: CauseSeverity::High,
                message_pt: "O processador de quem transmite está no limite.",
            });
        }
        if bandwidth {
            causes.push(DiagCause {
                id: "sender-bandwidth",
                severity: CauseSeverity::High,
                message_pt: "A conexão até quem assiste está no limite: pode ser o upload de quem transmite, a internet de quem assiste ou o caminho entre vocês.",
            });
        }
        if s.hardware == Some(false) && (cpu || s.cpu_limited_share >= 0.10) {
            causes.push(DiagCause {
                id: "software-encoder",
                severity: CauseSeverity::Medium,
                message_pt: "Quem transmite está codificando em software (sem aceleração de hardware).",
            });
        }
    }

    let loss = recv
        .map_or(0.0_f32, |r| r.loss)
        .max(send.map_or(0.0_f32, |s| s.remote_loss));
    if loss >= 0.08 {
        causes.push(DiagCause {
            id: "network-loss",
            severity: CauseSeverity::High,
            message_pt: "Pacotes se perdendo na rede entre vocês.",
        });
    } else if loss >= 0.03 {
        causes.push(DiagCause {
            id: "network-loss",
            severity: CauseSeverity::Medium,
            message_pt: "Pacotes se perdendo na rede entre vocês.",
        });
    }

    if let Some(rtt) = route.and_then(|r| r.rtt_ms) {
        if rtt >= 250 {
            causes.push(DiagCause {
                id: "high-rtt",
                severity: CauseSeverity::Medium,
                message_pt: "Latência alta entre vocês (distância ou rede lenta).",
            });
        }
    }

    if let Some(rv) = recv {
        if rv.drop_share >= 0.10 {
            causes.push(DiagCause {
                id: "receiver-drops",
                severity: CauseSeverity::High,
                message_pt: "Este computador está descartando quadros — não está dando conta de decodificar.",
            });
        }
        if rv.freezes > 0 || rv.freeze_seconds > 0.0 {
            causes.push(DiagCause {
                id: "freezes",
                severity: CauseSeverity::Medium,
                message_pt: "A imagem travou por alguns instantes nesta tela.",
            });
        }
    }

    causes.sort_by_key(|c| match c.severity {
        CauseSeverity::High => 0u8,
        CauseSeverity::Medium => 1u8,
    });
    causes
}

fn parse_remote_diag(val: &Value, received_at_ms: u64) -> Option<RemoteSenderDiag> {
    let obj = val.as_object()?;
    let send = obj.get("send").and_then(|s| s.as_object()).map(|s| {
        let f = |k: &str| s.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let u = |k: &str| s.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0).max(0.0).round() as u32;
        let opt_str = |k: &str| s.get(k).and_then(|v| v.as_str()).map(|x| x.to_string());
        SendVideoMetrics {
            width: u("width"),
            height: u("height"),
            fps: u("fps"),
            kbps: u("kbps"),
            limitation: opt_str("limitation").unwrap_or_else(|| "none".to_string()),
            cpu_limited_share: f("cpuLimitedShare").clamp(0.0, 1.0),
            bandwidth_limited_share: f("bandwidthLimitedShare").clamp(0.0, 1.0),
            encoder: opt_str("encoder"),
            hardware: s.get("hardware").and_then(|v| v.as_bool()),
            codec: opt_str("codec"),
            remote_loss: f("remoteLoss").clamp(0.0, 1.0),
            nack_per_second: f("nackPerSecond").max(0.0),
            pli_per_second: f("pliPerSecond").max(0.0),
            target_kbps: s
                .get("targetKbps")
                .and_then(|v| v.as_f64())
                .map(|n| n.max(0.0).round() as u32),
        }
    });

    let route = obj.get("route").and_then(|r| r.as_object()).map(|r| {
        let local_type = r
            .get("localType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let remote_type = r
            .get("remoteType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let kind = match r.get("kind").and_then(|v| v.as_str()) {
            Some("direct") => RouteKind::Direct,
            Some("relay-local") => RouteKind::RelayLocal,
            Some("relay-remote") => RouteKind::RelayRemote,
            Some("relay-both") => RouteKind::RelayBoth,
            Some("unknown") => RouteKind::Unknown,
            _ => classify_route(&local_type, &remote_type),
        };
        RouteStats {
            kind,
            local_type,
            remote_type,
            protocol: r.get("protocol").and_then(|v| v.as_str()).map(str::to_string),
            relay_protocol: r
                .get("relayProtocol")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            relay_url: r
                .get("relayUrl")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            rtt_ms: r
                .get("rttMs")
                .and_then(|v| v.as_f64())
                .map(|n| n.max(0.0).round() as u32),
            available_outgoing_kbps: r
                .get("availableOutgoingKbps")
                .and_then(|v| v.as_f64())
                .map(|n| n.max(0.0).round() as u32),
        }
    });

    Some(RemoteSenderDiag {
        send,
        route,
        cores: obj
            .get("cores")
            .and_then(|v| v.as_u64())
            .map(|c| c as u32),
        tier: obj
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(80).collect()),
        profile: obj
            .get("profile")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(80).collect()),
        received_at_ms,
    })
}

fn merge_viewer_route(mut local: RouteStats, remote: Option<&RouteStats>) -> RouteStats {
    if let Some(rem) = remote {
        if local.kind == RouteKind::Unknown && rem.kind != RouteKind::Unknown {
            // Na perspectiva do transmissor local/remote sao espelhados
            local.kind = match rem.kind {
                RouteKind::RelayLocal => RouteKind::RelayRemote,
                RouteKind::RelayRemote => RouteKind::RelayLocal,
                other => other,
            };
        }
        if local.relay_protocol.is_none() {
            if let Some(ref rp) = rem.relay_protocol {
                local.relay_protocol = Some(rp.clone());
            }
        }
        if local.rtt_ms.is_none() {
            local.rtt_ms = rem.rtt_ms;
        }
    }
    local
}

fn ice_is_up(state: RTCIceConnectionState) -> bool {
    matches!(
        state,
        RTCIceConnectionState::Connected | RTCIceConnectionState::Completed
    )
}

fn compute_recv_metrics(
    cur: &RecvCounterSample,
    prev: Option<&RecvCounterSample>,
) -> RecvVideoMetrics {
    let mut fps = 0u32;
    let mut kbps = 0u32;
    let mut loss = 0.0_f32;
    let mut drop_share = 0.0_f32;
    let mut freezes = 0u32;
    let mut freeze_seconds = 0.0_f32;

    if let Some(p) = prev {
        let secs = cur.at.saturating_duration_since(p.at).as_secs_f64();
        if secs >= 0.2 {
            let d_bytes = cur.bytes_received.saturating_sub(p.bytes_received);
            let d_recv = cur.packets_received.saturating_sub(p.packets_received);
            let d_lost = cur.packets_lost.saturating_sub(p.packets_lost);
            let d_frames_recv = cur.frames_received.saturating_sub(p.frames_received);
            let d_dropped = cur.frames_dropped.saturating_sub(p.frames_dropped);
            let d_decoded = cur.frames_decoded.saturating_sub(p.frames_decoded);
            let d_freezes = cur.freeze_count.saturating_sub(p.freeze_count);
            let d_freeze_ms = cur.freeze_duration_ms.saturating_sub(p.freeze_duration_ms);

            kbps = ((d_bytes as f64 * 8.0 / secs) / 1000.0).round() as u32;
            fps = (d_decoded as f64 / secs).round() as u32;
            let total_pkts = d_recv + d_lost;
            if total_pkts > 0 {
                loss = ((d_lost as f64 / total_pkts as f64) as f32).clamp(0.0, 1.0);
            }
            if d_frames_recv > 0 {
                drop_share = ((d_dropped as f64 / d_frames_recv as f64) as f32).clamp(0.0, 1.0);
            }
            freezes = d_freezes;
            freeze_seconds = (d_freeze_ms as f32) / 1000.0;
        }
    }

    RecvVideoMetrics {
        width: cur.width,
        height: cur.height,
        fps,
        kbps,
        loss,
        drop_share,
        freezes,
        freeze_seconds,
        codec: Some("H264".to_string()),
        hardware: Some(false),
        decoder: Some("OpenH264".to_string()),
        frames_decoded_total: cur.frames_decoded,
    }
}

fn compute_send_metrics(
    meta: &LocalStreamMeta,
    cur: &SendCounterSample,
    prev: Option<&SendCounterSample>,
    remote_loss: Option<f32>,
) -> SendVideoMetrics {
    let mut fps = 0u32;
    let mut kbps = 0u32;
    let mut nack_per_second = 0.0_f32;
    let mut pli_per_second = 0.0_f32;

    if let Some(p) = prev {
        let secs = cur.at.saturating_duration_since(p.at).as_secs_f64();
        if secs >= 0.2 {
            let d_bytes = cur.bytes_sent.saturating_sub(p.bytes_sent);
            let d_frames = cur.frames_sent.saturating_sub(p.frames_sent);
            let d_nack = cur.nack_count.saturating_sub(p.nack_count);
            let d_pli = cur.pli_count.saturating_sub(p.pli_count);
            kbps = ((d_bytes as f64 * 8.0 / secs) / 1000.0).round() as u32;
            fps = (d_frames as f64 / secs).round() as u32;
            nack_per_second = (d_nack as f64 / secs) as f32;
            pli_per_second = (d_pli as f64 / secs) as f32;
        }
    }

    let encoder_label = if meta.encoder_name.trim().is_empty() {
        "OpenH264".to_string()
    } else {
        meta.encoder_name.clone()
    };

    SendVideoMetrics {
        width: meta.width,
        height: meta.height,
        fps,
        kbps,
        limitation: "none".to_string(),
        cpu_limited_share: 0.0,
        bandwidth_limited_share: 0.0,
        encoder: Some(encoder_label),
        hardware: Some(meta.hardware),
        codec: Some("H264".to_string()),
        remote_loss: remote_loss.unwrap_or(0.0),
        nack_per_second,
        pli_per_second,
        target_kbps: if meta.target_bitrate_kbps > 0 {
            Some(meta.target_bitrate_kbps)
        } else {
            None
        },
    }
}

pub fn compute_monotonic_rtp_ts_90khz(
    pts: Duration,
    base_pts: &mut Option<Duration>,
    base_ts: u32,
    last_rtp_ts: &mut Option<u32>,
) -> u32 {
    let anchor = *base_pts.get_or_insert(pts);
    let elapsed = pts.saturating_sub(anchor);
    let ticks_u64 = (elapsed.as_nanos() * 90_000 / 1_000_000_000) as u64;
    let mut ts = base_ts.wrapping_add((ticks_u64 & 0xFFFF_FFFF) as u32);
    if let Some(prev) = *last_rtp_ts {
        let diff = ts.wrapping_sub(prev) as i32;
        if diff <= 0 {
            ts = prev.wrapping_add(1);
        }
    }
    *last_rtp_ts = Some(ts);
    ts
}

fn spawn_video_peer_sender_task(
    track: Arc<TrackLocalStaticRTP>,
    counters: Arc<SendCounters>,
    pc_id: u64,
    kf_flag: Arc<AtomicBool>,
    kf_last_ms: Arc<AtomicU64>,
) -> (mpsc::Sender<VideoFanoutFrame>, Arc<AtomicBool>) {
    let (tx, mut rx) = mpsc::channel::<VideoFanoutFrame>(12);
    let need_key = Arc::new(AtomicBool::new(true));
    let need_key_task = need_key.clone();

    tokio::spawn(async move {
        let mut payloader = H264Payloader::default();
        let mut seq = ((now_epoch_ms() ^ pc_id.wrapping_mul(0x9E37)) & 0xFFFF) as u16;
        let base_ts = ((now_epoch_ms().wrapping_mul(90)) ^ pc_id.wrapping_mul(0x517C_C1B7)) as u32;
        let mut base_pts: Option<Duration> = None;
        let mut last_rtp_ts: Option<u32> = None;
        let mut pending: VecDeque<VideoFanoutFrame> = VecDeque::with_capacity(12);
        let playout_ext = [HeaderExtension::Custom {
            uri: Cow::Borrowed(PLAYOUT_DELAY_URI),
            extension: Box::new(ZeroPlayoutDelayExt),
        }];

        while let Some(first) = rx.recv().await {
            pending.push_back(first);
            while let Ok(more) = rx.try_recv() {
                pending.push_back(more);
            }
            // Se houver um IDR mais recente na fila, descarta frames antigos antes dele sem quebrar delta chain
            if let Some(last_idr_idx) = pending.iter().rposition(|f| f.is_keyframe) {
                if last_idr_idx > 0 {
                    pending.drain(..last_idr_idx);
                }
            }
            // Se acumulou > 6 quadros sem conseguir drenar em tempo real, descarta o backlog obsoleto e pede IDR limpo
            if pending.len() > 6 && !need_key_task.load(Ordering::Acquire) {
                pending.clear();
                need_key_task.store(true, Ordering::Release);
                request_keyframe_coalesced(&kf_flag, &kf_last_ms, KEYFRAME_PLI_COOLDOWN_MS);
                continue;
            }

            while let Some(frame) = pending.pop_front() {
                if !frame.is_keyframe && need_key_task.load(Ordering::Acquire) {
                    request_keyframe_coalesced(&kf_flag, &kf_last_ms, KEYFRAME_PLI_COOLDOWN_MS);
                    continue;
                }

                let Ok(payloads) = payloader.payload(H264_RTP_MTU, &frame.data) else {
                    continue;
                };
                if payloads.is_empty() {
                    continue;
                }

                let rtp_ts = compute_monotonic_rtp_ts_90khz(
                    frame.pts,
                    &mut base_pts,
                    base_ts,
                    &mut last_rtp_ts,
                );
                let total_pkts = payloads.len();
                let mut wrote_zero = false;
                let mut write_failed = false;
                let mut sent_bytes = 0u64;

                for (idx, payload) in payloads.into_iter().enumerate() {
                    let pkt = RtpPacket {
                        header: RtpHeader {
                            version: 2,
                            marker: idx + 1 == total_pkts,
                            sequence_number: seq,
                            timestamp: rtp_ts,
                            ..Default::default()
                        },
                        payload,
                    };
                    seq = seq.wrapping_add(1);

                    let write_res = if idx == 0 || idx + 1 == total_pkts {
                        track.write_rtp_with_extensions(&pkt, &playout_ext).await
                    } else {
                        track.write_rtp(&pkt).await
                    };

                    match write_res {
                        Ok(0) => {
                            wrote_zero = true;
                            break;
                        }
                        Ok(_) => {
                            sent_bytes += pkt.payload.len() as u64;
                        }
                        Err(_) => {
                            write_failed = true;
                            break;
                        }
                    }

                    // P-frames normais (<= 96 pacotes) enviam sem sleep; apenas IDRs gigantes (> 96 pacotes, ex: 4K)
                    // fazem pausa de 1ms a cada 64 pacotes para nao estourar o buffer UDP sem travar quadros menores.
                    if total_pkts > 96 && (idx + 1) % 64 == 0 && (idx + 1) < total_pkts {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    } else if total_pkts > 64 && (idx + 1) % 48 == 0 && (idx + 1) < total_pkts {
                        tokio::task::yield_now().await;
                    }
                }

                if wrote_zero || write_failed {
                    need_key_task.store(true, Ordering::Release);
                    request_keyframe_coalesced(&kf_flag, &kf_last_ms, KEYFRAME_PLI_COOLDOWN_MS);
                } else {
                    if frame.is_keyframe {
                        need_key_task.store(false, Ordering::Release);
                    }
                    counters.bytes_sent.fetch_add(sent_bytes, Ordering::Relaxed);
                    counters.frames_sent.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    (tx, need_key)
}

pub fn compute_monotonic_audio_rtp_ts_48khz(
    frame_seq: u64,
    base_ts: u32,
    last_frame_seq: &mut Option<u64>,
    last_rtp_ts: &mut Option<u32>,
) -> u32 {
    let ts = match (*last_frame_seq, *last_rtp_ts) {
        (Some(prev_seq), Some(prev_ts)) => {
            if frame_seq > prev_seq {
                let delta = frame_seq - prev_seq;
                prev_ts.wrapping_add(((delta.wrapping_mul(960)) & 0xFFFF_FFFF) as u32)
            } else {
                prev_ts.wrapping_add(960)
            }
        }
        _ => base_ts,
    };
    *last_frame_seq = Some(frame_seq);
    *last_rtp_ts = Some(ts);
    ts
}

fn spawn_audio_peer_sender_task(
    track: Arc<TrackLocalStaticRTP>,
    counters: Option<Arc<SendCounters>>,
    pc_id: u64,
) -> mpsc::Sender<AudioFanoutPacket> {
    let (tx, mut rx) = mpsc::channel::<AudioFanoutPacket>(16);

    tokio::spawn(async move {
        let mut seq = ((now_epoch_ms() ^ pc_id.wrapping_mul(0x9E37)) & 0xFFFF) as u16;
        let base_ts =
            ((now_epoch_ms().wrapping_mul(48)) ^ pc_id.wrapping_mul(0x517C_C1B7)) as u32;
        let mut last_frame_seq: Option<u64> = None;
        let mut last_rtp_ts: Option<u32> = None;

        while let Some(pkt) = rx.recv().await {
            let rtp_ts = compute_monotonic_audio_rtp_ts_48khz(
                pkt.frame_seq,
                base_ts,
                &mut last_frame_seq,
                &mut last_rtp_ts,
            );
            let payload_len = pkt.data.len() as u64;
            let rtp_pkt = RtpPacket {
                header: RtpHeader {
                    version: 2,
                    marker: pkt.marker,
                    sequence_number: seq,
                    timestamp: rtp_ts,
                    ..Default::default()
                },
                payload: pkt.data,
            };
            seq = seq.wrapping_add(1);
            if track.write_rtp(&rtp_pkt).await.is_ok() {
                if let Some(ref c) = counters {
                    c.bytes_sent.fetch_add(payload_len, Ordering::Relaxed);
                    c.frames_sent.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    tx
}

pub fn spawn_webrtc_actor(
    rt: &tokio::runtime::Runtime,
    sig_tx: mpsc::UnboundedSender<SignalingCommand>,
    mixer: Arc<AudioMixer>,
    egui_ctx: egui::Context,
    keyframe_signals: KeyframeSignals,
    mic_frame_rx: std::sync::mpsc::Receiver<CapturedAudioFrame>,
    policy_map: VideoRenderPolicyMap,
) -> (
    mpsc::UnboundedSender<WebRtcCommand>,
    mpsc::UnboundedReceiver<WebRtcEvent>,
    AudioFanoutBus,
    VideoFanoutBus,
    RemoteVideoMailbox,
) {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<WebRtcCommand>();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel::<WebRtcEvent>();
    let self_cmd_tx = cmd_tx.clone();
    let audio_fanout = AudioFanoutBus::new();
    let audio_fanout_enc = audio_fanout.clone();
    let audio_fanout_ret = audio_fanout.clone();
    let video_fanout = VideoFanoutBus::new();
    let video_fanout_ret = video_fanout.clone();
    let remote_video_mailbox = RemoteVideoMailbox::new();
    let remote_video_mailbox_ret = remote_video_mailbox.clone();

    // Thread dedicada em alta prioridade para Opus + fanout sem competir com video/sinalizacao no Tokio
    let _ = std::thread::Builder::new()
        .name("golive-mic-enc".to_string())
        .spawn(move || {
            let _prio = AudioThreadPriorityGuard::enter_pro_audio(false);
            let mut opus_encoder = OpusVoiceEncoder::new().ok();

            while let Ok(frame) = mic_frame_rx.recv() {
                if !audio_fanout_enc.has_targets("mic") {
                    continue;
                }
                if opus_encoder.is_none() {
                    opus_encoder = OpusVoiceEncoder::new().ok();
                }
                let Some(ref mut enc) = opus_encoder else {
                    continue;
                };
                let Some(encoded) = enc.encode_frame(&frame.samples) else {
                    continue;
                };
                audio_fanout_enc.send_encoded_audio(
                    "mic",
                    encoded,
                    frame.frame_seq,
                    frame.talkspurt_start,
                );
            }
        });

    rt.spawn(async move {
        let api = build_webrtc_api();
        let state = Arc::new(Mutex::new(WebRtcState {
            api,
            self_id: None,
            ice_servers: default_stun_servers(),
            turn_configured: false,
            next_pc_id: 1,
            active_send_channels: HashSet::new(),
            channel_has_audio: HashMap::new(),
            known_peers: HashSet::new(),
            viewer_stopped_peers: HashSet::new(),
            send_retry_attempts: HashMap::new(),
            send_pcs: HashMap::new(),
            recv_pcs: HashMap::new(),
            stopped_by_us: HashSet::new(),
            pending_recv_ice: HashMap::new(),
            pending_send_ice: HashMap::new(),
            keyframe_signals,
            kf_cooldowns: new_keyframe_cooldowns(),
            local_stream_meta: HashMap::new(),
            watched_stats: None,
            watch_started_at: None,
            last_diag_request_at: None,
            latest_remote_diag: None,
            last_remote_snapshot: None,
            prev_recv_sample: None,
            prev_send_samples: HashMap::new(),
            prev_diag_send_samples: HashMap::new(),
            diag_responders: HashMap::new(),
            video_fanout: video_fanout.clone(),
            audio_fanout,
            remote_video_mailbox: remote_video_mailbox.clone(),
            policy_map,
        }));

        let mut stats_tick = tokio::time::interval(Duration::from_secs(STATS_POLL_SECS));
        stats_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_cmd = cmd_rx.recv() => {
                    let Some(cmd) = maybe_cmd else {
                        break;
                    };
                    match cmd {
                        WebRtcCommand::SetIceServers(list) => {
                            let (sanitized, has_turn) = sanitize_ice_servers(list);
                            let mut st = state.lock().await;
                            st.ice_servers = sanitized;
                            st.turn_configured = has_turn;
                        }
                        WebRtcCommand::SetSelfId(id) => {
                            state.lock().await.self_id = Some(id);
                        }
                        WebRtcCommand::UpdateLocalStreamMeta {
                            channel,
                            width,
                            height,
                            target_fps,
                            target_bitrate_kbps,
                            encoder_name,
                            hardware,
                        } => {
                            state.lock().await.local_stream_meta.insert(
                                channel,
                                LocalStreamMeta {
                                    width,
                                    height,
                                    target_fps,
                                    target_bitrate_kbps,
                                    encoder_name,
                                    hardware,
                                },
                            );
                        }
                        WebRtcCommand::SetWatchedStats(target) => {
                            let initial_diag_req = {
                                let mut st = state.lock().await;
                                st.watched_stats = target.clone();
                                st.prev_recv_sample = None;
                                st.prev_send_samples.clear();
                                st.latest_remote_diag = None;
                                st.last_remote_snapshot = None;
                                match &target {
                                    Some(StatsTarget::Remote { channel, peer_id }) => {
                                        let now = Instant::now();
                                        st.watch_started_at = Some(now);
                                        st.last_diag_request_at = Some(now);
                                        Some((channel.clone(), peer_id.clone()))
                                    }
                                    Some(StatsTarget::Own { .. }) | None => {
                                        st.watch_started_at = None;
                                        st.last_diag_request_at = None;
                                        None
                                    }
                                }
                            };
                            if let Some((ch, pid)) = initial_diag_req {
                                let _ = sig_tx.send(SignalingCommand::SendSignal {
                                    to: pid.clone(),
                                    data: SignalData {
                                        channel: Some(ch),
                                        role: Some("viewer".to_string()),
                                        kind: Some("diag-request".to_string()),
                                        origin_id: Some(pid),
                                        ..Default::default()
                                    },
                                });
                            }
                            if target.is_some() {
                                emit_watched_stats(&state, &evt_tx, &egui_ctx).await;
                            }
                        }
                        WebRtcCommand::StartChannel {
                            channel,
                            peers,
                            with_audio,
                        } => {
                            {
                                let mut st = state.lock().await;
                                st.active_send_channels.insert(channel.clone());
                                st.channel_has_audio.insert(channel.clone(), with_audio);
                                for p in &peers {
                                    st.known_peers.insert(p.clone());
                                    st.viewer_stopped_peers.remove(&(channel.clone(), p.clone()));
                                    st.send_retry_attempts.remove(&(channel.clone(), p.clone()));
                                }
                            }
                            for peer_id in peers {
                                open_send_pc(&state, &sig_tx, &self_cmd_tx, &channel, &peer_id).await;
                            }
                        }
                        WebRtcCommand::StopChannel { channel } => {
                            let to_close = {
                                let mut st = state.lock().await;
                                st.active_send_channels.remove(&channel);
                                st.channel_has_audio.remove(&channel);
                                st.local_stream_meta.remove(&channel);
                                st.viewer_stopped_peers.retain(|(ch, _)| ch != &channel);
                                st.send_retry_attempts.retain(|(ch, _), _| ch != &channel);
                                st.diag_responders.retain(|(ch, _), _| ch != &channel);
                                st.prev_send_samples.retain(|(ch, _, _), _| ch != &channel);
                                st.prev_diag_send_samples.retain(|(ch, _, _), _| ch != &channel);
                                 let keys: Vec<(String, String)> = st
                                    .send_pcs
                                    .keys()
                                    .filter(|(ch, _)| ch == &channel)
                                    .cloned()
                                    .collect();
                                let mut pcs = Vec::new();
                                for k in keys {
                                    st.pending_send_ice.remove(&k);
                                    if let Some(entry) = st.send_pcs.remove(&k) {
                                        pcs.push((k.1, entry.pc));
                                    }
                                }
                                if st.send_pcs.is_empty() {
                                    st.send_pcs.shrink_to_fit();
                                    st.pending_send_ice.shrink_to_fit();
                                    st.prev_send_samples.shrink_to_fit();
                                    st.prev_diag_send_samples.shrink_to_fit();
                                }
                                st.refresh_media_fanout();
                                pcs
                            };
                            for (peer_id, pc) in to_close {
                                let _ = sig_tx.send(SignalingCommand::SendSignal {
                                    to: peer_id,
                                    data: SignalData {
                                        channel: Some(channel.clone()),
                                        role: Some("broadcaster".to_string()),
                                        kind: Some("stop".to_string()),
                                        ..Default::default()
                                    },
                                });
                                let _ = pc.close().await;
                            }
                            crate::win_tray::trim_process_working_set();
                        }
                        WebRtcCommand::SyncPeers { peers } => {
                            let new_set: HashSet<String> = peers.iter().cloned().collect();
                            let (active_channels, removed_pcs, to_open) = {
                                let mut st = state.lock().await;
                                st.known_peers = new_set.clone();

                                // Fecha send_pcs de participantes que sairam da sala
                                let stale_keys: Vec<(String, String)> = st
                                    .send_pcs
                                    .keys()
                                    .filter(|(_, p)| !new_set.contains(p))
                                    .cloned()
                                    .collect();
                                let mut removed = Vec::new();
                                for k in stale_keys {
                                    st.pending_send_ice.remove(&k);
                                    st.send_retry_attempts.remove(&k);
                                    st.viewer_stopped_peers.remove(&k);
                                    st.diag_responders.remove(&k);
                                    if let Some(entry) = st.send_pcs.remove(&k) {
                                        removed.push(entry.pc);
                                    }
                                }
                                st.refresh_media_fanout();

                                let channels: Vec<String> = st.active_send_channels.iter().cloned().collect();
                                let mut missing = Vec::new();
                                for ch in &channels {
                                    for p in &peers {
                                        let key = (ch.clone(), p.clone());
                                        if !st.send_pcs.contains_key(&key)
                                            && !st.viewer_stopped_peers.contains(&key)
                                        {
                                            missing.push(key);
                                        }
                                    }
                                }
                                (channels, removed, missing)
                            };
                            for pc in removed_pcs {
                                let _ = pc.close().await;
                            }
                            let _ = active_channels;
                            for (ch, p) in to_open {
                                open_send_pc(&state, &sig_tx, &self_cmd_tx, &ch, &p).await;
                            }
                        }
                        WebRtcCommand::HandleSignal { from, data } => {
                            handle_signal(
                                &state,
                                &sig_tx,
                                &self_cmd_tx,
                                &mixer,
                                &evt_tx,
                                &egui_ctx,
                                &from,
                                data,
                            )
                            .await;
                        }
                        WebRtcCommand::StopWatchingPeer { channel, peer_id } => {
                            let pc_opt = {
                                let mut st = state.lock().await;
                                let key = (channel.clone(), peer_id.clone());
                                st.stopped_by_us.insert(key.clone());
                                st.pending_recv_ice.remove(&key);
                                st.remote_video_mailbox.remove(&channel, &peer_id);
                                let entry = st.recv_pcs.remove(&key);
                                if st.recv_pcs.is_empty() {
                                    st.recv_pcs.shrink_to_fit();
                                    st.pending_recv_ice.shrink_to_fit();
                                }
                                entry
                            };
                            if let Some(entry) = pc_opt {
                                let _ = entry.pc.close().await;
                            }
                            crate::win_tray::trim_process_working_set();
                            let _ = sig_tx.send(SignalingCommand::SendSignal {
                                to: peer_id,
                                data: SignalData {
                                    channel: Some(channel),
                                    role: Some("viewer".to_string()),
                                    kind: Some("stop".to_string()),
                                    ..Default::default()
                                },
                            });
                        }
                        WebRtcCommand::CleanupPeerChannel { channel, peer_id } => {
                            let pc_opt = {
                                let mut st = state.lock().await;
                                let key = (channel.clone(), peer_id.clone());
                                st.stopped_by_us.remove(&key);
                                st.pending_recv_ice.remove(&key);
                                st.remote_video_mailbox.remove(&channel, &peer_id);
                                let entry = st.recv_pcs.remove(&key);
                                if st.recv_pcs.is_empty() {
                                    st.recv_pcs.shrink_to_fit();
                                    st.pending_recv_ice.shrink_to_fit();
                                }
                                entry
                            };
                            if let Some(entry) = pc_opt {
                                let _ = entry.pc.close().await;
                            }
                            crate::win_tray::trim_process_working_set();
                        }
                        WebRtcCommand::ResumeWatchingPeer { channel, peer_id } => {
                            {
                                let mut st = state.lock().await;
                                st.stopped_by_us.remove(&(channel.clone(), peer_id.clone()));
                            }
                            let _ = evt_tx.send(WebRtcEvent::PeerResuming {
                                channel: channel.clone(),
                                peer_id: peer_id.clone(),
                            });
                            let _ = sig_tx.send(SignalingCommand::SendSignal {
                                to: peer_id,
                                data: SignalData {
                                    channel: Some(channel),
                                    role: Some("viewer".to_string()),
                                    kind: Some("resume".to_string()),
                                    ..Default::default()
                                },
                            });
                        }
                        WebRtcCommand::SendVideoSample {
                            channel,
                            data,
                            is_keyframe,
                            pts,
                        } => {
                            video_fanout.send_encoded_video(&channel, data, is_keyframe, pts);
                        }
                        WebRtcCommand::MarkSendConnected {
                            channel,
                            peer_id,
                            expected_pc_id,
                        } => {
                            let mut st = state.lock().await;
                            let key = (channel.clone(), peer_id);
                            let mut became_connected = false;
                            if let Some(entry) = st.send_pcs.get_mut(&key) {
                                if entry.pc_id == expected_pc_id {
                                    entry.connected = true;
                                    if let Some(ref nk) = entry.peer_need_key {
                                        nk.store(true, Ordering::Release);
                                    }
                                    became_connected = true;
                                }
                            }
                            if became_connected {
                                st.send_retry_attempts.remove(&key);
                                st.refresh_media_fanout();
                                st.request_channel_keyframe(&channel, 0);
                            }
                        }
                        WebRtcCommand::RetrySendPc {
                            channel,
                            peer_id,
                            expected_pc_id,
                        } => {
                            let can_restart_first = {
                                let st = state.lock().await;
                                let key = (channel.clone(), peer_id.clone());
                                st.active_send_channels.contains(&channel)
                                    && st.known_peers.contains(&peer_id)
                                    && !st.viewer_stopped_peers.contains(&key)
                                    && st.send_pcs.get(&key).is_some_and(|e| {
                                        e.pc_id == expected_pc_id
                                            && !e.paused_by_viewer
                                            && e.connected
                                            && e.last_ice_restart_at
                                                .is_none_or(|t| t.elapsed() >= Duration::from_secs(10))
                                    })
                            };
                            if can_restart_first
                                && try_ice_restart_send_pc(&state, &sig_tx, &channel, &peer_id).await
                            {
                                continue;
                            }

                            let (old_pc, retry_delay) = {
                                let mut st = state.lock().await;
                                let key = (channel.clone(), peer_id.clone());
                                let active = st.active_send_channels.contains(&channel);
                                let still_in_room = st.known_peers.contains(&peer_id);
                                let stopped_by_peer = st.viewer_stopped_peers.contains(&key);
                                let mut closed_pc = None;
                                let mut delay = None;

                                if active && !stopped_by_peer {
                                    let should_retry = match st.send_pcs.get(&key) {
                                        Some(e) => {
                                            e.pc_id == expected_pc_id
                                                && !e.paused_by_viewer
                                                && e.pc.connection_state() != RTCPeerConnectionState::Connected
                                        }
                                        None => false,
                                    };
                                    if should_retry {
                                        crate::diag_warn!(
                                            "webrtc",
                                            "RetrySendPc reconstruindo peer channel={} peer={} pc_id={}",
                                            channel,
                                            peer_id,
                                            expected_pc_id
                                        );
                                        st.pending_send_ice.remove(&key);
                                        if let Some(e) = st.send_pcs.remove(&key) {
                                            closed_pc = Some(e.pc);
                                        }
                                        st.refresh_media_fanout();
                                        if still_in_room {
                                            let attempt = st.send_retry_attempts.get(&key).copied().unwrap_or(0) as usize;
                                            if attempt < RETRY_BACKOFF_SECS.len() {
                                                st.send_retry_attempts.insert(key, (attempt + 1) as u32);
                                                delay = Some(Duration::from_secs(RETRY_BACKOFF_SECS[attempt]));
                                            }
                                        }
                                    }
                                }
                                (closed_pc, delay)
                            };
                            if let Some(pc) = old_pc {
                                let _ = pc.close().await;
                            }
                            if let Some(wait) = retry_delay {
                                let tx = self_cmd_tx.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(wait).await;
                                    let _ = tx.send(WebRtcCommand::ReopenSendPc { channel, peer_id });
                                });
                            }
                        }
                        WebRtcCommand::ReopenSendPc { channel, peer_id } => {
                            let should_open = {
                                let st = state.lock().await;
                                let key = (channel.clone(), peer_id.clone());
                                st.active_send_channels.contains(&channel)
                                    && st.known_peers.contains(&peer_id)
                                    && !st.viewer_stopped_peers.contains(&key)
                                    && !st.send_pcs.contains_key(&key)
                            };
                            if should_open {
                                open_send_pc(&state, &sig_tx, &self_cmd_tx, &channel, &peer_id).await;
                            }
                        }
                        WebRtcCommand::CloseAll => {
                            let (sends, recvs) = {
                                let mut st = state.lock().await;
                                st.active_send_channels.clear();
                                st.channel_has_audio.clear();
                                st.known_peers.clear();
                                st.viewer_stopped_peers.clear();
                                st.send_retry_attempts.clear();
                                st.stopped_by_us.clear();
                                st.pending_recv_ice.clear();
                                st.pending_send_ice.clear();
                                st.local_stream_meta.clear();
                                st.watched_stats = None;
                                st.watch_started_at = None;
                                st.last_diag_request_at = None;
                                st.latest_remote_diag = None;
                                st.last_remote_snapshot = None;
                                st.prev_recv_sample = None;
                                st.prev_send_samples.clear();
                                st.prev_diag_send_samples.clear();
                                st.diag_responders.clear();
                                st.remote_video_mailbox.clear();
                                let s: Vec<_> = st.send_pcs.drain().map(|(_, e)| e.pc).collect();
                                let r: Vec<_> = st.recv_pcs.drain().map(|(_, e)| e.pc).collect();
                                st.send_pcs.shrink_to_fit();
                                st.recv_pcs.shrink_to_fit();
                                st.pending_recv_ice.shrink_to_fit();
                                st.pending_send_ice.shrink_to_fit();
                                st.prev_send_samples.shrink_to_fit();
                                st.prev_diag_send_samples.shrink_to_fit();
                                st.refresh_media_fanout();
                                (s, r)
                            };
                            for pc in sends {
                                let _ = pc.close().await;
                            }
                            for pc in recvs {
                                let _ = pc.close().await;
                            }
                            crate::win_tray::trim_process_working_set();
                        }
                    }
                }
                _ = stats_tick.tick() => {
                    let has_work = {
                        let st = state.lock().await;
                        st.watched_stats.is_some() || !st.diag_responders.is_empty()
                    };
                    if has_work {
                        run_stats_and_diag_tick(&state, &sig_tx, &evt_tx, &egui_ctx).await;
                    }
                }
            }
        }
    });

    (
        cmd_tx,
        evt_rx,
        audio_fanout_ret,
        video_fanout_ret,
        remote_video_mailbox_ret,
    )
}

pub fn build_webrtc_api() -> Arc<API> {
    build_webrtc_api_internal(None)
}

fn build_webrtc_api_internal(vnet: Option<Arc<webrtc::util::vnet::net::Net>>) -> Arc<API> {
    let mut setting_engine = SettingEngine::default();
    if let Some(net) = vnet {
        setting_engine.set_ice_multicast_dns_mode(MulticastDnsMode::Disabled);
        setting_engine.set_vnet(Some(net));
    } else {
        setting_engine.set_ice_multicast_dns_mode(MulticastDnsMode::QueryOnly);
    }
    setting_engine.set_network_types(vec![NetworkType::Udp4]);
    setting_engine.set_ip_filter(Box::new(is_usable_local_ice_ip));
    // Timeouts ICE mais tolerantes sob carga maxima de CPU/GPU e keepalive STUN a cada 1s para nao perder consentimento
    setting_engine.set_ice_timeouts(
        Some(Duration::from_secs(12)),
        Some(Duration::from_secs(30)),
        Some(Duration::from_secs(1)),
    );
    setting_engine.set_host_acceptance_min_wait(Some(Duration::from_millis(0)));
    setting_engine.set_prflx_acceptance_min_wait(Some(Duration::from_millis(200)));
    setting_engine.set_srflx_acceptance_min_wait(Some(Duration::from_millis(400)));
    setting_engine.set_relay_acceptance_min_wait(Some(Duration::from_millis(1000)));

    let mut m = MediaEngine::default();

    // Opus 48 kHz padrao WebRTC
    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        RTPCodecType::Audio,
    );

    // configure_nack adiciona "nack" e "nack pli" automaticamente sem duplicar linhas a=rtcp-fb
    // Nao anunciar "goog-remb" porque o receptor TWCC ja guia o GCC sem confusao de REMB nulo.
    let video_rtcp_feedback = vec![RTCPFeedback {
        typ: "ccm".to_string(),
        parameter: "fir".to_string(),
    }];

    // 0. Constrained Baseline Level 5.1 (42e033) + Main Level 5.1 (4d0033) permitem 1080p60/1440p/4K
    // sem limitar taxa de macroblocos em Level 3.1, mantendo compatibilidade total com OpenH264.
    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e033"
                        .to_string(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 100,
            ..Default::default()
        },
        RTPCodecType::Video,
    );

    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=4d0033"
                        .to_string(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 121,
            ..Default::default()
        },
        RTPCodecType::Video,
    );

    // 1. Constrained Baseline (42e01f): obrigatorio no RFC 7742 para todos os navegadores
    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 102,
            ..Default::default()
        },
        RTPCodecType::Video,
    );

    // 2. Main (4d001f): aceito pelos decodificadores hardware (NVDEC/D3D11VideoDecoder) sem B-frames
    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=4d001f"
                        .to_string(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 127,
            ..Default::default()
        },
        RTPCodecType::Video,
    );

    // 3. Fallback Baseline (42001f)
    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f"
                        .to_string(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 125,
            ..Default::default()
        },
        RTPCodecType::Video,
    );

    // 4. Fallback High Level 5.0 (640032) para 4K em navegadores Chromium/Electron
    let _ = m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640032"
                        .to_string(),
                rtcp_feedback: video_rtcp_feedback,
            },
            payload_type: 123,
            ..Default::default()
        },
        RTPCodecType::Video,
    );

    let _ = m.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: PLAYOUT_DELAY_URI.to_string(),
        },
        RTPCodecType::Video,
        None,
    );

    let registry = configure_nack(Registry::new(), &mut m);
    let registry = configure_rtcp_reports(registry);
    let registry = configure_twcc(registry, &mut m).unwrap_or_else(|_| Registry::new());

    Arc::new(
        APIBuilder::new()
            .with_setting_engine(setting_engine)
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build(),
    )
}

async fn open_send_pc(
    state: &Arc<Mutex<WebRtcState>>,
    sig_tx: &mpsc::UnboundedSender<SignalingCommand>,
    self_cmd_tx: &mpsc::UnboundedSender<WebRtcCommand>,
    channel: &str,
    peer_id: &str,
) {
    let (api, ice_servers, kf_signals, kf_cooldowns, pc_id, with_audio) = {
        let mut st = state.lock().await;
        let key = (channel.to_string(), peer_id.to_string());
        if !st.active_send_channels.contains(channel)
            || st.send_pcs.contains_key(&key)
            || st.viewer_stopped_peers.contains(&key)
        {
            return;
        }
        let id = st.next_pc_id;
        st.next_pc_id = st.next_pc_id.wrapping_add(1).max(1);
        let ch_audio = st.channel_has_audio.get(channel).copied().unwrap_or(false);
        (
            st.api.clone(),
            st.ice_servers.clone(),
            st.keyframe_signals.clone(),
            st.kf_cooldowns.clone(),
            id,
            ch_audio,
        )
    };

    let Some(pc) = create_peer_connection(&api, ice_servers).await else {
        return;
    };

    let mut audio_track = None;
    let mut audio_tx = None;
    let mut video_track = None;
    let mut video_tx = None;
    let mut peer_need_key = None;
    let send_counters = Arc::new(SendCounters::default());
    let unconnected_since = Arc::new(std::sync::Mutex::new(Some(Instant::now())));
    let kf_flag = channel_keyframe_flag(&kf_signals, channel);
    let kf_last_ms = channel_keyframe_cooldown(&kf_cooldowns, channel);
    let sendonly_init = Some(RTCRtpTransceiverInit {
        direction: RTCRtpTransceiverDirection::Sendonly,
        send_encodings: vec![],
    });

    if channel == "mic" {
        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                ..Default::default()
            },
            "audio".to_string(),
            "golive-lite-mic".to_string(),
        ));
        if let Ok(transceiver) = pc
            .add_transceiver_from_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>, sendonly_init)
            .await
        {
            let rtp_sender = transceiver.sender().await;
            let tx_peer = spawn_audio_peer_sender_task(
                track.clone(),
                Some(send_counters.clone()),
                pc_id,
            );
            audio_track = Some(track);
            audio_tx = Some(tx_peer);
            tokio::spawn(async move {
                while rtp_sender.read_rtcp().await.is_ok() {}
            });
        }
    } else {
        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            format!("golive-lite-{}", channel),
        ));
        if let Ok(transceiver) = pc
            .add_transceiver_from_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>, sendonly_init)
            .await
        {
            let rtp_sender = transceiver.sender().await;
            let (tx_peer, nk_peer) = spawn_video_peer_sender_task(
                track.clone(),
                send_counters.clone(),
                pc_id,
                kf_flag.clone(),
                kf_last_ms.clone(),
            );
            video_track = Some(track);
            video_tx = Some(tx_peer);
            peer_need_key = Some(nk_peer);

            let kf_rtcp = kf_flag.clone();
            let kf_cd_rtcp = kf_last_ms.clone();
            let cnt_rtcp = send_counters.clone();
            tokio::spawn(async move {
                while let Ok((pkts, _)) = rtp_sender.read_rtcp().await {
                    for pkt in pkts {
                        let any = pkt.as_any();
                        if any.is::<PictureLossIndication>() || any.is::<FullIntraRequest>() {
                            cnt_rtcp.pli_count.fetch_add(1, Ordering::Relaxed);
                            request_keyframe_coalesced(
                                &kf_rtcp,
                                &kf_cd_rtcp,
                                KEYFRAME_PLI_COOLDOWN_MS,
                            );
                    } else if any.is::<TransportLayerNack>() {
                        cnt_rtcp.nack_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });
        }

        if with_audio {
            let aud_track = Arc::new(TrackLocalStaticRTP::new(
                RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_OPUS.to_string(),
                    clock_rate: 48_000,
                    channels: 2,
                    sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                    ..Default::default()
                },
                "audio".to_string(),
                format!("golive-lite-{}", channel),
            ));
            let aud_init = Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Sendonly,
                send_encodings: vec![],
            });
            if let Ok(transceiver) = pc
                .add_transceiver_from_track(
                    aud_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                    aud_init,
                )
                .await
            {
                let rtp_sender = transceiver.sender().await;
                let tx_aud =
                    spawn_audio_peer_sender_task(aud_track.clone(), None, pc_id);
                audio_track = Some(aud_track);
                audio_tx = Some(tx_aud);
                tokio::spawn(async move {
                    while rtp_sender.read_rtcp().await.is_ok() {}
                });
            }
        }
    }

    {
        let unconn_ice = unconnected_since.clone();
        pc.on_ice_connection_state_change(Box::new(move |s: RTCIceConnectionState| {
            let unconn_ice = unconn_ice.clone();
            Box::pin(async move {
                let mut g = unconn_ice.lock().unwrap_or_else(|e| e.into_inner());
                if ice_is_up(s) {
                    *g = None;
                } else if g.is_none() {
                    *g = Some(Instant::now());
                }
            })
        }));
    }

    {
        let cmd_state = self_cmd_tx.clone();
        let ch_state = channel.to_string();
        let peer_state = peer_id.to_string();
        pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            let cmd_state = cmd_state.clone();
            let ch_state = ch_state.clone();
            let peer_state = peer_state.clone();
            Box::pin(async move {
                if ch_state != "mic"
                    || matches!(s, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected)
                {
                    crate::diag_info!(
                        "webrtc",
                        "send_pc state={} channel={} peer={} pc_id={}",
                        s,
                        ch_state,
                        peer_state,
                        pc_id
                    );
                }
                match s {
                    RTCPeerConnectionState::Connected => {
                        let _ = cmd_state.send(WebRtcCommand::MarkSendConnected {
                            channel: ch_state,
                            peer_id: peer_state,
                            expected_pc_id: pc_id,
                        });
                    }
                    RTCPeerConnectionState::Failed => {
                        let _ = cmd_state.send(WebRtcCommand::RetrySendPc {
                            channel: ch_state,
                            peer_id: peer_state,
                            expected_pc_id: pc_id,
                        });
                    }
                    _ => {}
                }
            })
        }));
    }

    let local_ice = Arc::new(std::sync::Mutex::new(PendingLocalIce::new()));
    let local_ice_cb = local_ice.clone();
    let sig_ice = sig_tx.clone();
    let ch_ice = channel.to_string();
    let peer_ice = peer_id.to_string();
    pc.on_ice_candidate(Box::new(move |cand| {
        let local_ice_cb = local_ice_cb.clone();
        let sig_ice = sig_ice.clone();
        let ch_ice = ch_ice.clone();
        let peer_ice = peer_ice.clone();
        Box::pin(async move {
            if let Some(c) = cand {
                if let Ok(json_cand) = c.to_json() {
                    if !json_cand.candidate.is_empty() {
                        let immediate_payload = {
                            let mut guard = local_ice_cb.lock().unwrap_or_else(|e| e.into_inner());
                            if guard.ready {
                                Some(format_ice_candidate_json(
                                    &json_cand.candidate,
                                    &guard.mid,
                                    guard.ufrag.as_deref(),
                                ))
                            } else {
                                guard.queued.push(json_cand.candidate);
                                None
                            }
                        };
                        if let Some(val) = immediate_payload {
                            let _ = sig_ice.send(SignalingCommand::SendSignal {
                                to: peer_ice,
                                data: SignalData {
                                    channel: Some(ch_ice),
                                    role: Some("broadcaster".to_string()),
                                    kind: Some("ice".to_string()),
                                    candidate: Some(val),
                                    ..Default::default()
                                },
                            });
                        }
                    }
                }
            }
        })
    }));

    let inserted = {
        let mut st = state.lock().await;
        let key = (channel.to_string(), peer_id.to_string());
        if st.active_send_channels.contains(channel)
            && !st.send_pcs.contains_key(&key)
            && !st.viewer_stopped_peers.contains(&key)
        {
            st.send_pcs.insert(
                key,
                SendPeerEntry {
                    pc_id,
                    pc: pc.clone(),
                    local_ice: local_ice.clone(),
                    audio_track,
                    audio_tx,
                    video_track,
                    video_tx,
                    peer_need_key,
                    send_counters,
                    unconnected_since,
                    last_ice_restart_at: None,
                    paused_by_viewer: false,
                    remote_desc_set: false,
                    connected: false,
                },
            );
            true
        } else {
            false
        }
    };
    if !inserted {
        let _ = pc.close().await;
        return;
    }

    // Watchdog de 12s para reconstruir conexoes travadas caso offer/ICE nao conclua
    {
        let pc_weak = Arc::downgrade(&pc);
        let cmd_wd = self_cmd_tx.clone();
        let ch_wd = channel.to_string();
        let peer_wd = peer_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(CONNECT_TIMEOUT_SECS)).await;
            let Some(pc_ref) = pc_weak.upgrade() else {
                return;
            };
            let s = pc_ref.connection_state();
            if s != RTCPeerConnectionState::Connected && s != RTCPeerConnectionState::Closed {
                let _ = cmd_wd.send(WebRtcCommand::RetrySendPc {
                    channel: ch_wd,
                    peer_id: peer_wd,
                    expected_pc_id: pc_id,
                });
            }
        });
    }

    if let Ok(offer) = pc.create_offer(None).await {
        if pc.set_local_description(offer.clone()).await.is_ok() {
            if let Ok(sdp_val) = serde_json::to_value(&offer) {
                let _ = sig_tx.send(SignalingCommand::SendSignal {
                    to: peer_id.to_string(),
                    data: SignalData {
                        channel: Some(channel.to_string()),
                        role: Some("broadcaster".to_string()),
                        kind: Some("offer".to_string()),
                        sdp: Some(sdp_val),
                        ..Default::default()
                    },
                });
            }
            flush_local_ice(
                &local_ice,
                &offer.sdp,
                sig_tx,
                peer_id,
                channel,
                "broadcaster",
            );
        }
    }
}

fn reset_local_ice(local_ice: &Arc<std::sync::Mutex<PendingLocalIce>>) {
    let mut guard = local_ice.lock().unwrap_or_else(|e| e.into_inner());
    guard.ready = false;
    guard.queued.clear();
}

/// Reinicia apenas o transporte ICE da PeerConnection existente (`iceRestart: true`),
/// mantendo a mesma `<video>` track no navegador (nao derruba tela cheia no site).
async fn try_ice_restart_send_pc(
    state: &Arc<Mutex<WebRtcState>>,
    sig_tx: &mpsc::UnboundedSender<SignalingCommand>,
    channel: &str,
    peer_id: &str,
) -> bool {
    let prep = {
        let mut st = state.lock().await;
        let key = (channel.to_string(), peer_id.to_string());
        if !st.active_send_channels.contains(channel) || st.viewer_stopped_peers.contains(&key) {
            return false;
        }
        let Some(entry) = st.send_pcs.get_mut(&key) else {
            return false;
        };
        if entry.pc.connection_state() == RTCPeerConnectionState::Closed
            || entry.pc.signaling_state() != RTCSignalingState::Stable
            || entry.pc.ice_gathering_state() != RTCIceGatheringState::Complete
        {
            return false;
        }
        if entry
            .last_ice_restart_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
        {
            return false;
        }
        entry.last_ice_restart_at = Some(Instant::now());
        if let Some(ref nk) = entry.peer_need_key {
            nk.store(true, Ordering::Release);
        }
        Some((entry.pc_id, entry.pc.clone(), entry.local_ice.clone()))
    };

    let Some((pc_id, pc, local_ice)) = prep else {
        return false;
    };

    reset_local_ice(&local_ice);

    let offer = match pc
        .create_offer(Some(RTCOfferOptions {
            ice_restart: true,
            voice_activity_detection: false,
        }))
        .await
    {
        Ok(o) => o,
        Err(e) => {
            crate::diag_warn!(
                "webrtc",
                "ICE restart create_offer falhou channel={} peer={} pc_id={}: {}",
                channel,
                peer_id,
                pc_id,
                e
            );
            return false;
        }
    };

    if let Err(e) = pc.set_local_description(offer.clone()).await {
        crate::diag_warn!(
            "webrtc",
            "ICE restart set_local_description falhou channel={} peer={} pc_id={}: {}",
            channel,
            peer_id,
            pc_id,
            e
        );
        return false;
    }

    let Ok(sdp_val) = serde_json::to_value(&offer) else {
        return false;
    };

    let _ = sig_tx.send(SignalingCommand::SendSignal {
        to: peer_id.to_string(),
        data: SignalData {
            channel: Some(channel.to_string()),
            role: Some("broadcaster".to_string()),
            kind: Some("offer".to_string()),
            sdp: Some(sdp_val),
            ice_restart: Some(true),
            ..Default::default()
        },
    });
    flush_local_ice(
        &local_ice,
        &offer.sdp,
        sig_tx,
        peer_id,
        channel,
        "broadcaster",
    );

    if channel != "mic" {
        let st = state.lock().await;
        st.request_channel_keyframe(channel, 0);
    }

    crate::diag_info!(
        "webrtc",
        "ICE restart in-place enviado (preserva tela cheia no site) channel={} peer={} pc_id={}",
        channel,
        peer_id,
        pc_id
    );
    true
}

fn flush_local_ice(
    local_ice: &Arc<std::sync::Mutex<PendingLocalIce>>,
    sdp: &str,
    sig_tx: &mpsc::UnboundedSender<SignalingCommand>,
    to_peer: &str,
    channel: &str,
    role: &str,
) {
    let (mid, ufrag) = extract_sdp_mid_and_ufrag(sdp);
    let pending = {
        let mut guard = local_ice.lock().unwrap_or_else(|e| e.into_inner());
        guard.mid = mid.clone();
        guard.ufrag = ufrag.clone();
        guard.ready = true;
        std::mem::take(&mut guard.queued)
    };
    for cand_str in pending {
        let val = format_ice_candidate_json(&cand_str, &mid, ufrag.as_deref());
        let _ = sig_tx.send(SignalingCommand::SendSignal {
            to: to_peer.to_string(),
            data: SignalData {
                channel: Some(channel.to_string()),
                role: Some(role.to_string()),
                kind: Some("ice".to_string()),
                candidate: Some(val),
                ..Default::default()
            },
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_signal(
    state: &Arc<Mutex<WebRtcState>>,
    sig_tx: &mpsc::UnboundedSender<SignalingCommand>,
    self_cmd_tx: &mpsc::UnboundedSender<WebRtcCommand>,
    mixer: &Arc<AudioMixer>,
    evt_tx: &mpsc::UnboundedSender<WebRtcEvent>,
    egui_ctx: &egui::Context,
    from: &str,
    data: SignalData,
) {
    let kind = data.kind.as_deref().unwrap_or("");
    if kind == "peer-left" {
        let (s, r) = {
            let mut st = state.lock().await;
            st.known_peers.remove(from);
            st.viewer_stopped_peers.retain(|(_, p)| p != from);
            st.send_retry_attempts.retain(|(_, p), _| p != from);
            st.diag_responders.retain(|(_, p), _| p != from);
            st.prev_send_samples.retain(|(_, p, _), _| p != from);
            st.prev_diag_send_samples.retain(|(_, p, _), _| p != from);
            let mut s_pcs = Vec::new();
            let mut r_pcs = Vec::new();
            let s_keys: Vec<_> = st
                .send_pcs
                .keys()
                .filter(|(_, p)| p == from)
                .cloned()
                .collect();
            for k in s_keys {
                if let Some(e) = st.send_pcs.remove(&k) {
                    s_pcs.push(e.pc);
                }
                st.pending_send_ice.remove(&k);
            }
            st.refresh_media_fanout();
            let r_keys: Vec<_> = st
                .recv_pcs
                .keys()
                .filter(|(_, p)| p == from)
                .cloned()
                .collect();
            for k in r_keys {
                st.remote_video_mailbox.remove(&k.0, &k.1);
                if let Some(e) = st.recv_pcs.remove(&k) {
                    r_pcs.push(e.pc);
                }
                st.pending_recv_ice.remove(&k);
            }
            st.policy_map.remove_peer(from);
            (s_pcs, r_pcs)
        };
        for pc in s {
            let _ = pc.close().await;
        }
        for pc in r {
            let _ = pc.close().await;
        }
        return;
    }

    let Some(channel) = data.channel.clone() else {
        return;
    };
    let role = data.role.as_deref().unwrap_or("");

    if role == "broadcaster" {
        match kind {
            "offer" => {
                let Some(sdp_val) = data.sdp else { return };
                let Ok(offer) = serde_json::from_value::<RTCSessionDescription>(sdp_val) else {
                    return;
                };
                let key = (channel.clone(), from.to_string());

                if data.ice_restart != Some(true) && state.lock().await.stopped_by_us.contains(&key) {
                    return;
                }

                let existing_reusable = if data.ice_restart == Some(true) {
                    let mut st = state.lock().await;
                    if let Some(entry) = st.recv_pcs.get_mut(&key) {
                        if entry.pc.connection_state() != RTCPeerConnectionState::Closed {
                            entry.remote_desc_set = false;
                            {
                                let mut g = entry
                                    .local_ice
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                g.ready = false;
                            }
                            Some((entry.pc.clone(), entry.local_ice.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                let (pc, local_ice) = if let Some(pair) = existing_reusable {
                    pair
                } else {
                    let (api, ice_servers, old_pc, remote_video_mb_trk, policy_map_trk) = {
                        let mut st = state.lock().await;
                        let old = st.recv_pcs.remove(&key);
                        st.pending_recv_ice.remove(&key);
                        (
                            st.api.clone(),
                            st.ice_servers.clone(),
                            old.map(|o| o.pc),
                            st.remote_video_mailbox.clone(),
                            st.policy_map.clone(),
                        )
                    };
                    if let Some(pc) = old_pc {
                        let _ = pc.close().await;
                    }

                    let Some(pc) = create_peer_connection(&api, ice_servers).await else {
                        return;
                    };

                    let local_ice = Arc::new(std::sync::Mutex::new(PendingLocalIce::new()));
                    let local_ice_cb = local_ice.clone();
                    let sig_ice = sig_tx.clone();
                    let ch_ice = channel.clone();
                    let peer_ice = from.to_string();
                    pc.on_ice_candidate(Box::new(move |cand| {
                        let local_ice_cb = local_ice_cb.clone();
                        let sig_ice = sig_ice.clone();
                        let ch_ice = ch_ice.clone();
                        let peer_ice = peer_ice.clone();
                        Box::pin(async move {
                            if let Some(c) = cand {
                                if let Ok(json_cand) = c.to_json() {
                                    if !json_cand.candidate.is_empty() {
                                        let immediate_payload = {
                                            let mut guard = local_ice_cb
                                                .lock()
                                                .unwrap_or_else(|e| e.into_inner());
                                            if guard.ready {
                                                Some(format_ice_candidate_json(
                                                    &json_cand.candidate,
                                                    &guard.mid,
                                                    guard.ufrag.as_deref(),
                                                ))
                                            } else {
                                                guard.queued.push(json_cand.candidate);
                                                None
                                            }
                                        };
                                        if let Some(val) = immediate_payload {
                                            let _ = sig_ice.send(SignalingCommand::SendSignal {
                                                to: peer_ice,
                                                data: SignalData {
                                                    channel: Some(ch_ice),
                                                    role: Some("viewer".to_string()),
                                                    kind: Some("ice".to_string()),
                                                    candidate: Some(val),
                                                    ..Default::default()
                                                },
                                            });
                                        }
                                    }
                                }
                            }
                        })
                    }));

                    let unconnected_since =
                        Arc::new(std::sync::Mutex::new(Some(Instant::now())));
                    {
                        let unconn_ice = unconnected_since.clone();
                        pc.on_ice_connection_state_change(Box::new(move |s: RTCIceConnectionState| {
                            let unconn_ice = unconn_ice.clone();
                            Box::pin(async move {
                                let mut g = unconn_ice.lock().unwrap_or_else(|e| e.into_inner());
                                if ice_is_up(s) {
                                    *g = None;
                                } else if g.is_none() {
                                    *g = Some(Instant::now());
                                }
                            })
                        }));
                    }

                    let recv_counters = Arc::new(RecvCounters::default());
                    let ch_trk = channel.clone();
                    let peer_trk = from.to_string();
                    let mixer_trk = mixer.clone();
                    let sig_trk = sig_tx.clone();
                    let ctx_trk = egui_ctx.clone();
                    let pc_weak = Arc::downgrade(&pc);
                    let cnt_trk = recv_counters.clone();

                    pc.on_track(Box::new(move |track, _receiver, _transceiver| {
                        let ch_trk = ch_trk.clone();
                        let peer_trk = peer_trk.clone();
                        let mixer_trk = mixer_trk.clone();
                        let sig_trk = sig_trk.clone();
                        let remote_video_mb_dec = remote_video_mb_trk.clone();
                        let policy_map_dec = policy_map_trk.clone();
                        let ctx_trk = ctx_trk.clone();
                        let pc_weak = pc_weak.clone();
                        let cnt_trk = cnt_trk.clone();

                        Box::pin(async move {
                            match track.kind() {
                                RTPCodecType::Audio => {
                                    let source_key = if ch_trk == "mic" {
                                        peer_trk.clone()
                                    } else {
                                        format!("{}:{}", ch_trk, peer_trk)
                                    };
                                    tokio::spawn(async move {
                                        let Ok(mut opus_dec) = OpusAudioDecoder::new() else {
                                            return;
                                        };
                                        let mut mono_pcm = Vec::with_capacity(2880);
                                        let mut last_repaint = Instant::now();
                                        while let Ok((rtp_pkt, _)) = track.read_rtp().await {
                                            if opus_dec.decode_packet(&rtp_pkt.payload, &mut mono_pcm) {
                                                mixer_trk.push_samples(
                                                    &source_key,
                                                    &mono_pcm,
                                                    now_epoch_ms(),
                                                );
                                                if last_repaint.elapsed() >= Duration::from_millis(100) {
                                                    ctx_trk.request_repaint();
                                                    last_repaint = Instant::now();
                                                }
                                            }
                                        }
                                    });
                                }
                                RTPCodecType::Video => {
                                    let (au_tx, au_rx) =
                                        std::sync::mpsc::sync_channel::<Bytes>(16);
                                    let ch_dec = ch_trk.clone();
                                    let peer_dec = peer_trk.clone();
                                    let ctx_dec = ctx_trk.clone();
                                    let needs_pli = Arc::new(AtomicBool::new(true));
                                    let needs_pli_dec = needs_pli.clone();
                                    let cnt_dec = cnt_trk.clone();
                                    let policy_flag =
                                        policy_map_dec.get_or_create(&ch_dec, &peer_dec);
                                    let policy_rtp = policy_flag.clone();
                                    let ch_rtp = ch_trk.clone();
                                    let peer_rtp = peer_trk.clone();
                                    let sig_rtp = sig_trk;

                                    // Thread dedicada por track remoto de video: pula decode em Modo Eco (HIDDEN)
                                    // e converte direto para Color32 escalado sem alocacoes extras.
                                    let _ = std::thread::Builder::new()
                                        .name(format!("golive-h264-{}-{}", ch_dec, peer_dec))
                                        .spawn(move || {
                                            let Ok(mut decoder) = H264VideoDecoder::new() else {
                                                return;
                                            };
                                            let mut last_frame_at: Option<Instant> = None;
                                            let mut last_ui_push = Instant::now() - Duration::from_secs(1);
                                            let mut avg_frame_ms = 0.0_f64;
                                            let mut decoded_local = 0u64;
                                            let mut was_hidden =
                                                policy_flag.load(Ordering::Relaxed) == VIEW_POLICY_HIDDEN;

                                            while let Ok(au) = au_rx.recv() {
                                                let policy = policy_flag.load(Ordering::Relaxed);
                                                if policy == VIEW_POLICY_HIDDEN {
                                                    if !was_hidden {
                                                        was_hidden = true;
                                                        needs_pli_dec.store(true, Ordering::Release);
                                                    }
                                                    continue;
                                                }
                                                let force_push = if was_hidden {
                                                    was_hidden = false;
                                                    needs_pli_dec.store(true, Ordering::Release);
                                                    true
                                                } else {
                                                    false
                                                };

                                                let (max_w, max_h, min_ui_ms) = match policy {
                                                    VIEW_POLICY_FOCUSED => (3840usize, 2160usize, 8u64),
                                                    VIEW_POLICY_GRID_LARGE => (2560usize, 1440usize, 16u64),
                                                    _ => (1920usize, 1080usize, 16u64),
                                                };

                                                if !force_push
                                                    && last_ui_push.elapsed()
                                                        < Duration::from_millis(min_ui_ms)
                                                {
                                                    if decoder.decode_annex_b_reference_only(&au).is_ok() {
                                                        cnt_dec.frames_decoded.fetch_add(1, Ordering::Relaxed);
                                                        last_frame_at = Some(Instant::now());
                                                    } else {
                                                        cnt_dec.frames_dropped.fetch_add(1, Ordering::Relaxed);
                                                        needs_pli_dec.store(true, Ordering::Release);
                                                    }
                                                    continue;
                                                }

                                                let mut px_buf = remote_video_mb_dec.take_recycled_buf();
                                                match decoder.decode_annex_b_to_color32(
                                                    &au,
                                                    max_w,
                                                    max_h,
                                                    &mut px_buf,
                                                ) {
                                                    Ok(Some((orig_w, orig_h, dst_w, dst_h))) => {
                                                        needs_pli_dec.store(false, Ordering::Release);
                                                        cnt_dec.width.store(orig_w as u32, Ordering::Relaxed);
                                                        cnt_dec.height.store(orig_h as u32, Ordering::Relaxed);
                                                        cnt_dec.frames_decoded.fetch_add(1, Ordering::Relaxed);
                                                        decoded_local += 1;

                                                        let now = Instant::now();
                                                        if let Some(prev_at) = last_frame_at {
                                                            let gap_ms = now.duration_since(prev_at).as_secs_f64() * 1000.0;
                                                            if decoded_local >= 5 {
                                                                let freeze_thresh =
                                                                    (3.0 * avg_frame_ms).max(avg_frame_ms + 150.0).max(250.0);
                                                                if gap_ms >= freeze_thresh {
                                                                    cnt_dec.freeze_count.fetch_add(1, Ordering::Relaxed);
                                                                    cnt_dec.freeze_duration_ms.fetch_add(
                                                                        gap_ms.round() as u64,
                                                                        Ordering::Relaxed,
                                                                    );
                                                                }
                                                            }
                                                            if gap_ms > 0.0 && gap_ms < 2000.0 {
                                                                avg_frame_ms = if avg_frame_ms <= 0.0 {
                                                                    gap_ms
                                                                } else {
                                                                    avg_frame_ms * 0.85 + gap_ms * 0.15
                                                                };
                                                            }
                                                        }
                                                        last_frame_at = Some(now);

                                                        if force_push
                                                            || last_ui_push.elapsed()
                                                                >= Duration::from_millis(min_ui_ms)
                                                        {
                                                            last_ui_push = now;
                                                            remote_video_mb_dec.push(
                                                                DecodedRemoteVideo {
                                                                    channel: ch_dec.clone(),
                                                                    peer_id: peer_dec.clone(),
                                                                    width: dst_w,
                                                                    height: dst_h,
                                                                    orig_width: orig_w,
                                                                    orig_height: orig_h,
                                                                    pixels: px_buf,
                                                                },
                                                            );
                                                            ctx_dec.request_repaint();
                                                        } else {
                                                            remote_video_mb_dec.recycle_buf(px_buf);
                                                        }
                                                    }
                                                    Ok(None) => {
                                                        remote_video_mb_dec.recycle_buf(px_buf);
                                                    }
                                                    Err(_) => {
                                                        remote_video_mb_dec.recycle_buf(px_buf);
                                                        cnt_dec.frames_dropped.fetch_add(1, Ordering::Relaxed);
                                                        needs_pli_dec.store(true, Ordering::Release);
                                                    }
                                                }
                                            }
                                        });

                                    let cnt_rtp = cnt_trk;
                                    tokio::spawn(async move {
                                        let mut assembler = H264RtpAssembler::new();
                                        let mut media_ssrc = track.ssrc();
                                        if media_ssrc != 0 {
                                            if let Some(pc_ref) = pc_weak.upgrade() {
                                                let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                                                    Box::new(PictureLossIndication {
                                                        sender_ssrc: 0,
                                                        media_ssrc,
                                                    });
                                                let _ = pc_ref.write_rtcp(&[pli]).await;
                                            }
                                        }

                                        let mut last_sent_policy = policy_rtp.load(Ordering::Relaxed);
                                        let mut pending_resume_resend: Option<Instant> = None;
                                        let mut last_quality_sent = Instant::now();
                                        let track_started_at = Instant::now();
                                        let mut initial_hidden_notified = false;
                                        // So envia sinal inicial imediato se a politica ja estiver ativa;
                                        // caso ainda esteja HIDDEN no primeiro tick da track, aguarda a UI
                                        // rodar `sync_video_render_policies` (~400ms) para nao travar em 576p15.
                                        if last_sent_policy != VIEW_POLICY_HIDDEN {
                                            initial_hidden_notified = true;
                                            let (tier, idle) = quality_tier_for_policy(last_sent_policy);
                                            let _ = sig_rtp.send(SignalingCommand::SendSignal {
                                                to: peer_rtp.clone(),
                                                data: SignalData {
                                                    channel: Some(ch_rtp.clone()),
                                                    role: Some("viewer".to_string()),
                                                    kind: Some("quality".to_string()),
                                                    tier: Some(tier.to_string()),
                                                    idle: if idle { Some(true) } else { None },
                                                    ..Default::default()
                                                },
                                            });
                                        }

                                        let mut ctrl_tick =
                                            tokio::time::interval(Duration::from_millis(120));
                                        ctrl_tick.set_missed_tick_behavior(
                                            tokio::time::MissedTickBehavior::Delay,
                                        );
                                        let mut last_pli_at = Instant::now() - Duration::from_secs(2);
                                        let mut last_seq: Option<u16> = None;

                                        loop {
                                            tokio::select! {
                                                res = track.read_rtp() => {
                                                    let Ok((rtp_pkt, _)) = res else {
                                                        break;
                                                    };
                                                    cnt_rtp.bytes_received.fetch_add(
                                                        rtp_pkt.payload.len() as u64 + 12,
                                                        Ordering::Relaxed,
                                                    );
                                                    cnt_rtp.packets_received.fetch_add(1, Ordering::Relaxed);
                                                    if let Some(prev) = last_seq {
                                                        let diff = rtp_pkt.header.sequence_number.wrapping_sub(prev);
                                                        if diff > 1 && diff < 500 {
                                                            cnt_rtp.packets_lost.fetch_add(
                                                                (diff - 1) as u64,
                                                                Ordering::Relaxed,
                                                            );
                                                        }
                                                    }
                                                    last_seq = Some(rtp_pkt.header.sequence_number);

                                                    if media_ssrc == 0 && rtp_pkt.header.ssrc != 0 {
                                                        media_ssrc = rtp_pkt.header.ssrc;
                                                        if let Some(pc_ref) = pc_weak.upgrade() {
                                                            let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                                                                Box::new(PictureLossIndication {
                                                                    sender_ssrc: 0,
                                                                    media_ssrc,
                                                                });
                                                            let _ = pc_ref.write_rtcp(&[pli]).await;
                                                            last_pli_at = Instant::now();
                                                        }
                                                    }
                                                    let (maybe_au, saw_gap) = assembler.push_packet(&rtp_pkt);
                                                    if saw_gap {
                                                        needs_pli.store(true, Ordering::Release);
                                                    }
                                                    if let Some(au) = maybe_au {
                                                        cnt_rtp.frames_received.fetch_add(1, Ordering::Relaxed);
                                                        if au_tx.try_send(au).is_err() {
                                                            cnt_rtp.frames_dropped.fetch_add(1, Ordering::Relaxed);
                                                            needs_pli.store(true, Ordering::Release);
                                                        }
                                                    }
                                                    if needs_pli.load(Ordering::Acquire)
                                                        && media_ssrc != 0
                                                        && last_pli_at.elapsed() >= Duration::from_secs(1)
                                                    {
                                                        if let Some(pc_ref) = pc_weak.upgrade() {
                                                            let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                                                                Box::new(PictureLossIndication {
                                                                    sender_ssrc: 0,
                                                                    media_ssrc,
                                                                });
                                                            let _ = pc_ref.write_rtcp(&[pli]).await;
                                                            last_pli_at = Instant::now();
                                                        }
                                                    }
                                                }
                                                _ = ctrl_tick.tick() => {
                                                    let cur_policy = policy_rtp.load(Ordering::Relaxed);
                                                    let policy_changed = cur_policy != last_sent_policy;
                                                    let due_initial_hidden = !initial_hidden_notified
                                                        && cur_policy == VIEW_POLICY_HIDDEN
                                                        && track_started_at.elapsed()
                                                            >= Duration::from_millis(400);
                                                    let due_resume = pending_resume_resend
                                                        .is_some_and(|t| Instant::now() >= t);
                                                    let due_refresh =
                                                        last_quality_sent.elapsed() >= Duration::from_secs(20);

                                                    if policy_changed
                                                        || due_initial_hidden
                                                        || due_resume
                                                        || due_refresh
                                                    {
                                                        initial_hidden_notified = true;
                                                        let resuming = policy_changed
                                                            && last_sent_policy == VIEW_POLICY_HIDDEN
                                                            && cur_policy != VIEW_POLICY_HIDDEN;
                                                        last_sent_policy = cur_policy;
                                                        last_quality_sent = Instant::now();
                                                        if resuming {
                                                            needs_pli.store(true, Ordering::Release);
                                                            pending_resume_resend =
                                                                Some(Instant::now() + Duration::from_millis(1500));
                                                        } else if due_resume || cur_policy == VIEW_POLICY_HIDDEN {
                                                            pending_resume_resend = None;
                                                        }
                                                        let (tier, idle) = quality_tier_for_policy(cur_policy);
                                                        let _ = sig_rtp.send(SignalingCommand::SendSignal {
                                                            to: peer_rtp.clone(),
                                                            data: SignalData {
                                                                channel: Some(ch_rtp.clone()),
                                                                role: Some("viewer".to_string()),
                                                                kind: Some("quality".to_string()),
                                                                tier: Some(tier.to_string()),
                                                                idle: if idle { Some(true) } else { None },
                                                                ..Default::default()
                                                            },
                                                        });
                                                    }

                                                    if needs_pli.load(Ordering::Acquire)
                                                        && media_ssrc != 0
                                                        && last_pli_at.elapsed() >= Duration::from_secs(1)
                                                    {
                                                        if let Some(pc_ref) = pc_weak.upgrade() {
                                                            let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                                                                Box::new(PictureLossIndication {
                                                                    sender_ssrc: 0,
                                                                    media_ssrc,
                                                                });
                                                            let _ = pc_ref.write_rtcp(&[pli]).await;
                                                            last_pli_at = Instant::now();
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                                _ => {}
                            }
                        })
                    }));

                    {
                        let mut st = state.lock().await;
                        st.recv_pcs.insert(
                            key.clone(),
                            RecvPeerEntry {
                                pc: pc.clone(),
                                remote_desc_set: false,
                                local_ice: local_ice.clone(),
                                recv_counters,
                                unconnected_since,
                            },
                        );
                    }
                    (pc, local_ice)
                };

                if pc.set_remote_description(offer).await.is_ok() {
                    let queued = {
                        let mut st = state.lock().await;
                        if let Some(entry) = st.recv_pcs.get_mut(&key) {
                            entry.remote_desc_set = true;
                        }
                        st.pending_recv_ice.remove(&key).unwrap_or_default()
                    };
                    for c in queued {
                        add_remote_ice_with_mdns_fallback(&pc, c).await;
                    }
                    if let Ok(answer) = pc.create_answer(None).await {
                        if pc.set_local_description(answer.clone()).await.is_ok() {
                            if let Ok(sdp_val) = serde_json::to_value(&answer) {
                                let _ = sig_tx.send(SignalingCommand::SendSignal {
                                    to: from.to_string(),
                                    data: SignalData {
                                        channel: Some(channel.clone()),
                                        role: Some("viewer".to_string()),
                                        kind: Some("answer".to_string()),
                                        sdp: Some(sdp_val),
                                        ..Default::default()
                                    },
                                });
                            }
                            flush_local_ice(
                                &local_ice,
                                &answer.sdp,
                                sig_tx,
                                from,
                                &channel,
                                "viewer",
                            );
                        }
                    }
                }
            }
            "ice" => {
                if let Some(cand_val) = data.candidate {
                    if let Ok(cand) = serde_json::from_value::<RTCIceCandidateInit>(cand_val) {
                        let key = (channel.clone(), from.to_string());
                        let pc_opt = {
                            let mut st = state.lock().await;
                            if let Some(entry) = st.recv_pcs.get(&key) {
                                if entry.remote_desc_set {
                                    Some(entry.pc.clone())
                                } else {
                                    st.pending_recv_ice.entry(key).or_default().push(cand.clone());
                                    None
                                }
                            } else {
                                st.pending_recv_ice.entry(key).or_default().push(cand.clone());
                                None
                            }
                        };
                        if let Some(pc) = pc_opt {
                            add_remote_ice_with_mdns_fallback(&pc, cand).await;
                        }
                    }
                }
            }
            "stop" => {
                let old_pc = {
                    let mut st = state.lock().await;
                    let key = (channel.clone(), from.to_string());
                    st.stopped_by_us.remove(&key);
                    st.pending_recv_ice.remove(&key);
                    st.remote_video_mailbox.remove(&channel, from);
                    if st.watched_stats.as_ref()
                        == Some(&StatsTarget::Remote {
                            channel: channel.clone(),
                            peer_id: from.to_string(),
                        })
                    {
                        st.latest_remote_diag = None;
                    }
                    st.recv_pcs.remove(&key).map(|e| e.pc)
                };
                if let Some(pc) = old_pc {
                    let _ = pc.close().await;
                }
                let _ = evt_tx.send(WebRtcEvent::PeerStoppedSharing {
                    channel,
                    peer_id: from.to_string(),
                });
                egui_ctx.request_repaint();
            }
            "diag" => {
                if channel == "mic" {
                    return;
                }
                if let Some(diag_val) = data.diag {
                    if let Some(parsed) = parse_remote_diag(&diag_val, now_epoch_ms()) {
                        let expected_origin = data.origin_id.as_deref().unwrap_or(from);
                        let patched_snapshot = {
                            let mut st = state.lock().await;
                            let matches_watch = st.watched_stats.as_ref()
                                == Some(&StatsTarget::Remote {
                                    channel: channel.clone(),
                                    peer_id: from.to_string(),
                                });
                            if matches_watch && expected_origin == from {
                                st.latest_remote_diag =
                                    Some(((channel.clone(), from.to_string()), parsed.clone()));
                                if let Some(ref mut snap) = st.last_remote_snapshot {
                                    if snap.channel == channel && snap.peer_id == from {
                                        snap.route = merge_viewer_route(
                                            snap.route.clone(),
                                            parsed.route.as_ref(),
                                        );
                                        snap.remote_diag = Some(parsed);
                                        snap.sender_silent = false;
                                        snap.causes = diagnose_connection(
                                            Some(&snap.route),
                                            snap.remote_diag.as_ref().and_then(|d| d.send.as_ref()),
                                            snap.recv.as_ref(),
                                        );
                                        Some(snap.clone())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        };
                        if let Some(snap) = patched_snapshot {
                            let _ = evt_tx.send(WebRtcEvent::StatsUpdated(TileStatsSnapshot::Remote(snap)));
                            egui_ctx.request_repaint();
                        }
                    }
                }
            }
            _ => {}
        }
    } else if role == "viewer" {
        match kind {
            "answer" => {
                if let Some(sdp_val) = data.sdp {
                    if let Ok(ans) = serde_json::from_value::<RTCSessionDescription>(sdp_val) {
                        let key = (channel.clone(), from.to_string());
                        let pc_opt = {
                            let st = state.lock().await;
                            st.send_pcs.get(&key).map(|e| (e.pc_id, e.pc.clone()))
                        };
                        if let Some((expected_pc_id, pc)) = pc_opt {
                            if pc.set_remote_description(ans).await.is_ok() {
                                let queued = {
                                    let mut st = state.lock().await;
                                    let mut already_connected = false;
                                    if let Some(e) = st.send_pcs.get_mut(&key) {
                                        e.remote_desc_set = true;
                                        already_connected = e.connected;
                                    }
                                    st.refresh_media_fanout();
                                    if already_connected && channel != "mic" {
                                        st.request_channel_keyframe(&channel, 0);
                                    }
                                    st.pending_send_ice.remove(&key).unwrap_or_default()
                                };
                                for c in queued {
                                    add_remote_ice_with_mdns_fallback(&pc, c).await;
                                }
                            } else {
                                let _ = self_cmd_tx.send(WebRtcCommand::RetrySendPc {
                                    channel: channel.clone(),
                                    peer_id: from.to_string(),
                                    expected_pc_id,
                                });
                            }
                        }
                    }
                }
            }
            "ice" => {
                if let Some(cand_val) = data.candidate {
                    if let Ok(cand) = serde_json::from_value::<RTCIceCandidateInit>(cand_val) {
                        let key = (channel.clone(), from.to_string());
                        let pc_opt = {
                            let mut st = state.lock().await;
                            if let Some(e) = st.send_pcs.get(&key) {
                                if e.remote_desc_set {
                                    Some(e.pc.clone())
                                } else {
                                    st.pending_send_ice.entry(key).or_default().push(cand.clone());
                                    None
                                }
                            } else {
                                st.pending_send_ice.entry(key).or_default().push(cand.clone());
                                None
                            }
                        };
                        if let Some(pc) = pc_opt {
                            add_remote_ice_with_mdns_fallback(&pc, cand).await;
                        }
                    }
                }
            }
            "stop" => {
                let mut st = state.lock().await;
                let key = (channel, from.to_string());
                st.viewer_stopped_peers.insert(key.clone());
                if let Some(entry) = st.send_pcs.get_mut(&key) {
                    entry.paused_by_viewer = true;
                }
                st.refresh_media_fanout();
            }
            "resume" => {
                let should_reopen = {
                    let mut st = state.lock().await;
                    let key = (channel.clone(), from.to_string());
                    st.viewer_stopped_peers.remove(&key);
                    st.send_retry_attempts.remove(&key);
                    if let Some(entry) = st.send_pcs.get_mut(&key) {
                        entry.paused_by_viewer = false;
                        if let Some(ref nk) = entry.peer_need_key {
                            nk.store(true, Ordering::Release);
                        }
                        st.refresh_media_fanout();
                        st.request_channel_keyframe(&channel, 0);
                        false
                    } else {
                        st.active_send_channels.contains(&channel)
                    }
                };
                if should_reopen {
                    open_send_pc(state, sig_tx, self_cmd_tx, &channel, from).await;
                }
            }
            "reconnect-request" => {
                crate::diag_warn!(
                    "webrtc",
                    "recebeu reconnect-request do viewer channel={} peer={}",
                    channel,
                    from
                );
                if try_ice_restart_send_pc(state, sig_tx, &channel, from).await {
                    return;
                }
                crate::diag_warn!(
                    "webrtc",
                    "ICE restart indisponivel; recriando send_pc channel={} peer={}",
                    channel,
                    from
                );
                let (old_pc, active) = {
                    let mut st = state.lock().await;
                    let key = (channel.clone(), from.to_string());
                    st.viewer_stopped_peers.remove(&key);
                    st.send_retry_attempts.remove(&key);
                    st.pending_send_ice.remove(&key);
                    let removed = st.send_pcs.remove(&key).map(|e| e.pc);
                    st.refresh_media_fanout();
                    (removed, st.active_send_channels.contains(&channel))
                };
                if let Some(pc) = old_pc {
                    let _ = pc.close().await;
                }
                if active {
                    open_send_pc(state, sig_tx, self_cmd_tx, &channel, from).await;
                }
            }
            "quality" => {
                if channel == "mic" {
                    return;
                }
                // O site envia `{ kind: "quality", tier: "360p15", idle: true }` quando a aba fica oculta,
                // e OMITE `idle` (`None`) quando a aba volta a ficar visivel! Tratar `None` como `false`.
                let next_idle = data.idle.unwrap_or(false);
                let mut st = state.lock().await;
                let key = (channel.clone(), from.to_string());
                let mut trigger_kf = false;
                if let Some(entry) = st.send_pcs.get_mut(&key) {
                    if entry.paused_by_viewer && !next_idle {
                        trigger_kf = true;
                        if let Some(ref nk) = entry.peer_need_key {
                            nk.store(true, Ordering::Release);
                        }
                    }
                    entry.paused_by_viewer = next_idle;
                    st.refresh_media_fanout();
                }
                if trigger_kf && channel != "mic" {
                    st.request_channel_keyframe(&channel, 0);
                }
            }
            "diag-request" => {
                if channel == "mic" {
                    return;
                }
                let reply_payload = {
                    let mut st = state.lock().await;
                    let key = (channel.clone(), from.to_string());
                    if !st.send_pcs.contains_key(&key) {
                        return;
                    }
                    let origin = data
                        .origin_id
                        .clone()
                        .or_else(|| st.self_id.clone())
                        .unwrap_or_default();
                    let now = Instant::now();
                    if let Some(existing) = st.diag_responders.get_mut(&key) {
                        existing.origin_id = origin;
                        existing.expires_at = now + Duration::from_secs(DIAG_LEASE_SECS);
                        None
                    } else {
                        let active_for_ch = st
                            .diag_responders
                            .keys()
                            .filter(|(c, _)| c == &channel)
                            .count();
                        if active_for_ch >= DIAG_MAX_RESPONDERS_PER_CHANNEL {
                            return;
                        }
                        st.diag_responders.insert(
                            key.clone(),
                            DiagResponderLease {
                                origin_id: origin.clone(),
                                expires_at: now + Duration::from_secs(DIAG_LEASE_SECS),
                            },
                        );
                        st.send_pcs.get(&key).map(|entry| {
                            (
                                entry.pc_id,
                                entry.pc.clone(),
                                entry.send_counters.clone(),
                                st.local_stream_meta.get(&channel).cloned().unwrap_or_default(),
                                origin,
                            )
                        })
                    }
                };
                if let Some((pc_id, pc, cnt, meta, origin)) = reply_payload {
                    send_diag_reply_for_peer(
                        state, sig_tx, &channel, from, &origin, pc_id, &pc, &cnt, &meta,
                    )
                    .await;
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_diag_reply_for_peer(
    state: &Arc<Mutex<WebRtcState>>,
    sig_tx: &mpsc::UnboundedSender<SignalingCommand>,
    channel: &str,
    peer_id: &str,
    origin_id: &str,
    pc_id: u64,
    pc: &Arc<RTCPeerConnection>,
    cnt: &Arc<SendCounters>,
    meta: &LocalStreamMeta,
) {
    let report = pc.get_stats().await;
    let (route, remote_loss, _) = read_route_and_remote_inbound(&report);
    let cur = cnt.snapshot();
    let prev = {
        let mut st = state.lock().await;
        let key = (channel.to_string(), peer_id.to_string(), pc_id);
        let old = st.prev_diag_send_samples.get(&key).copied();
        st.prev_diag_send_samples.insert(key, cur);
        old
    };
    let send = compute_send_metrics(meta, &cur, prev.as_ref(), remote_loss);
    let cores = std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u32);

    let diag = json!({
        "send": {
            "width": send.width,
            "height": send.height,
            "fps": send.fps,
            "kbps": send.kbps,
            "limitation": send.limitation,
            "cpuLimitedShare": send.cpu_limited_share,
            "bandwidthLimitedShare": send.bandwidth_limited_share,
            "encoder": send.encoder,
            "hardware": send.hardware,
            "codec": send.codec,
            "remoteLoss": send.remote_loss,
            "nackPerSecond": send.nack_per_second,
            "pliPerSecond": send.pli_per_second,
            "targetKbps": send.target_kbps,
        },
        "route": {
            "kind": route.kind,
            "localType": route.local_type,
            "remoteType": route.remote_type,
            "protocol": route.protocol,
            "relayProtocol": route.relay_protocol,
            "relayUrl": route.relay_url,
            "rttMs": route.rtt_ms,
            "availableOutgoingKbps": route.available_outgoing_kbps,
        },
        "cores": cores,
        "profile": "balanced",
        "height": if meta.height > 0 { Some(meta.height) } else { None },
        "frameRate": if meta.target_fps > 0 { Some(meta.target_fps) } else { None },
        "maxBitrateKbps": if meta.target_bitrate_kbps > 0 { Some(meta.target_bitrate_kbps) } else { None },
    });

    let _ = sig_tx.send(SignalingCommand::SendSignal {
        to: peer_id.to_string(),
        data: SignalData {
            channel: Some(channel.to_string()),
            role: Some("broadcaster".to_string()),
            kind: Some("diag".to_string()),
            origin_id: if origin_id.is_empty() {
                None
            } else {
                Some(origin_id.to_string())
            },
            diag: Some(diag),
            ..Default::default()
        },
    });
}

async fn emit_watched_stats(
    state: &Arc<Mutex<WebRtcState>>,
    evt_tx: &mpsc::UnboundedSender<WebRtcEvent>,
    egui_ctx: &egui::Context,
) {
    let target_opt = state.lock().await.watched_stats.clone();
    match target_opt {
        Some(StatsTarget::Remote { channel, peer_id }) => {
            let entry_info = {
                let st = state.lock().await;
                st.recv_pcs.get(&(channel.clone(), peer_id.clone())).map(|e| {
                    (
                        e.pc.clone(),
                        e.recv_counters.clone(),
                        e.unconnected_since
                            .lock()
                            .unwrap_or_else(|x| x.into_inner())
                            .map(|t| t.elapsed())
                            .unwrap_or_default(),
                    )
                })
            };

            let (route, recv, ice_state, ice_up, ice_failed, unconn_elapsed) =
                if let Some((pc, cnt, unconn)) = entry_info {
                    let report = pc.get_stats().await;
                    let (route, _, _) = read_route_and_remote_inbound(&report);
                    let cur = cnt.snapshot();
                    let prev = {
                        let mut st = state.lock().await;
                        let old = st
                            .prev_recv_sample
                            .as_ref()
                            .filter(|(k, _)| k.0 == channel && k.1 == peer_id)
                            .map(|(_, s)| *s);
                        st.prev_recv_sample =
                            Some(((channel.clone(), peer_id.clone()), cur));
                        old
                    };
                    let metrics = compute_recv_metrics(&cur, prev.as_ref());
                    let ic = pc.ice_connection_state();
                    (
                        route,
                        Some(metrics),
                        ic.to_string(),
                        ice_is_up(ic),
                        ic == RTCIceConnectionState::Failed,
                        unconn,
                    )
                } else {
                    (
                        RouteStats::default(),
                        None,
                        "new".to_string(),
                        false,
                        false,
                        Duration::ZERO,
                    )
                };

            let snapshot = {
                let mut st = state.lock().await;
                if st.watched_stats.as_ref()
                    != Some(&StatsTarget::Remote {
                        channel: channel.clone(),
                        peer_id: peer_id.clone(),
                    })
                {
                    return;
                }
                let now_ms = now_epoch_ms();
                let valid_diag = st
                    .latest_remote_diag
                    .as_ref()
                    .filter(|(k, d)| {
                        k.0 == channel
                            && k.1 == peer_id
                            && now_ms.saturating_sub(d.received_at_ms)
                                < (SENDER_SILENCE_SECS * 1000)
                    })
                    .map(|(_, d)| d.clone());

                let watch_elapsed = st
                    .watch_started_at
                    .map(|t| t.elapsed())
                    .unwrap_or_default();
                let sender_silent =
                    valid_diag.is_none() && watch_elapsed >= Duration::from_secs(SENDER_SILENCE_SECS);
                let merged_route =
                    merge_viewer_route(route, valid_diag.as_ref().and_then(|d| d.route.as_ref()));
                let not_connected = st.recv_pcs.contains_key(&(channel.clone(), peer_id.clone()))
                    && !ice_up
                    && (ice_failed
                        || (merged_route.kind == RouteKind::Unknown
                            && unconn_elapsed >= Duration::from_secs(NOT_CONNECTED_SECS)));

                let causes = if st.recv_pcs.contains_key(&(channel.clone(), peer_id.clone())) {
                    diagnose_connection(
                        Some(&merged_route),
                        valid_diag.as_ref().and_then(|d| d.send.as_ref()),
                        recv.as_ref(),
                    )
                } else {
                    Vec::new()
                };

                let snap = RemoteTileStatsSnapshot {
                    channel,
                    peer_id,
                    route: merged_route,
                    recv,
                    remote_diag: valid_diag,
                    sender_silent,
                    ice_state,
                    not_connected,
                    turn_configured: st.turn_configured,
                    causes,
                };
                st.last_remote_snapshot = Some(snap.clone());
                snap
            };

            let _ = evt_tx.send(WebRtcEvent::StatsUpdated(TileStatsSnapshot::Remote(snapshot)));
            egui_ctx.request_repaint();
        }
        Some(StatsTarget::Own { channel }) => {
            let (meta, turn_configured, entries) = {
                let st = state.lock().await;
                let meta = st.local_stream_meta.get(&channel).cloned().unwrap_or_default();
                let list: Vec<_> = st
                    .send_pcs
                    .iter()
                    .filter(|((ch, _), _)| ch == &channel)
                    .map(|((_, pid), e)| {
                        (
                            pid.clone(),
                            e.pc_id,
                            e.pc.clone(),
                            e.send_counters.clone(),
                            e.paused_by_viewer,
                            e.unconnected_since
                                .lock()
                                .unwrap_or_else(|x| x.into_inner())
                                .map(|t| t.elapsed())
                                .unwrap_or_default(),
                        )
                    })
                    .collect();
                (meta, st.turn_configured, list)
            };

            let mut viewers = Vec::with_capacity(entries.len());
            let mut total_kbps = 0u32;

            for (peer_id, pc_id, pc, cnt, paused, unconn) in entries {
                let report = pc.get_stats().await;
                let (route, remote_loss, _) = read_route_and_remote_inbound(&report);
                let cur = cnt.snapshot();
                let prev = {
                    let mut st = state.lock().await;
                    let key = (channel.clone(), peer_id.clone(), pc_id);
                    let old = st.prev_send_samples.get(&key).copied();
                    st.prev_send_samples.insert(key, cur);
                    old
                };
                let send_metrics = if paused {
                    None
                } else {
                    let m = compute_send_metrics(&meta, &cur, prev.as_ref(), remote_loss);
                    total_kbps = total_kbps.saturating_add(m.kbps);
                    Some(m)
                };
                let ic = pc.ice_connection_state();
                let not_connected = !ice_is_up(ic)
                    && (ic == RTCIceConnectionState::Failed
                        || unconn >= Duration::from_secs(NOT_CONNECTED_SECS));
                let worst_cause = if paused {
                    None
                } else {
                    diagnose_connection(Some(&route), send_metrics.as_ref(), None)
                        .into_iter()
                        .next()
                };

                viewers.push(OwnViewerStatsRow {
                    peer_id,
                    route,
                    send: send_metrics,
                    paused,
                    ice_state: ic.to_string(),
                    not_connected,
                    worst_cause,
                });
            }

            {
                let st = state.lock().await;
                if st.watched_stats.as_ref() != Some(&StatsTarget::Own { channel: channel.clone() }) {
                    return;
                }
            }

            let encoder_label = if meta.encoder_name.trim().is_empty() {
                "OpenH264".to_string()
            } else {
                meta.encoder_name.clone()
            };
            let snap = OwnStreamStatsSnapshot {
                channel,
                capture_width: meta.width,
                capture_height: meta.height,
                capture_fps: meta.target_fps,
                total_kbps,
                codec: "H264".to_string(),
                encoder: encoder_label,
                hardware: meta.hardware,
                cores: std::thread::available_parallelism()
                    .ok()
                    .map(|n| n.get() as u32),
                turn_configured,
                viewers,
            };
            let _ = evt_tx.send(WebRtcEvent::StatsUpdated(TileStatsSnapshot::Own(snap)));
            egui_ctx.request_repaint();
        }
        None => {}
    }
}

async fn run_stats_and_diag_tick(
    state: &Arc<Mutex<WebRtcState>>,
    sig_tx: &mpsc::UnboundedSender<SignalingCommand>,
    evt_tx: &mpsc::UnboundedSender<WebRtcEvent>,
    egui_ctx: &egui::Context,
) {
    // 1. Renova diag-request a cada 5s enquanto o painel remoto estiver aberto
    let diag_req = {
        let mut st = state.lock().await;
        let watched = st.watched_stats.clone();
        if let Some(StatsTarget::Remote { channel, peer_id }) = watched {
            let should_send = st
                .last_diag_request_at
                .map_or(true, |t| t.elapsed().as_millis() >= DIAG_REQUEST_INTERVAL_MS);
            if should_send {
                st.last_diag_request_at = Some(Instant::now());
                Some((channel, peer_id))
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some((ch, pid)) = diag_req {
        let _ = sig_tx.send(SignalingCommand::SendSignal {
            to: pid.clone(),
            data: SignalData {
                channel: Some(ch),
                role: Some("viewer".to_string()),
                kind: Some("diag-request".to_string()),
                origin_id: Some(pid),
                ..Default::default()
            },
        });
    }

    // 2. Atualiza painel de stats atualmente aberto
    emit_watched_stats(state, evt_tx, egui_ctx).await;

    // 3. Responde a espectadores com lease diag-request ativo (a cada 2s)
    let responder_jobs = {
        let mut st = state.lock().await;
        let now = Instant::now();
        let active_keys: HashSet<(String, String)> = st.send_pcs.keys().cloned().collect();
        st.diag_responders
            .retain(|k, lease| lease.expires_at > now && active_keys.contains(k));
        let responder_keys: HashSet<(String, String)> = st.diag_responders.keys().cloned().collect();
        st.prev_diag_send_samples
            .retain(|(ch, pid, _), _| responder_keys.contains(&(ch.clone(), pid.clone())));

        let mut jobs = Vec::new();
        for ((ch, pid), lease) in &st.diag_responders {
            if let Some(entry) = st.send_pcs.get(&(ch.clone(), pid.clone())) {
                let meta = st.local_stream_meta.get(ch).cloned().unwrap_or_default();
                jobs.push((
                    ch.clone(),
                    pid.clone(),
                    lease.origin_id.clone(),
                    entry.pc_id,
                    entry.pc.clone(),
                    entry.send_counters.clone(),
                    meta,
                ));
            }
        }
        jobs
    };

    for (ch, pid, origin, pc_id, pc, cnt, meta) in responder_jobs {
        send_diag_reply_for_peer(state, sig_tx, &ch, &pid, &origin, pc_id, &pc, &cnt, &meta).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_ice_servers_sets_password_and_filters_unsupported() {
        let input = vec![
            IceServerConfig {
                urls: vec![
                    "stun:stun.example.org:3478?transport=udp".to_string(),
                    "turn:turn.cloudflare.com:3478?transport=udp".to_string(),
                    "turn:turn.cloudflare.com:3478?transport=tcp".to_string(),
                    "turns:turn.cloudflare.com:5349?transport=tcp".to_string(),
                ],
                username: Some("cf-user".to_string()),
                credential: Some("cf-secret".to_string()),
            },
            IceServerConfig {
                urls: vec!["turn:no-creds.example.com:3478?transport=udp".to_string()],
                username: None,
                credential: None,
            },
        ];

        let (servers, has_turn) = sanitize_ice_servers(input);
        assert!(has_turn);
        assert!(servers
            .iter()
            .any(|s| s.urls.contains(&"stun:stun.l.google.com:19302".to_string())));
        assert!(servers
            .iter()
            .any(|s| s.urls.contains(&"stun:stun.example.org:3478".to_string())));

        let turn_entries: Vec<_> = servers
            .iter()
            .filter(|s| s.urls.iter().any(|u| u.starts_with("turn:")))
            .collect();
        assert_eq!(turn_entries.len(), 1);
        assert_eq!(
            turn_entries[0].urls,
            vec!["turn:turn.cloudflare.com:3478?transport=udp".to_string()]
        );
        assert_eq!(turn_entries[0].credential_type, RTCIceCredentialType::Password);
        assert_eq!(turn_entries[0].username, "cf-user");
        assert_eq!(turn_entries[0].credential, "cf-secret");

        // Nenhuma URL `turns:` ou `transport=tcp` pode permanecer
        for s in &servers {
            for u in &s.urls {
                assert!(!u.starts_with("turns:"));
                assert!(!u.contains("transport=tcp"));
            }
        }
    }

    #[test]
    fn test_format_ice_candidate_and_sdp_extract() {
        let sample_sdp = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:video0\r\na=ice-ufrag:AbCd1234\r\na=sendonly\r\n";
        let (mid, ufrag) = extract_sdp_mid_and_ufrag(sample_sdp);
        assert_eq!(mid, "video0");
        assert_eq!(ufrag.as_deref(), Some("AbCd1234"));

        let json_val = format_ice_candidate_json(
            "candidate:1 1 UDP 2122252543 192.168.1.10 51234 typ host",
            &mid,
            ufrag.as_deref(),
        );
        let obj = json_val.as_object().expect("json object");
        assert_eq!(obj.get("sdpMid").and_then(|v| v.as_str()), Some("video0"));
        assert_eq!(obj.get("sdpMLineIndex").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(
            obj.get("usernameFragment").and_then(|v| v.as_str()),
            Some("AbCd1234")
        );

        let no_ufrag = format_ice_candidate_json("candidate:1 1 UDP 2122252543 10.0.0.2 5000 typ host", "0", None);
        assert!(no_ufrag.as_object().unwrap().get("usernameFragment").is_none());
    }

    #[test]
    fn test_mdns_candidate_rewrite() {
        let mdns = "candidate:123 1 udp 2122260223 550e8400-e29b-41d4-a716-446655440000.local 54321 typ host generation 0";
        assert_eq!(
            extract_mdns_host_and_port(mdns),
            Some((
                "550e8400-e29b-41d4-a716-446655440000.local".to_string(),
                54321
            ))
        );
        let rewritten = rewrite_candidate_host(mdns, "192.168.15.7").expect("rewritten");
        assert_eq!(
            rewritten,
            "candidate:123 1 udp 2122260223 192.168.15.7 54321 typ host generation 0"
        );

        let ipv4 = "candidate:123 1 udp 2122260223 192.168.15.7 54321 typ host";
        assert_eq!(extract_mdns_host_and_port(ipv4), None);
    }

    #[test]
    fn test_diagnose_connection_verdicts() {
        let direct = RouteStats {
            kind: RouteKind::Direct,
            rtt_ms: Some(15),
            ..Default::default()
        };
        let clean_recv = RecvVideoMetrics {
            loss: 0.0,
            drop_share: 0.0,
            freezes: 0,
            ..Default::default()
        };
        assert!(diagnose_connection(Some(&direct), None, Some(&clean_recv)).is_empty());

        let turn_tcp = RouteStats {
            kind: RouteKind::RelayLocal,
            relay_protocol: Some("tcp".to_string()),
            rtt_ms: Some(280),
            ..Default::default()
        };
        let noisy_recv = RecvVideoMetrics {
            loss: 0.09,
            drop_share: 0.15,
            freezes: 2,
            ..Default::default()
        };
        let causes = diagnose_connection(Some(&turn_tcp), None, Some(&noisy_recv));
        assert!(causes.iter().any(|c| c.id == "turn-tcp" && c.severity == CauseSeverity::High));
        assert!(causes.iter().any(|c| c.id == "network-loss" && c.severity == CauseSeverity::High));
        assert!(causes.iter().any(|c| c.id == "receiver-drops" && c.severity == CauseSeverity::High));
        assert!(causes.iter().any(|c| c.id == "high-rtt" && c.severity == CauseSeverity::Medium));
        assert!(causes.iter().any(|c| c.id == "freezes" && c.severity == CauseSeverity::Medium));

        // Severidades High devem aparecer antes das Medium
        let first_medium = causes.iter().position(|c| c.severity == CauseSeverity::Medium);
        let last_high = causes.iter().rposition(|c| c.severity == CauseSeverity::High);
        if let (Some(fm), Some(lh)) = (first_medium, last_high) {
            assert!(lh < fm);
        }
    }

    #[tokio::test]
    async fn test_offer_answer_roundtrip_with_browser_sdp() {
        let api = build_webrtc_api();

        // 1. Video ("screen") com perfis H264 42e01f/42001f/4d001f, TWCC e sem rtcp-fb duplicado
        let sender_pc = api
            .new_peer_connection(RTCConfiguration {
                ice_servers: default_stun_servers(),
                ..Default::default()
            })
            .await
            .expect("sender video pc");

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-screen".to_string(),
        ));

        let init = Some(RTCRtpTransceiverInit {
            direction: RTCRtpTransceiverDirection::Sendonly,
            send_encodings: vec![],
        });
        sender_pc
            .add_transceiver_from_track(video_track as Arc<dyn TrackLocal + Send + Sync>, init)
            .await
            .expect("add video sendonly transceiver");

        let offer = sender_pc.create_offer(None).await.expect("create offer");
        assert!(offer.sdp.contains("a=sendonly"), "offer must specify a=sendonly");
        assert!(offer.sdp.contains("a=msid:"), "offer must carry a=msid:");
        assert!(
            offer.sdp.contains("profile-level-id=42e01f"),
            "offer must advertise Constrained Baseline 42e01f"
        );
        assert!(
            offer.sdp.contains("profile-level-id=42001f"),
            "offer must advertise Baseline 42001f fallback"
        );
        assert!(
            offer.sdp.contains("profile-level-id=4d001f"),
            "offer must advertise Main 4d001f fallback"
        );
        assert!(
            offer.sdp.contains("transport-cc"),
            "offer must negotiate TWCC (transport-cc)"
        );

        let mut seen_rtcp_fb = HashSet::new();
        for line in offer.sdp.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("a=rtcp-fb:") {
                assert!(
                    seen_rtcp_fb.insert(trimmed.to_string()),
                    "SDP nao deve duplicar linha rtcp-fb: {}",
                    trimmed
                );
            }
        }
        assert!(seen_rtcp_fb.iter().any(|l| l.ends_with(" nack")));
        assert!(seen_rtcp_fb.iter().any(|l| l.ends_with(" nack pli")));
        assert!(seen_rtcp_fb.iter().any(|l| l.ends_with(" transport-cc")));

        sender_pc
            .set_local_description(offer.clone())
            .await
            .expect("set local offer");

        let receiver_pc = api
            .new_peer_connection(RTCConfiguration {
                ice_servers: default_stun_servers(),
                ..Default::default()
            })
            .await
            .expect("receiver pc");
        receiver_pc
            .set_remote_description(offer)
            .await
            .expect("receiver set remote offer");
        let answer = receiver_pc.create_answer(None).await.expect("create answer");
        assert!(answer.sdp.contains("a=recvonly"), "answer must be a=recvonly");
        assert!(answer.sdp.contains("42e01f"));
        receiver_pc
            .set_local_description(answer.clone())
            .await
            .expect("receiver set local answer");
        sender_pc
            .set_remote_description(answer)
            .await
            .expect("sender accepts browser-style answer");

        // 2. Audio ("mic") Opus Sendonly
        let mic_sender = api
            .new_peer_connection(RTCConfiguration {
                ice_servers: default_stun_servers(),
                ..Default::default()
            })
            .await
            .expect("sender mic pc");
        let mic_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                ..Default::default()
            },
            "audio".to_string(),
            "golive-lite-mic".to_string(),
        ));
        mic_sender
            .add_transceiver_from_track(
                mic_track as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .expect("add mic sendonly transceiver");
        let mic_offer = mic_sender.create_offer(None).await.expect("mic offer");
        assert!(mic_offer.sdp.contains("opus/48000/2"));
        assert!(mic_offer.sdp.contains("a=sendonly"));

        mic_sender
            .set_local_description(mic_offer.clone())
            .await
            .expect("mic local offer");

        let mic_receiver = api
            .new_peer_connection(RTCConfiguration {
                ice_servers: default_stun_servers(),
                ..Default::default()
            })
            .await
            .expect("mic receiver");
        mic_receiver
            .set_remote_description(mic_offer)
            .await
            .expect("mic receiver set remote");
        let mic_answer = mic_receiver.create_answer(None).await.expect("mic answer");
        mic_sender
            .set_remote_description(mic_answer)
            .await
            .expect("mic sender set answer");

        let _ = sender_pc.close().await;
        let _ = receiver_pc.close().await;
        let _ = mic_sender.close().await;
        let _ = mic_receiver.close().await;
    }

    #[test]
    fn test_compute_monotonic_rtp_ts_90khz() {
        let mut base_pts = None;
        let mut last_ts = None;
        let base_ts = 0xFFFF_FFF0u32;

        let ts0 = compute_monotonic_rtp_ts_90khz(
            Duration::from_millis(0),
            &mut base_pts,
            base_ts,
            &mut last_ts,
        );
        assert_eq!(ts0, base_ts);

        // Mesmo PTS deve incrementar em >= 1 tick
        let ts_dup = compute_monotonic_rtp_ts_90khz(
            Duration::from_millis(0),
            &mut base_pts,
            base_ts,
            &mut last_ts,
        );
        assert_eq!(ts_dup, ts0.wrapping_add(1));

        // Frame 240 fps (~4166.667 us = 375 ticks em 90kHz -> cruza overflow u32)
        let ts_240 = compute_monotonic_rtp_ts_90khz(
            Duration::from_nanos(4_166_667),
            &mut base_pts,
            base_ts,
            &mut last_ts,
        );
        assert_eq!(ts_240, base_ts.wrapping_add(375));
        assert!((ts_240.wrapping_sub(ts_dup) as i32) > 0);
    }

    #[test]
    fn test_request_keyframe_coalesced_cooldown() {
        let flag = AtomicBool::new(false);
        let last_ms = AtomicU64::new(0);

        assert!(request_keyframe_coalesced(&flag, &last_ms, 1_000));
        assert!(flag.load(Ordering::Acquire));

        flag.store(false, Ordering::Release);
        assert!(!request_keyframe_coalesced(&flag, &last_ms, 1_000));
        assert!(!flag.load(Ordering::Acquire));

        assert!(request_keyframe_coalesced(&flag, &last_ms, 0));
        assert!(flag.load(Ordering::Acquire));
    }

    #[test]
    fn test_is_usable_local_ice_ip_filters_ipv6_and_apipa() {
        use std::net::{Ipv4Addr, Ipv6Addr};

        assert!(!is_usable_local_ice_ip(IpAddr::V6(
            "2804:4390:200a:d500:9c31:a719:dc87:3fc2".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_usable_local_ice_ip(IpAddr::V6(
            "fe80::a873:2144:e2a2:2020".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_usable_local_ice_ip(IpAddr::V6(
            "fdfd::1a26:3ae9".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_usable_local_ice_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_usable_local_ice_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 83, 107))));
        assert!(!is_usable_local_ice_ip(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));

        assert!(is_usable_local_ice_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))));
        assert!(is_usable_local_ice_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))));
        assert!(is_usable_local_ice_ip(IpAddr::V4(Ipv4Addr::new(26, 38, 58, 233))));
    }

    #[tokio::test]
    async fn test_ice_restart_rotates_ice_ufrag_in_place() {
        let api = build_webrtc_api();
        let sender_pc = api
            .new_peer_connection(RTCConfiguration {
                ice_servers: vec![],
                ..Default::default()
            })
            .await
            .expect("sender pc");
        let receiver_pc = api
            .new_peer_connection(RTCConfiguration {
                ice_servers: vec![],
                ..Default::default()
            })
            .await
            .expect("receiver pc");

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-restart-test".to_string(),
        ));
        sender_pc
            .add_transceiver_from_track(
                video_track as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .expect("add transceiver");

        let mut gather_done = sender_pc.gathering_complete_promise().await;
        let initial_offer = sender_pc.create_offer(None).await.expect("initial offer");
        let (_, ufrag1) = extract_sdp_mid_and_ufrag(&initial_offer.sdp);
        sender_pc
            .set_local_description(initial_offer.clone())
            .await
            .expect("set local");
        receiver_pc
            .set_remote_description(initial_offer)
            .await
            .expect("recv set remote");
        let answer = receiver_pc.create_answer(None).await.expect("answer");
        receiver_pc
            .set_local_description(answer.clone())
            .await
            .expect("recv set local");
        sender_pc
            .set_remote_description(answer)
            .await
            .expect("sender set remote");
        assert_eq!(sender_pc.signaling_state(), RTCSignalingState::Stable);

        let _ = tokio::time::timeout(Duration::from_secs(5), gather_done.recv()).await;
        assert_eq!(sender_pc.ice_gathering_state(), RTCIceGatheringState::Complete);

        let restart_offer = sender_pc
            .create_offer(Some(RTCOfferOptions {
                ice_restart: true,
                voice_activity_detection: false,
            }))
            .await
            .expect("restart offer");
        let (_, ufrag2) = extract_sdp_mid_and_ufrag(&restart_offer.sdp);
        assert_ne!(ufrag1, ufrag2, "ICE restart deve gerar novo ice-ufrag");
        sender_pc
            .set_local_description(restart_offer)
            .await
            .expect("set local restart offer");

        let _ = sender_pc.close().await;
        let _ = receiver_pc.close().await;
    }

    #[tokio::test]
    async fn test_browser_offer_answer_and_local_ipv4_ice_dtls_connect() {
        use webrtc::util::vnet::net::{Net, NetConfig};
        use webrtc::util::vnet::router::{Router, RouterConfig};

        let router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "192.168.1.0/24".to_string(),
                ..Default::default()
            })
            .expect("vnet router"),
        ));
        let net0 = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["192.168.1.10".to_string()],
            ..Default::default()
        })));
        let net1 = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["192.168.1.20".to_string()],
            ..Default::default()
        })));
        {
            let nic0 = net0.get_nic().expect("nic0");
            let nic1 = net1.get_nic().expect("nic1");
            {
                let mut r = router.lock().await;
                r.add_net(nic0.clone()).await.expect("add net0");
                r.add_net(nic1.clone()).await.expect("add net1");
            }
            nic0.lock()
                .await
                .set_router(Arc::clone(&router))
                .await
                .expect("set router0");
            nic1.lock()
                .await
                .set_router(Arc::clone(&router))
                .await
                .expect("set router1");
            router.lock().await.start().await.expect("start router");
        }

        let sender_api = build_webrtc_api_internal(Some(net0));
        let receiver_api = build_webrtc_api_internal(Some(net1));
        let sender_pc = Arc::new(
            sender_api
                .new_peer_connection(RTCConfiguration {
                    ice_servers: vec![],
                    ..Default::default()
                })
                .await
                .expect("sender pc"),
        );
        let receiver_pc = Arc::new(
            receiver_api
                .new_peer_connection(RTCConfiguration {
                    ice_servers: vec![],
                    ..Default::default()
                })
                .await
                .expect("receiver pc"),
        );

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-test-screen".to_string(),
        ));
        sender_pc
            .add_transceiver_from_track(
                video_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .expect("add transceiver");

        let recv_pkts = Arc::new(AtomicU32::new(0));
        let recv_pkts_cb = recv_pkts.clone();
        receiver_pc.on_track(Box::new(move |track, _, _| {
            let recv_pkts_cb = recv_pkts_cb.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    while let Ok((_pkt, _)) = track.read_rtp().await {
                        recv_pkts_cb.fetch_add(1, Ordering::Relaxed);
                    }
                });
            })
        }));

        let (s_ice_tx, mut s_ice_rx) = mpsc::unbounded_channel::<String>();
        let (r_ice_tx, mut r_ice_rx) = mpsc::unbounded_channel::<String>();

        sender_pc.on_ice_candidate(Box::new(move |c| {
            let s_ice_tx = s_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = s_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));
        receiver_pc.on_ice_candidate(Box::new(move |c| {
            let r_ice_tx = r_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = r_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));

        let offer = sender_pc.create_offer(None).await.expect("create offer");
        // Nenhum candidato/endpoint local deve anunciar IPv6 ou APIPA no SDP/ICE
        sender_pc
            .set_local_description(offer.clone())
            .await
            .expect("sender set local");
        receiver_pc
            .set_remote_description(offer)
            .await
            .expect("receiver set remote");
        let answer = receiver_pc.create_answer(None).await.expect("create answer");
        receiver_pc
            .set_local_description(answer.clone())
            .await
            .expect("receiver set local");
        sender_pc
            .set_remote_description(answer)
            .await
            .expect("sender set remote");

        // Injeta candidato .local + end-of-candidates vazio para testar o filtro resiliente
        add_remote_ice_with_mdns_fallback(
            &sender_pc,
            RTCIceCandidateInit {
                candidate: "".to_string(),
                sdp_mid: Some("0".to_string()),
                sdp_mline_index: Some(0),
                username_fragment: None,
            },
        )
        .await;
        add_remote_ice_with_mdns_fallback(
            &sender_pc,
            RTCIceCandidateInit {
                candidate:
                    "candidate:1 1 udp 2122260223 nonexistent-mdns-test.local 50000 typ host"
                        .to_string(),
                sdp_mid: Some("0".to_string()),
                sdp_mline_index: Some(0),
                username_fragment: None,
            },
        )
        .await;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            while let Ok(c) = s_ice_rx.try_recv() {
                if c.contains("typ host") {
                    assert!(
                        c.contains("192.168.1.10"),
                        "host candidate must be IPv4 192.168.1.10 (no IPv6/APIPA): {}",
                        c
                    );
                }
                let json = format_ice_candidate_json(&c, "0", None);
                let init: RTCIceCandidateInit = serde_json::from_value(json).unwrap();
                add_remote_ice_with_mdns_fallback(&receiver_pc, init).await;
            }
            while let Ok(c) = r_ice_rx.try_recv() {
                if c.contains("typ host") {
                    assert!(
                        c.contains("192.168.1.20"),
                        "host candidate must be IPv4 192.168.1.20 (no IPv6/APIPA): {}",
                        c
                    );
                }
                let json = format_ice_candidate_json(&c, "0", None);
                let init: RTCIceCandidateInit = serde_json::from_value(json).unwrap();
                add_remote_ice_with_mdns_fallback(&sender_pc, init).await;
            }
            if sender_pc.connection_state() == RTCPeerConnectionState::Connected
                && receiver_pc.connection_state() == RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert_eq!(
            sender_pc.connection_state(),
            RTCPeerConnectionState::Connected,
            "sender pc must reach Connected via IPv4 ICE+DTLS"
        );
        assert_eq!(
            receiver_pc.connection_state(),
            RTCPeerConnectionState::Connected,
            "receiver pc must reach Connected via IPv4 ICE+DTLS"
        );

        // Envia IDR Annex-B sintetico pelo H264Payloader e valida entregaRTP
        let idr_au = Bytes::from_static(&[
            0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0x8D, 0x8D, 0x40, 0x50, 0x1E, 0xD0, 0x0F, 0x08,
            0x84, 0x6A, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65, 0x88, 0x84, 0x21,
            0xA0, 0x11, 0x22, 0x33,
        ]);
        let mut payloader = H264Payloader::default();
        let payloads = payloader.payload(H264_RTP_MTU, &idr_au).expect("payload");
        let n = payloads.len();
        for (i, payload) in payloads.into_iter().enumerate() {
            let pkt = RtpPacket {
                header: RtpHeader {
                    version: 2,
                    marker: i + 1 == n,
                    sequence_number: (i + 1) as u16,
                    timestamp: 90_000,
                    ..Default::default()
                },
                payload,
            };
            let _ = video_track.write_rtp(&pkt).await;
        }

        let rtp_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < rtp_deadline && recv_pkts.load(Ordering::Relaxed) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            recv_pkts.load(Ordering::Relaxed) > 0,
            "receiver must receive H264 RTP packets over connected IPv4 DTLS/SRTP"
        );

        let _ = sender_pc.close().await;
        let _ = receiver_pc.close().await;
        let _ = router.lock().await.stop().await;
    }

    #[tokio::test]
    async fn test_patched_dtls_accepts_chrome_pq_grease_and_fragmented_client_hello() {
        use webrtc::dtls::config::{ClientAuthType, Config as DtlsConfig, ExtendedMasterSecretType};
        use webrtc::dtls::conn::DTLSConn;
        use webrtc::dtls::crypto::Certificate;
        use webrtc::dtls::extension::extension_use_srtp::SrtpProtectionProfile;
        use webrtc::util::conn::conn_pipe::pipe;
        use webrtc::util::Conn;

        // Constroi ClientHello real estilo Chrome 131-146+:
        // - session_id nao-vazio (32 bytes)
        // - GREASE + cipher suites TLS 1.3 + TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 (0xc02b)
        // - supported_groups (0x000a): GREASE (0x2a2a), X25519MLKEM768 (0x11ec), X25519Kyber768 (0x6399), X25519 (0x001d), P256 (0x0017)
        // - extensoes DTLS 1.3 desconhecidas: supported_versions (0x002b) + key_share (0x0033) de 1216B (>1400B fragmentado)
        let build_chrome_client_hello = |cookie: &[u8]| -> Vec<u8> {
            let mut body = Vec::new();
            body.extend_from_slice(&[0xfe, 0xfd]); // DTLS 1.2 legacy_version
            body.extend_from_slice(&[0x42; 32]); // random
            body.push(32);
            body.extend_from_slice(&[0x11; 32]); // session_id
            body.push(cookie.len() as u8);
            body.extend_from_slice(cookie);
            let suites: [u16; 3] = [0x4a4a, 0x1301, 0xc02b];
            body.extend_from_slice(&((suites.len() * 2) as u16).to_be_bytes());
            for s in suites {
                body.extend_from_slice(&s.to_be_bytes());
            }
            body.extend_from_slice(&[1, 0]); // compression_methods

            let mut exts = Vec::new();
            // GREASE extension
            exts.extend_from_slice(&0x6a6au16.to_be_bytes());
            exts.extend_from_slice(&0u16.to_be_bytes());
            // supported_groups (0x000a)
            let groups: [u16; 5] = [0x2a2a, 0x11ec, 0x6399, 0x001d, 0x0017];
            exts.extend_from_slice(&0x000au16.to_be_bytes());
            exts.extend_from_slice(&((2 + groups.len() * 2) as u16).to_be_bytes());
            exts.extend_from_slice(&((groups.len() * 2) as u16).to_be_bytes());
            for g in groups {
                exts.extend_from_slice(&g.to_be_bytes());
            }
            // ec_point_formats (0x000b)
            exts.extend_from_slice(&[0x00, 0x0b, 0x00, 0x02, 0x01, 0x00]);
            // use_srtp (0x000e)
            exts.extend_from_slice(&[
                0x00, 0x0e, 0x00, 0x07, 0x00, 0x04, 0x00, 0x07, 0x00, 0x01, 0x00,
            ]);
            // extended_master_secret (0x0017)
            exts.extend_from_slice(&[0x00, 0x17, 0x00, 0x00]);
            // supported_versions (0x002b): DTLS 1.3 + DTLS 1.2
            exts.extend_from_slice(&[0x00, 0x2b, 0x00, 0x05, 0x04, 0xfe, 0xfc, 0xfe, 0xfd]);
            // key_share (0x0033): 1216 bytes (ML-KEM-768)
            exts.extend_from_slice(&0x0033u16.to_be_bytes());
            exts.extend_from_slice(&1216u16.to_be_bytes());
            exts.extend_from_slice(&vec![0xab; 1216]);

            body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
            body.extend_from_slice(&exts);
            body
        };

        let make_dtls_record =
            |rec_seq: u64, msg_seq: u16, total_len: usize, offset: usize, chunk: &[u8]| -> Vec<u8> {
                let mut hs = Vec::with_capacity(12 + chunk.len());
                hs.push(1); // HandshakeType::ClientHello
                let len_be = (total_len as u32).to_be_bytes();
                hs.extend_from_slice(&len_be[1..4]);
                hs.extend_from_slice(&msg_seq.to_be_bytes());
                let off_be = (offset as u32).to_be_bytes();
                hs.extend_from_slice(&off_be[1..4]);
                let flen_be = (chunk.len() as u32).to_be_bytes();
                hs.extend_from_slice(&flen_be[1..4]);
                hs.extend_from_slice(chunk);

                let mut rec = Vec::with_capacity(13 + hs.len());
                rec.push(22); // ContentType::Handshake
                rec.extend_from_slice(&[0xfe, 0xff]); // DTLS 1.0 record version (padrao RFC)
                rec.extend_from_slice(&0u16.to_be_bytes()); // epoch 0
                rec.extend_from_slice(&rec_seq.to_be_bytes()[2..8]); // uint48 seq
                rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
                rec.extend_from_slice(&hs);
                rec
            };

        let (client_conn, server_conn) = pipe();
        let cert = Certificate::generate_self_signed(vec!["golive-lite.local".to_string()])
            .expect("self-signed cert");
        let server_cfg = DtlsConfig {
            certificates: vec![cert],
            extended_master_secret: ExtendedMasterSecretType::Request,
            srtp_protection_profiles: vec![
                SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
                SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80,
            ],
            client_auth: ClientAuthType::RequireAnyClientCert,
            insecure_skip_verify: true,
            ..Default::default()
        };

        let server_task = tokio::spawn(async move {
            DTLSConn::new(
                Arc::new(server_conn) as Arc<dyn Conn + Send + Sync>,
                server_cfg,
                false,
                None,
            )
            .await
        });

        // Flight 1: ClientHello fragmentado em 2 datagramas + duplicata do fragmento 0
        let ch1 = build_chrome_client_hello(&[]);
        assert!(ch1.len() > 1300);
        let split_at = 900usize;
        client_conn
            .send(&make_dtls_record(0, 0, ch1.len(), 0, &ch1[..split_at]))
            .await
            .unwrap();
        client_conn
            .send(&make_dtls_record(1, 0, ch1.len(), 0, &ch1[..split_at]))
            .await
            .unwrap();
        client_conn
            .send(&make_dtls_record(2, 0, ch1.len(), split_at, &ch1[split_at..]))
            .await
            .unwrap();

        // Servidor DEVE responder HelloVerifyRequest (22 / handshake type 3), e NAO Fatal Alert (21)!
        let mut buf = vec![0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(3), client_conn.recv(&mut buf))
            .await
            .expect("DTLS server must reply promptly")
            .expect("recv HelloVerifyRequest");
        assert!(n >= 28, "HelloVerifyRequest record too short: {}", n);
        assert_eq!(
            buf[0], 22,
            "expected DTLS Handshake (22), got content_type={} (21 = Fatal Alert!)",
            buf[0]
        );
        assert_eq!(
            buf[13], 3,
            "expected HelloVerifyRequest (3), got handshake_type={}",
            buf[13]
        );
        // Extrai cookie do HelloVerifyRequest: 13B record + 12B hs header + 2B version + 1B cookie_len
        let cookie_len = buf[27] as usize;
        let cookie = buf[28..28 + cookie_len].to_vec();
        assert!(!cookie.is_empty());

        // Flight 3: ClientHello fragmentado COM cookie -> Servidor DEVE responder ServerHello (type 2)!
        let ch2 = build_chrome_client_hello(&cookie);
        client_conn
            .send(&make_dtls_record(3, 1, ch2.len(), 0, &ch2[..split_at]))
            .await
            .unwrap();
        client_conn
            .send(&make_dtls_record(4, 1, ch2.len(), split_at, &ch2[split_at..]))
            .await
            .unwrap();

        let n2 = tokio::time::timeout(Duration::from_secs(3), client_conn.recv(&mut buf))
            .await
            .expect("DTLS server must reply ServerHello")
            .expect("recv ServerHello");
        assert!(n2 >= 26, "ServerHello flight too short: {}", n2);
        assert_eq!(buf[0], 22, "expected Handshake record for ServerHello");
        assert_eq!(buf[13], 2, "expected ServerHello (2), got {}", buf[13]);

        // Verifica que ServerHello nao inclui a extensao proibida supported_groups (0x000a)
        let sh_record_len = u16::from_be_bytes([buf[11], buf[12]]) as usize;
        let sh_body = &buf[25..13 + sh_record_len];
        // ServerHello: 2B version + 32B random + 1B sid_len(=0) + 2B cipher + 1B comp + 2B ext_len
        let ext_len = u16::from_be_bytes([sh_body[38], sh_body[39]]) as usize;
        let sh_exts = &sh_body[40..40 + ext_len];
        let mut off = 0;
        while off + 4 <= sh_exts.len() {
            let ext_type = u16::from_be_bytes([sh_exts[off], sh_exts[off + 1]]);
            let elen = u16::from_be_bytes([sh_exts[off + 2], sh_exts[off + 3]]) as usize;
            assert_ne!(
                ext_type, 0x000a,
                "DTLS 1.2 ServerHello must not include supported_groups (RFC 8422)"
            );
            off += 4 + elen;
        }

        let _ = client_conn.close().await;
        server_task.abort();
    }

    #[test]
    fn test_sanitize_ice_servers_drops_port_53_and_tcp_turn() {
        let input = vec![IceServerConfig {
            urls: vec![
                "turn:turn.cloudflare.com:3478?transport=udp".to_string(),
                "turn:turn.cloudflare.com:53?transport=udp".to_string(),
                "turn:turn.cloudflare.com:53".to_string(),
                "turn:turn.cloudflare.com:3478?transport=tcp".to_string(),
                "turns:turn.cloudflare.com:443?transport=tcp".to_string(),
            ],
            username: Some("u".to_string()),
            credential: Some("c".to_string()),
        }];
        let (out, has_turn) = sanitize_ice_servers(input);
        assert!(has_turn);
        let turn_urls: Vec<&str> = out
            .iter()
            .flat_map(|s| s.urls.iter().map(String::as_str))
            .filter(|u| u.starts_with("turn"))
            .collect();
        assert_eq!(turn_urls, vec!["turn:turn.cloudflare.com:3478?transport=udp"]);
    }

    #[tokio::test]
    async fn test_browser_mdns_only_prflx_connect_and_rtp() {
        use webrtc::util::vnet::net::{Net, NetConfig};
        use webrtc::util::vnet::router::{Router, RouterConfig};

        let router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "192.168.1.0/24".to_string(),
                ..Default::default()
            })
            .expect("vnet router"),
        ));
        let net_sender = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["192.168.1.10".to_string()],
            ..Default::default()
        })));
        let net_browser = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["192.168.1.20".to_string()],
            ..Default::default()
        })));
        {
            let nic0 = net_sender.get_nic().expect("nic0");
            let nic1 = net_browser.get_nic().expect("nic1");
            {
                let mut r = router.lock().await;
                r.add_net(nic0.clone()).await.expect("add net0");
                r.add_net(nic1.clone()).await.expect("add net1");
            }
            nic0.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            nic1.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            router.lock().await.start().await.unwrap();
        }

        let sender_api = build_webrtc_api_internal(Some(net_sender));
        let browser_api = build_webrtc_api_internal(Some(net_browser));
        let sender_pc = Arc::new(
            sender_api
                .new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        let browser_pc = Arc::new(
            browser_api
                .new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-mdns-test".to_string(),
        ));
        sender_pc
            .add_transceiver_from_track(
                video_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();

        let recv_pkts = Arc::new(AtomicU32::new(0));
        let recv_pkts_cb = recv_pkts.clone();
        browser_pc.on_track(Box::new(move |track, _, _| {
            let recv_pkts_cb = recv_pkts_cb.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    while let Ok((_pkt, _)) = track.read_rtp().await {
                        recv_pkts_cb.fetch_add(1, Ordering::Relaxed);
                    }
                });
            })
        }));

        let (s_ice_tx, mut s_ice_rx) = mpsc::unbounded_channel::<String>();
        sender_pc.on_ice_candidate(Box::new(move |c| {
            let s_ice_tx = s_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = s_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));

        // Navegador SEM permissao de microfone: nao entrega IPv4 local ao sender (apenas .local mDNS)
        let offer = sender_pc.create_offer(None).await.unwrap();
        sender_pc.set_local_description(offer.clone()).await.unwrap();
        browser_pc.set_remote_description(offer).await.unwrap();
        let answer = browser_pc.create_answer(None).await.unwrap();
        browser_pc.set_local_description(answer.clone()).await.unwrap();
        sender_pc.set_remote_description(answer).await.unwrap();

        add_remote_ice_with_mdns_fallback(
            &sender_pc,
            RTCIceCandidateInit {
                candidate:
                    "candidate:1 1 udp 2122260223 11111111-2222-3333-4444-555555555555.local 54321 typ host"
                        .to_string(),
                sdp_mid: Some("0".to_string()),
                sdp_mline_index: Some(0),
                username_fragment: None,
            },
        )
        .await;

        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            while let Ok(c) = s_ice_rx.try_recv() {
                let json = format_ice_candidate_json(&c, "0", None);
                let init: RTCIceCandidateInit = serde_json::from_value(json).unwrap();
                add_remote_ice_with_mdns_fallback(&browser_pc, init).await;
            }
            if sender_pc.connection_state() == RTCPeerConnectionState::Connected
                && browser_pc.connection_state() == RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert_eq!(
            sender_pc.connection_state(),
            RTCPeerConnectionState::Connected,
            "sender must connect via prflx even when browser hides IP behind .local mDNS"
        );
        assert_eq!(
            browser_pc.connection_state(),
            RTCPeerConnectionState::Connected,
            "browser must reach Connected"
        );

        let idr_au = Bytes::from_static(&[
            0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0x8D, 0x8D, 0x40, 0x50, 0x1E, 0xD0, 0x0F, 0x08,
            0x84, 0x6A, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65, 0x88, 0x84, 0x21,
            0xA0, 0x11, 0x22, 0x33,
        ]);
        let mut payloader = H264Payloader::default();
        let payloads = payloader.payload(H264_RTP_MTU, &idr_au).unwrap();
        let n = payloads.len();
        for (i, payload) in payloads.into_iter().enumerate() {
            let pkt = RtpPacket {
                header: RtpHeader {
                    version: 2,
                    marker: i + 1 == n,
                    sequence_number: (i + 1) as u16,
                    timestamp: 90_000,
                    ..Default::default()
                },
                payload,
            };
            let _ = video_track.write_rtp(&pkt).await;
        }
        let rtp_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < rtp_deadline && recv_pkts.load(Ordering::Relaxed) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            recv_pkts.load(Ordering::Relaxed) > 0,
            "browser must receive H264 RTP over prflx+DTLS"
        );

        let _ = sender_pc.close().await;
        let _ = browser_pc.close().await;
        let _ = router.lock().await.stop().await;
    }

    #[tokio::test]
    async fn test_non_local_wan_nat_ice_dtls_and_rtp() {
        use webrtc::util::vnet::nat::{EndpointDependencyType, NatMode, NatType};
        use webrtc::util::vnet::net::{Net, NetConfig};
        use webrtc::util::vnet::router::{Nic, Router, RouterConfig};

        // Topologia NAO-LOCAL (WAN + NAT):
        // WAN (203.0.113.0/24) <-> Remote browser (203.0.113.20)
        //                      <-> NAT Router (WAN 203.0.113.10, LAN 10.10.1.0/24) <-> Sender (10.10.1.10)
        let wan_router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "203.0.113.0/24".to_string(),
                min_delay: Duration::from_millis(10),
                ..Default::default()
            })
            .expect("wan router"),
        ));
        let nat_router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "10.10.1.0/24".to_string(),
                static_ips: vec!["203.0.113.10".to_string()],
                nat_type: Some(NatType {
                    mode: NatMode::Normal,
                    mapping_behavior: EndpointDependencyType::EndpointIndependent,
                    filtering_behavior: EndpointDependencyType::EndpointAddrPortDependent,
                    mapping_life_time: Duration::from_secs(30),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .expect("nat router"),
        ));

        let net_sender = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.10.1.10".to_string()],
            ..Default::default()
        })));
        let net_remote = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["203.0.113.20".to_string()],
            ..Default::default()
        })));

        {
            let nic_sender = net_sender.get_nic().unwrap();
            let nic_remote = net_remote.get_nic().unwrap();
            {
                let mut w = wan_router.lock().await;
                w.add_net(nic_remote.clone()).await.unwrap();
                w.add_net(Arc::clone(&nat_router) as Arc<Mutex<dyn Nic + Send + Sync>>)
                    .await
                    .unwrap();
            }
            nic_remote
                .lock()
                .await
                .set_router(Arc::clone(&wan_router))
                .await
                .unwrap();
            nat_router
                .lock()
                .await
                .set_router(Arc::clone(&wan_router))
                .await
                .unwrap();
            {
                let mut n = nat_router.lock().await;
                n.add_net(nic_sender.clone()).await.unwrap();
            }
            nic_sender
                .lock()
                .await
                .set_router(Arc::clone(&nat_router))
                .await
                .unwrap();
            wan_router.lock().await.start().await.unwrap();
            nat_router.lock().await.start().await.unwrap();
        }

        let sender_api = build_webrtc_api_internal(Some(net_sender));
        let remote_api = build_webrtc_api_internal(Some(net_remote));
        let sender_pc = Arc::new(
            sender_api
                .new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        let remote_pc = Arc::new(
            remote_api
                .new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-wan-test".to_string(),
        ));
        sender_pc
            .add_transceiver_from_track(
                video_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();

        let recv_pkts = Arc::new(AtomicU32::new(0));
        let recv_pkts_cb = recv_pkts.clone();
        remote_pc.on_track(Box::new(move |track, _, _| {
            let recv_pkts_cb = recv_pkts_cb.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    while let Ok((_pkt, _)) = track.read_rtp().await {
                        recv_pkts_cb.fetch_add(1, Ordering::Relaxed);
                    }
                });
            })
        }));

        let (s_ice_tx, mut s_ice_rx) = mpsc::unbounded_channel::<String>();
        let (r_ice_tx, mut r_ice_rx) = mpsc::unbounded_channel::<String>();
        sender_pc.on_ice_candidate(Box::new(move |c| {
            let s_ice_tx = s_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = s_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));
        remote_pc.on_ice_candidate(Box::new(move |c| {
            let r_ice_tx = r_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = r_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));

        let offer = sender_pc.create_offer(None).await.unwrap();
        sender_pc.set_local_description(offer.clone()).await.unwrap();
        remote_pc.set_remote_description(offer).await.unwrap();
        let answer = remote_pc.create_answer(None).await.unwrap();
        remote_pc.set_local_description(answer.clone()).await.unwrap();
        sender_pc.set_remote_description(answer).await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            while let Ok(c) = s_ice_rx.try_recv() {
                let json = format_ice_candidate_json(&c, "0", None);
                let init: RTCIceCandidateInit = serde_json::from_value(json).unwrap();
                add_remote_ice_with_mdns_fallback(&remote_pc, init).await;
            }
            while let Ok(c) = r_ice_rx.try_recv() {
                let json = format_ice_candidate_json(&c, "0", None);
                let init: RTCIceCandidateInit = serde_json::from_value(json).unwrap();
                add_remote_ice_with_mdns_fallback(&sender_pc, init).await;
            }
            if sender_pc.connection_state() == RTCPeerConnectionState::Connected
                && remote_pc.connection_state() == RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert_eq!(
            sender_pc.connection_state(),
            RTCPeerConnectionState::Connected,
            "sender behind NAT must connect to non-local WAN peer"
        );
        assert_eq!(
            remote_pc.connection_state(),
            RTCPeerConnectionState::Connected,
            "remote WAN peer must reach Connected"
        );

        let idr_au = Bytes::from_static(&[
            0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0x8D, 0x8D, 0x40, 0x50, 0x1E, 0xD0, 0x0F, 0x08,
            0x84, 0x6A, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65, 0x88, 0x84, 0x21,
            0xA0, 0x11, 0x22, 0x33,
        ]);
        let mut payloader = H264Payloader::default();
        let payloads = payloader.payload(H264_RTP_MTU, &idr_au).unwrap();
        let n = payloads.len();
        for (i, payload) in payloads.into_iter().enumerate() {
            let pkt = RtpPacket {
                header: RtpHeader {
                    version: 2,
                    marker: i + 1 == n,
                    sequence_number: (i + 1) as u16,
                    timestamp: 90_000,
                    ..Default::default()
                },
                payload,
            };
            let _ = video_track.write_rtp(&pkt).await;
        }
        let rtp_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < rtp_deadline && recv_pkts.load(Ordering::Relaxed) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            recv_pkts.load(Ordering::Relaxed) > 0,
            "remote WAN peer must receive H264 RTP across NAT"
        );

        let _ = sender_pc.close().await;
        let _ = remote_pc.close().await;
        let _ = nat_router.lock().await.stop().await;
        let _ = wan_router.lock().await.stop().await;
    }

    #[test]
    fn test_compute_monotonic_audio_rtp_ts_48khz_across_vad_gaps_and_restart() {
        let mut last_seq = None;
        let mut last_ts = None;
        let base = 1_000_000u32;

        let t0 = compute_monotonic_audio_rtp_ts_48khz(10, base, &mut last_seq, &mut last_ts);
        let t1 = compute_monotonic_audio_rtp_ts_48khz(11, base, &mut last_seq, &mut last_ts);
        // Gap VAD de 50 frames (1 segundo de silencio)
        let t51 = compute_monotonic_audio_rtp_ts_48khz(61, base, &mut last_seq, &mut last_ts);
        // Frame repetido ou reinicio nao retrocede o timestamp
        let trestart = compute_monotonic_audio_rtp_ts_48khz(0, base, &mut last_seq, &mut last_ts);

        assert_eq!(t0, base);
        assert_eq!(t1.wrapping_sub(t0), 960);
        assert_eq!(t51.wrapping_sub(t1), 50 * 960);
        assert_eq!(trestart.wrapping_sub(t51), 960);
    }

    #[tokio::test]
    async fn test_mic_rtp_over_vnet_preserves_timestamps_and_talkspurt_marker() {
        use crate::media::codecs::OpusAudioDecoder;
        use webrtc::util::vnet::net::{Net, NetConfig};
        use webrtc::util::vnet::router::{Router, RouterConfig};

        let router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "10.20.0.0/24".to_string(),
                min_delay: Duration::from_millis(5),
                ..Default::default()
            })
            .expect("router"),
        ));
        let net_a = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.20.0.10".to_string()],
            ..Default::default()
        })));
        let net_b = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.20.0.20".to_string()],
            ..Default::default()
        })));
        {
            let nic_a = net_a.get_nic().unwrap();
            let nic_b = net_b.get_nic().unwrap();
            {
                let mut r = router.lock().await;
                r.add_net(nic_a.clone()).await.unwrap();
                r.add_net(nic_b.clone()).await.unwrap();
            }
            nic_a.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            nic_b.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            router.lock().await.start().await.unwrap();
        }

        let api_a = build_webrtc_api_internal(Some(net_a));
        let api_b = build_webrtc_api_internal(Some(net_b));
        let pc_a = Arc::new(api_a.new_peer_connection(RTCConfiguration::default()).await.unwrap());
        let pc_b = Arc::new(api_b.new_peer_connection(RTCConfiguration::default()).await.unwrap());

        let audio_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                ..Default::default()
            },
            "audio".to_string(),
            "golive-lite-mic-test".to_string(),
        ));
        pc_a.add_transceiver_from_track(
            audio_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Sendonly,
                send_encodings: vec![],
            }),
        )
        .await
        .unwrap();

        let (pkt_tx, mut pkt_rx) = mpsc::unbounded_channel::<(bool, u32, Bytes)>();
        pc_b.on_track(Box::new(move |track, _, _| {
            let pkt_tx = pkt_tx.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    while let Ok((pkt, _)) = track.read_rtp().await {
                        let _ = pkt_tx.send((pkt.header.marker, pkt.header.timestamp, pkt.payload));
                    }
                });
            })
        }));

        let (a_ice_tx, mut a_ice_rx) = mpsc::unbounded_channel::<String>();
        let (b_ice_tx, mut b_ice_rx) = mpsc::unbounded_channel::<String>();
        pc_a.on_ice_candidate(Box::new(move |c| {
            let a_ice_tx = a_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = a_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));
        pc_b.on_ice_candidate(Box::new(move |c| {
            let b_ice_tx = b_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = b_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));

        let offer = pc_a.create_offer(None).await.unwrap();
        pc_a.set_local_description(offer.clone()).await.unwrap();
        pc_b.set_remote_description(offer).await.unwrap();
        let answer = pc_b.create_answer(None).await.unwrap();
        pc_b.set_local_description(answer.clone()).await.unwrap();
        pc_a.set_remote_description(answer).await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            while let Ok(c) = a_ice_rx.try_recv() {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(format_ice_candidate_json(&c, "0", None)).unwrap();
                add_remote_ice_with_mdns_fallback(&pc_b, init).await;
            }
            while let Ok(c) = b_ice_rx.try_recv() {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(format_ice_candidate_json(&c, "0", None)).unwrap();
                add_remote_ice_with_mdns_fallback(&pc_a, init).await;
            }
            if pc_a.connection_state() == RTCPeerConnectionState::Connected
                && pc_b.connection_state() == RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(pc_a.connection_state(), RTCPeerConnectionState::Connected);
        assert_eq!(pc_b.connection_state(), RTCPeerConnectionState::Connected);

        let counters = Arc::new(SendCounters::default());
        let audio_tx =
            spawn_audio_peer_sender_task(audio_track.clone(), Some(counters.clone()), 42);
        let mut enc = OpusVoiceEncoder::new().unwrap();
        let mut tone = [0i16; 960];
        for (i, s) in tone.iter_mut().enumerate() {
            *s = (((i as f32) * 2.0 * std::f32::consts::PI * 330.0 / 48_000.0).sin() * 9000.0) as i16;
        }
        let opus_bytes = enc.encode_frame(&tone).unwrap();

        // Talkspurt 1 (seqs 0, 1, 2) + silencio VAD de 25 frames + Talkspurt 2 (seqs 28, 29)
        for (seq, marker) in [(0u64, true), (1, false), (2, false), (28, true), (29, false)] {
            audio_tx
                .send(AudioFanoutPacket {
                    data: opus_bytes.clone(),
                    frame_seq: seq,
                    marker,
                })
                .await
                .unwrap();
        }

        let mut received = Vec::new();
        let rx_deadline = Instant::now() + Duration::from_secs(3);
        while received.len() < 5 && Instant::now() < rx_deadline {
            match tokio::time::timeout(Duration::from_millis(300), pkt_rx.recv()).await {
                Ok(Some(item)) => received.push(item),
                _ => break,
            }
        }
        assert_eq!(received.len(), 5, "all 5 Opus RTP packets must arrive");

        let markers: Vec<bool> = received.iter().map(|r| r.0).collect();
        assert_eq!(markers, vec![true, false, false, true, false]);
        assert_eq!(received[1].1.wrapping_sub(received[0].1), 960);
        assert_eq!(received[2].1.wrapping_sub(received[1].1), 960);
        assert_eq!(received[3].1.wrapping_sub(received[2].1), 26 * 960);
        assert_eq!(received[4].1.wrapping_sub(received[3].1), 960);

        let mut dec = OpusAudioDecoder::new().unwrap();
        let mut decoded = Vec::new();
        for (_, _, payload) in &received {
            assert!(dec.decode_packet(payload, &mut decoded));
            assert_eq!(decoded.len(), 960);
        }

        let _ = pc_a.close().await;
        let _ = pc_b.close().await;
        let _ = router.lock().await.stop().await;
    }

    #[tokio::test]
    async fn test_screen_channel_muxes_video_and_audio_tracks_over_vnet() {
        use crate::media::codecs::{OpusAudioDecoder, OpusStereoAudioEncoder};
        use webrtc::util::vnet::net::{Net, NetConfig};
        use webrtc::util::vnet::router::{Router, RouterConfig};

        let router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "10.30.0.0/24".to_string(),
                min_delay: Duration::from_millis(5),
                ..Default::default()
            })
            .expect("router"),
        ));
        let net_a = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.30.0.10".to_string()],
            ..Default::default()
        })));
        let net_b = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.30.0.20".to_string()],
            ..Default::default()
        })));
        {
            let nic_a = net_a.get_nic().unwrap();
            let nic_b = net_b.get_nic().unwrap();
            {
                let mut r = router.lock().await;
                r.add_net(nic_a.clone()).await.unwrap();
                r.add_net(nic_b.clone()).await.unwrap();
            }
            nic_a.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            nic_b.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            router.lock().await.start().await.unwrap();
        }

        let api_a = build_webrtc_api_internal(Some(net_a));
        let api_b = build_webrtc_api_internal(Some(net_b));
        let pc_a = Arc::new(api_a.new_peer_connection(RTCConfiguration::default()).await.unwrap());
        let pc_b = Arc::new(api_b.new_peer_connection(RTCConfiguration::default()).await.unwrap());

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-screen".to_string(),
        ));
        let audio_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                ..Default::default()
            },
            "audio".to_string(),
            "golive-lite-screen".to_string(),
        ));

        for trk in [
            video_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
            audio_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
        ] {
            pc_a.add_transceiver_from_track(
                trk,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();
        }

        let (vid_tx, mut vid_rx) = mpsc::unbounded_channel::<Bytes>();
        let (aud_tx, mut aud_rx) = mpsc::unbounded_channel::<Bytes>();
        pc_b.on_track(Box::new(move |track, _, _| {
            let vid_tx = vid_tx.clone();
            let aud_tx = aud_tx.clone();
            Box::pin(async move {
                match track.kind() {
                    RTPCodecType::Video => {
                        tokio::spawn(async move {
                            while let Ok((pkt, _)) = track.read_rtp().await {
                                let _ = vid_tx.send(pkt.payload);
                            }
                        });
                    }
                    RTPCodecType::Audio => {
                        tokio::spawn(async move {
                            while let Ok((pkt, _)) = track.read_rtp().await {
                                let _ = aud_tx.send(pkt.payload);
                            }
                        });
                    }
                    _ => {}
                }
            })
        }));

        let (a_ice_tx, mut a_ice_rx) = mpsc::unbounded_channel::<String>();
        let (b_ice_tx, mut b_ice_rx) = mpsc::unbounded_channel::<String>();
        pc_a.on_ice_candidate(Box::new(move |c| {
            let a_ice_tx = a_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = a_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));
        pc_b.on_ice_candidate(Box::new(move |c| {
            let b_ice_tx = b_ice_tx.clone();
            Box::pin(async move {
                if let Some(cand) = c {
                    if let Ok(j) = cand.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = b_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));

        let offer = pc_a.create_offer(None).await.unwrap();
        pc_a.set_local_description(offer.clone()).await.unwrap();
        pc_b.set_remote_description(offer).await.unwrap();
        let answer = pc_b.create_answer(None).await.unwrap();
        pc_b.set_local_description(answer.clone()).await.unwrap();
        pc_a.set_remote_description(answer).await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            while let Ok(c) = a_ice_rx.try_recv() {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(format_ice_candidate_json(&c, "0", None)).unwrap();
                add_remote_ice_with_mdns_fallback(&pc_b, init).await;
            }
            while let Ok(c) = b_ice_rx.try_recv() {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(format_ice_candidate_json(&c, "0", None)).unwrap();
                add_remote_ice_with_mdns_fallback(&pc_a, init).await;
            }
            if pc_a.connection_state() == RTCPeerConnectionState::Connected
                && pc_b.connection_state() == RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(pc_a.connection_state(), RTCPeerConnectionState::Connected);
        assert_eq!(pc_b.connection_state(), RTCPeerConnectionState::Connected);

        let idr_au = Bytes::from_static(&[
            0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0x8D, 0x8D, 0x40, 0x50, 0x1E, 0xD0, 0x0F, 0x08,
            0x84, 0x6A, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65, 0x88, 0x84, 0x21,
            0xA0,
        ]);
        let mut payloader = H264Payloader::default();
        for (i, p) in payloader.payload(H264_RTP_MTU, &idr_au).unwrap().into_iter().enumerate() {
            let pkt = RtpPacket {
                header: RtpHeader {
                    version: 2,
                    marker: true,
                    sequence_number: (i + 1) as u16,
                    timestamp: 90_000,
                    ..Default::default()
                },
                payload: p,
            };
            let _ = video_track.write_rtp(&pkt).await;
        }

        let counters = Arc::new(SendCounters::default());
        let aud_tx_peer =
            spawn_audio_peer_sender_task(audio_track.clone(), Some(counters), 77);
        let mut enc = OpusStereoAudioEncoder::new().unwrap();
        let mut stereo = [0i16; 1920];
        for i in 0..960 {
            let s = (((i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin() * 8000.0)
                as i16;
            stereo[i * 2] = s;
            stereo[i * 2 + 1] = s;
        }
        let opus_pkt = enc.encode_frame(&stereo).unwrap();
        aud_tx_peer
            .send(AudioFanoutPacket {
                data: opus_pkt,
                frame_seq: 0,
                marker: true,
            })
            .await
            .unwrap();

        let got_vid = tokio::time::timeout(Duration::from_secs(2), vid_rx.recv())
            .await
            .ok()
            .flatten();
        let got_aud = tokio::time::timeout(Duration::from_secs(2), aud_rx.recv())
            .await
            .ok()
            .flatten();
        assert!(got_vid.is_some(), "muxed H264 video must arrive");
        let aud_payload = got_aud.expect("muxed Opus audio must arrive");
        let mut dec = OpusAudioDecoder::new().unwrap();
        let mut pcm_out = Vec::new();
        assert!(dec.decode_packet(&aud_payload, &mut pcm_out));
        assert_eq!(pcm_out.len(), 960);

        let _ = pc_a.close().await;
        let _ = pc_b.close().await;
        let _ = router.lock().await.stop().await;
    }

    #[tokio::test]
    async fn test_zero_playout_delay_extension_and_sdp() {
        let ext = ZeroPlayoutDelayExt;
        assert_eq!(ext.marshal_size(), 3);
        let mut small = [0u8; 2];
        assert!(ext.marshal_to(&mut small).is_err());
        let mut buf = [0xFFu8; 3];
        assert_eq!(ext.marshal_to(&mut buf).unwrap(), 3);
        assert_eq!(buf, [0u8, 0, 0]);

        let api = build_webrtc_api_internal(None);
        let pc = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await.unwrap());
        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-screen".to_string(),
        ));
        let _ = pc
            .add_track(track as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .unwrap();
        let offer = pc.create_offer(None).await.unwrap();
        assert!(
            offer.sdp.contains(PLAYOUT_DELAY_URI),
            "SDP offer must advertise playout-delay RTP header extension"
        );
        let _ = pc.close().await;
    }

    #[tokio::test]
    async fn test_video_fanout_bus_delivers_directly_without_polluting_video_counters() {
        use crate::media::codecs::OpusStereoAudioEncoder;
        use webrtc::util::vnet::net::{Net, NetConfig};
        use webrtc::util::vnet::router::{Router, RouterConfig};

        let router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "10.31.0.0/24".to_string(),
                min_delay: Duration::from_millis(2),
                ..Default::default()
            })
            .expect("router"),
        ));
        let net_a = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.31.0.10".to_string()],
            ..Default::default()
        })));
        let net_b = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec!["10.31.0.20".to_string()],
            ..Default::default()
        })));
        {
            let nic_a = net_a.get_nic().unwrap();
            let nic_b = net_b.get_nic().unwrap();
            {
                let mut r = router.lock().await;
                r.add_net(nic_a.clone()).await.unwrap();
                r.add_net(nic_b.clone()).await.unwrap();
            }
            nic_a.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            nic_b.lock().await.set_router(Arc::clone(&router)).await.unwrap();
            router.lock().await.start().await.unwrap();
        }

        let api_a = build_webrtc_api_internal(Some(net_a));
        let api_b = build_webrtc_api_internal(Some(net_b));
        let pc_a = Arc::new(api_a.new_peer_connection(RTCConfiguration::default()).await.unwrap());
        let pc_b = Arc::new(api_b.new_peer_connection(RTCConfiguration::default()).await.unwrap());

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                clock_rate: 90_000,
                channels: 0,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                        .to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "golive-lite-screen".to_string(),
        ));
        let audio_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                ..Default::default()
            },
            "audio".to_string(),
            "golive-lite-screen".to_string(),
        ));
        pc_a.add_track(video_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .unwrap();
        pc_a.add_track(audio_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .unwrap();

        let (vid_tx_rx, mut vid_rx) = mpsc::channel::<usize>(16);
        let (aud_tx_rx, mut aud_rx) = mpsc::channel::<usize>(16);
        pc_b.on_track(Box::new(move |track, _, _| {
            let vid_tx_rx = vid_tx_rx.clone();
            let aud_tx_rx = aud_tx_rx.clone();
            Box::pin(async move {
                let is_vid = track.kind() == RTPCodecType::Video;
                tokio::spawn(async move {
                    while let Ok((pkt, _)) = track.read_rtp().await {
                        if !pkt.payload.is_empty() {
                            if is_vid {
                                let _ = vid_tx_rx.send(pkt.payload.len()).await;
                            } else {
                                let _ = aud_tx_rx.send(pkt.payload.len()).await;
                            }
                        }
                    }
                });
            })
        }));

        let (a_ice_tx, mut a_ice_rx) = mpsc::unbounded_channel();
        let (b_ice_tx, mut b_ice_rx) = mpsc::unbounded_channel();
        pc_a.on_ice_candidate(Box::new(move |c| {
            let a_ice_tx = a_ice_tx.clone();
            Box::pin(async move {
                if let Some(c) = c {
                    if let Ok(j) = c.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = a_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));
        pc_b.on_ice_candidate(Box::new(move |c| {
            let b_ice_tx = b_ice_tx.clone();
            Box::pin(async move {
                if let Some(c) = c {
                    if let Ok(j) = c.to_json() {
                        if !j.candidate.is_empty() {
                            let _ = b_ice_tx.send(j.candidate);
                        }
                    }
                }
            })
        }));

        let offer = pc_a.create_offer(None).await.unwrap();
        pc_a.set_local_description(offer.clone()).await.unwrap();
        pc_b.set_remote_description(offer).await.unwrap();
        let answer = pc_b.create_answer(None).await.unwrap();
        pc_b.set_local_description(answer.clone()).await.unwrap();
        pc_a.set_remote_description(answer).await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            while let Ok(c) = a_ice_rx.try_recv() {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(format_ice_candidate_json(&c, "0", None)).unwrap();
                add_remote_ice_with_mdns_fallback(&pc_b, init).await;
            }
            while let Ok(c) = b_ice_rx.try_recv() {
                let init: RTCIceCandidateInit =
                    serde_json::from_value(format_ice_candidate_json(&c, "0", None)).unwrap();
                add_remote_ice_with_mdns_fallback(&pc_a, init).await;
            }
            if pc_a.connection_state() == RTCPeerConnectionState::Connected
                && pc_b.connection_state() == RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(pc_a.connection_state(), RTCPeerConnectionState::Connected);
        assert_eq!(pc_b.connection_state(), RTCPeerConnectionState::Connected);

        let counters = Arc::new(SendCounters::default());
        let kf_flag = Arc::new(AtomicBool::new(false));
        let kf_last_ms = Arc::new(AtomicU64::new(0));
        let (vid_peer_tx, vid_need_key) = spawn_video_peer_sender_task(
            video_track,
            counters.clone(),
            99,
            kf_flag.clone(),
            kf_last_ms.clone(),
        );
        // Audio acoplado ao canal de tela passa counters = None para nao inflar frames_sent do video
        let aud_peer_tx = spawn_audio_peer_sender_task(audio_track, None, 99);

        let vbus = VideoFanoutBus::new();
        {
            let mut guard = vbus.inner.write().unwrap();
            guard.insert(
                "screen".to_string(),
                vec![VideoFanoutTarget {
                    tx: vid_peer_tx,
                    need_key: vid_need_key,
                    channel_kf_flag: kf_flag,
                    channel_kf_last_ms: kf_last_ms,
                }],
            );
        }
        assert!(vbus.has_targets("screen"));

        let idr_au = Bytes::from_static(&[
            0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0x8D, 0x8D, 0x40, 0x50, 0x1E, 0xD0, 0x0F, 0x08,
            0x84, 0x6A, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65, 0x88, 0x84, 0x21,
            0xA0,
        ]);
        let p_au = Bytes::from_static(&[0, 0, 0, 1, 0x41, 0x9A, 0x22, 0x10]);
        vbus.send_encoded_video("screen", idr_au, true, Duration::from_millis(0));
        vbus.send_encoded_video("screen", p_au, false, Duration::from_millis(16));

        let mut enc = OpusStereoAudioEncoder::new().unwrap();
        let stereo = [1200i16; 1920];
        let opus_pkt = enc.encode_frame(&stereo).unwrap();
        for seq in 0..5u64 {
            aud_peer_tx
                .send(AudioFanoutPacket {
                    data: opus_pkt.clone(),
                    frame_seq: seq,
                    marker: seq == 0,
                })
                .await
                .unwrap();
        }

        let got_v1 = tokio::time::timeout(Duration::from_secs(2), vid_rx.recv())
            .await
            .ok()
            .flatten();
        let got_v2 = tokio::time::timeout(Duration::from_secs(2), vid_rx.recv())
            .await
            .ok()
            .flatten();
        let got_a1 = tokio::time::timeout(Duration::from_secs(2), aud_rx.recv())
            .await
            .ok()
            .flatten();
        assert!(got_v1.is_some() && got_v2.is_some(), "video frames must arrive via VideoFanoutBus");
        assert!(got_a1.is_some(), "screen audio packets must arrive");

        // Apenas os 2 quadros de video devem contar em counters.frames_sent (e nunca os 5 pacotes de audio)
        assert_eq!(counters.frames_sent.load(Ordering::Relaxed), 2);

        let _ = pc_a.close().await;
        let _ = pc_b.close().await;
        let _ = router.lock().await.stop().await;
    }

    #[test]
    fn test_remote_video_mailbox_overwrites_stale_frames_bounded_memory() {
        let mb = RemoteVideoMailbox::new();
        for seq in 0..100u8 {
            mb.push(DecodedRemoteVideo {
                channel: "screen".to_string(),
                peer_id: "peer-1".to_string(),
                width: 1920,
                height: 1080,
                orig_width: 1920,
                orig_height: 1080,
                pixels: vec![egui::Color32::from_gray(seq); 4],
            });
        }
        mb.push(DecodedRemoteVideo {
            channel: "camera".to_string(),
            peer_id: "peer-1".to_string(),
            width: 640,
            height: 360,
            orig_width: 640,
            orig_height: 360,
            pixels: vec![egui::Color32::from_gray(7); 4],
        });

        let drained = mb.drain_latest();
        assert_eq!(drained.len(), 2, "must store at most 1 latest frame per (channel, peer)");
        let screen = drained.iter().find(|f| f.channel == "screen").unwrap();
        assert_eq!(screen.pixels[0], egui::Color32::from_gray(99), "must keep only the most recent frame");
        assert!(mb.drain_latest().is_empty(), "drain_latest must empty the mailbox");

        mb.push(DecodedRemoteVideo {
            channel: "screen".to_string(),
            peer_id: "peer-2".to_string(),
            width: 1280,
            height: 720,
            orig_width: 1280,
            orig_height: 720,
            pixels: vec![egui::Color32::from_gray(1); 2],
        });
        mb.remove("screen", "peer-2");
        assert!(mb.drain_latest().is_empty());
    }

    #[test]
    fn test_remote_video_mailbox_recycles_buffers_and_bounds_pool() {
        let mb = RemoteVideoMailbox::new();
        for i in 0..10u8 {
            mb.push(DecodedRemoteVideo {
                channel: "screen".into(),
                peer_id: "p1".into(),
                width: 640,
                height: 360,
                orig_width: 1920,
                orig_height: 1080,
                pixels: Vec::with_capacity(64),
            });
            let _ = i;
        }
        // Pool holds bounded buffers (up to MAX_RECYCLED_FRAME_BUFS)
        let b1 = mb.take_recycled_buf();
        assert!(b1.capacity() >= 64);
        for _ in 0..10 {
            mb.recycle_buf(Vec::with_capacity(128));
        }
        assert_eq!(mb.recycle_pool.lock().unwrap().len(), MAX_RECYCLED_FRAME_BUFS);
    }
}
