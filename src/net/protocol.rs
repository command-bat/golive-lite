use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PRIVATE_ROOM_PREFIX: &str = "priv-";
pub const MAX_PRIVATE_ROOM_NAME_LENGTH: usize = 20;

/// Extrai o handle da sala a partir de um codigo puro ou URL colada (ex: golive.nemtudo.me/watch/sala, g.nemtudo.me/sala).
pub fn room_handle_from_input(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || (!trimmed.contains('.') && !trimmed.contains('/')) {
        return trimmed.to_string();
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    };
    let Ok(parsed) = url::Url::parse(&candidate) else {
        return trimmed.to_string();
    };
    let host = parsed
        .host_str()
        .unwrap_or("")
        .to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let segments: Vec<&str> = parsed
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if host == "golive.nemtudo.me" || host == "localhost" {
        if segments.len() >= 2 && segments[0] == "watch" {
            return segments[1].to_string();
        }
    } else if host == "g.nemtudo.me" {
        if segments.len() == 1 {
            return segments[0].to_string();
        }
    }
    trimmed.to_string()
}

/// Valida o handle conforme HANDLE_RE do servidor: ^[a-zA-Z0-9_-]{1,32}$
pub fn is_valid_room_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle.len() <= 32
        && handle
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn is_private_room_handle(handle: &str) -> bool {
    handle.starts_with(PRIVATE_ROOM_PREFIX)
}

/// Gera codigo numerico de 6 digitos para salas privadas (`priv-<nome>-<123456>`).
pub fn generate_private_room_code() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let pid = std::process::id() as u64;
    let mut x = nanos ^ (pid << 17) ^ seq.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    format!("{:06}", x % 1_000_000)
}

pub fn to_private_room_handle(raw_name: &str, code: &str) -> String {
    let trimmed = raw_name.trim();
    let clean = trimmed.strip_prefix(PRIVATE_ROOM_PREFIX).unwrap_or(trimmed);
    format!("{}{}-{}", PRIVATE_ROOM_PREFIX, clean, code)
}

pub fn normalize_private_join_handle(raw_input: &str) -> String {
    let parsed = room_handle_from_input(raw_input);
    if parsed.is_empty() || is_private_room_handle(&parsed) {
        parsed
    } else {
        format!("{}{}", PRIVATE_ROOM_PREFIX, parsed)
    }
}

/// Retorna true para salas efemeras geradas automaticamente (chamadas, temas, chamados) que nao devem ir para Salas recentes.
pub fn is_ephemeral_room_handle(handle: &str) -> bool {
    let Some(rest) = handle.strip_prefix(PRIVATE_ROOM_PREFIX) else {
        return false;
    };
    if let Some(after) = rest.strip_prefix("call-") {
        if let Some((mid, code)) = after.rsplit_once('-') {
            return mid.len() == 10
                && mid.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && code.len() == 6
                && code.bytes().all(|b| b.is_ascii_digit());
        }
    }
    if let Some(after) = rest.strip_prefix("ch-") {
        if let Some((mid, code)) = after.rsplit_once('-') {
            return mid.len() == 8
                && mid.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && code.len() == 6
                && code.bytes().all(|b| b.is_ascii_digit());
        }
    }
    if let Some(code) = rest.strip_prefix("theme-creation-") {
        return code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
    }
    if let Some(code) = rest.strip_prefix("theme-view-") {
        return code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
    }
    false
}

/// Divide "priv-familia-123456" em ("familia", "123456") se seguir o padrao privado do GoLive.
pub fn split_private_room_handle(handle: &str) -> Option<(&str, &str)> {
    let rest = handle.strip_prefix(PRIVATE_ROOM_PREFIX)?;
    let (name, code) = rest.rsplit_once('-')?;
    if !name.is_empty() && code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        Some((name, code))
    } else {
        None
    }
}

pub fn signaling_http_base(ws_url: &str) -> String {
    let base = ws_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/ws").unwrap_or(base);
    if let Some(rest) = base.strip_prefix("wss://") {
        format!("https://{}", rest)
    } else if let Some(rest) = base.strip_prefix("ws://") {
        format!("http://{}", rest)
    } else {
        base.to_string()
    }
}

pub fn ws_url_with_locale(ws_url: &str) -> String {
    if ws_url.contains('?') {
        format!("{}&lang=pt", ws_url)
    } else {
        format!("{}?lang=pt", ws_url)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedTone {
    Blue,
    Gold,
    Ruby,
}

/// Determina o selo verificado (Azul, Dourado ou Rubi) seguindo a tabela oficial (`entitlements.ts`).
pub fn verified_badge_tone(flags: &[String]) -> Option<VerifiedTone> {
    if flags.iter().any(|f| f == "PRO_ULTRA") {
        if flags.iter().any(|f| f == "BADGE_TONE_BLUE") {
            return Some(VerifiedTone::Blue);
        }
        if flags.iter().any(|f| f == "BADGE_TONE_GOLD") {
            return Some(VerifiedTone::Gold);
        }
        return Some(VerifiedTone::Ruby);
    }
    if flags.iter().any(|f| f == "PRO_MAX") {
        if flags.iter().any(|f| f == "BADGE_TONE_BLUE") {
            return Some(VerifiedTone::Blue);
        }
        return Some(VerifiedTone::Gold);
    }
    if flags.iter().any(|f| f == "VERIFIED" || f == "PRO") {
        return Some(VerifiedTone::Blue);
    }
    None
}

/// Resolve cor de nome da loja (`#RRGGBB`, `#RGB`, `glow:#hex`, `linear-gradient(...)`, `shimmer:...`)
/// em `[r, g, b]` sem custo de animacao.
pub fn resolve_name_color_rgb(raw: Option<&str>) -> Option<[u8; 3]> {
    let val = raw?.trim();
    if val.is_empty() {
        return None;
    }
    let candidate = val.strip_prefix("glow:").unwrap_or(val).trim();
    if let Some(hex_pos) = candidate.find('#') {
        let slice = &candidate[hex_pos..];
        let end = slice
            .char_indices()
            .skip(1)
            .take_while(|(_, c)| c.is_ascii_hexdigit())
            .last()
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(0);
        if end >= 4 {
            let token = &slice[..end];
            let fallback = [255, 255, 255];
            let parsed = parse_hex_rgb(Some(token), fallback);
            if token.eq_ignore_ascii_case("#fff")
                || token.eq_ignore_ascii_case("#ffffff")
                || parsed != fallback
            {
                return Some(parsed);
            }
        }
    }
    None
}

/// Formata saldo de pontos com separador de milhar brasileiro (`6300` -> `"6.300"`).
pub fn format_points_br(points: u64) -> String {
    let s = points.to_string();
    let len = s.len();
    let mut out = String::with_capacity(len + len / 3);
    for (idx, ch) in s.chars().enumerate() {
        if idx > 0 && (len - idx) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

pub fn format_duration_pt(total_seconds: u64) -> String {
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    if h > 0 {
        format!("{}h {:02}min", h, m)
    } else if m > 0 {
        format!("{}min {:02}s", m, s)
    } else {
        format!("{}s", s)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoomAdmin {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoomBan {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLink {
    pub network: String,
    pub url: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub auto_name: Option<String>,
}

pub const ROOM_PERMISSION_ROWS: &[(&str, &str)] = &[
    ("mic", "Permitir que todos liguem o microfone"),
    ("screen", "Permitir que todos compartilhem sua tela"),
    ("camera", "Permitir que todos liguem sua camera"),
    ("videoSource", "Permitir que todos adicionem uma fonte de video"),
    ("chat", "Permitir que todos enviem mensagens no chat"),
    ("gif", "Permitir que todos enviem GIFS"),
    ("image", "Permitir que todos enviem imagens"),
    ("theme", "Permitir que Pro Max troquem o tema da sala"),
    ("tools", "Permitir que todos abram e fechem ferramentas"),
    ("music", "Permitir que todos coloquem musica"),
    ("recording", "Permitir que todos gravem a chamada"),
    ("transcript", "Permitir que todos usem transcricao e traducao ao vivo"),
    ("clips", "Permitir que todos criem clipes"),
];

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct RoomPermissions {
    pub mic: bool,
    pub screen: bool,
    pub camera: bool,
    pub video_source: bool,
    pub chat: bool,
    pub gif: bool,
    pub image: bool,
    pub theme: bool,
    pub tools: bool,
    pub music: bool,
    pub recording: bool,
    pub transcript: bool,
    pub clips: bool,
}

impl Default for RoomPermissions {
    fn default() -> Self {
        Self::all_allowed()
    }
}

impl RoomPermissions {
    pub const fn all_allowed() -> Self {
        Self {
            mic: true,
            screen: true,
            camera: true,
            video_source: true,
            chat: true,
            gif: true,
            image: true,
            theme: true,
            tools: true,
            music: true,
            recording: true,
            transcript: true,
            clips: true,
        }
    }

    pub fn from_json(val: Option<&Value>) -> Self {
        match val {
            Some(v) if v.is_object() => {
                serde_json::from_value::<RoomPermissions>(v.clone()).unwrap_or_default()
            }
            _ => Self::all_allowed(),
        }
    }

    pub fn get(&self, key: &str) -> bool {
        match key {
            "mic" => self.mic,
            "screen" => self.screen,
            "camera" => self.camera,
            "videoSource" => self.video_source,
            "chat" => self.chat,
            "gif" => self.gif,
            "image" => self.image,
            "theme" => self.theme,
            "tools" => self.tools,
            "music" => self.music,
            "recording" => self.recording,
            "transcript" => self.transcript,
            "clips" => self.clips,
            _ => true,
        }
    }
}

pub const ROOM_CATEGORIES: &[(&str, &str)] = &[
    ("gameplay", "Gameplay"),
    ("conversa", "Conversa"),
    ("musica", "Musica"),
    ("filmes", "Filmes e Series"),
    ("estudos", "Estudos"),
    ("trabalho", "Trabalho"),
    ("esportes", "Esportes"),
    ("programacao", "Programacao"),
    ("arte", "Arte e Desenho"),
    ("outros", "Outros"),
];

pub fn room_category_label(id: Option<&str>) -> Option<&'static str> {
    let id = id?.trim();
    ROOM_CATEGORIES
        .iter()
        .find(|(cid, _)| *cid == id)
        .map(|(_, lbl)| *lbl)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanTier {
    Free,
    Account,
    Premium,
    PremiumMax,
    ProUltra,
}

impl PlanTier {
    pub fn from_flags(flags: &[String], has_account: bool) -> Self {
        if flags.iter().any(|f| f == "PRO_ULTRA") {
            PlanTier::ProUltra
        } else if flags.iter().any(|f| f == "PRO_MAX") {
            PlanTier::PremiumMax
        } else if flags.iter().any(|f| f == "PRO") {
            PlanTier::Premium
        } else if has_account {
            PlanTier::Account
        } else {
            PlanTier::Free
        }
    }

    pub fn is_pro(self) -> bool {
        matches!(
            self,
            PlanTier::Premium | PlanTier::PremiumMax | PlanTier::ProUltra
        )
    }

    /// Quantidade maxima de telas simultaneas conforme regras oficiais do GoLive (`multiScreen.ts`).
    pub fn max_screens(self) -> usize {
        match self {
            PlanTier::Free | PlanTier::Account | PlanTier::Premium => 2,
            PlanTier::PremiumMax => 5,
            PlanTier::ProUltra => 10,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlanTier::Free => "Gratuito",
            PlanTier::Account => "Conta",
            PlanTier::Premium => "Pro",
            PlanTier::PremiumMax => "Pro Max",
            PlanTier::ProUltra => "Pro Ultra",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShareResolution {
    #[serde(rename = "576p")]
    P576,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "2160p")]
    P2160,
}

impl ShareResolution {
    pub const ALL: [ShareResolution; 5] = [
        Self::P576,
        Self::P720,
        Self::P1080,
        Self::P1440,
        Self::P2160,
    ];

    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::P576 => (1024, 576),
            Self::P720 => (1280, 720),
            Self::P1080 => (1920, 1080),
            Self::P1440 => (2560, 1440),
            Self::P2160 => (3840, 2160),
        }
    }

    pub fn requires_pro(self) -> bool {
        matches!(self, Self::P1440 | Self::P2160)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::P576 => "576p",
            Self::P720 => "720p (Padrao)",
            Self::P1080 => "1080p Full HD",
            Self::P1440 => "1440p 2K (Pro)",
            Self::P2160 => "2160p 4K (Pro)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShareFps {
    #[serde(rename = "15")]
    Fps15 = 15,
    #[serde(rename = "24")]
    Fps24 = 24,
    #[serde(rename = "30")]
    Fps30 = 30,
    #[serde(rename = "60")]
    Fps60 = 60,
    #[serde(rename = "120")]
    Fps120 = 120,
    #[serde(rename = "240")]
    Fps240 = 240,
}

impl ShareFps {
    pub const ALL: [ShareFps; 6] = [
        Self::Fps15,
        Self::Fps24,
        Self::Fps30,
        Self::Fps60,
        Self::Fps120,
        Self::Fps240,
    ];

    pub fn as_u32(self) -> u32 {
        self as u32
    }

    pub fn requires_pro(self) -> bool {
        matches!(self, Self::Fps120 | Self::Fps240)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fps15 => "15 fps",
            Self::Fps24 => "24 fps",
            Self::Fps30 => "30 fps (Padrao)",
            Self::Fps60 => "60 fps",
            Self::Fps120 => "120 fps (Pro)",
            Self::Fps240 => "240 fps (Pro)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareBitrate {
    Low,
    Medium,
    High,
    Ultra,
    Maximo,
    Extremo,
}

impl ShareBitrate {
    pub const ALL: [ShareBitrate; 6] = [
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Ultra,
        Self::Maximo,
        Self::Extremo,
    ];

    pub fn kbps(self) -> u32 {
        match self {
            Self::Low => 700,
            Self::Medium => 2000,
            Self::High => 6000,
            Self::Ultra => 8000,
            Self::Maximo => 16000,
            Self::Extremo => 32000,
        }
    }

    pub fn requires_pro(self) -> bool {
        matches!(self, Self::Maximo | Self::Extremo)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Baixo (700 kbps)",
            Self::Medium => "Medio (2 Mbps)",
            Self::High => "Alto (6 Mbps)",
            Self::Ultra => "Ultra (8 Mbps)",
            Self::Maximo => "Maximo (16 Mbps - Pro)",
            Self::Extremo => "Extremo (32 Mbps - Pro)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DegradationMode {
    Text,
    Balanced,
    Motion,
}

impl DegradationMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Texto / Codigo",
            Self::Balanced => "Equilibrado",
            Self::Motion => "Video / Jogo",
        }
    }
}

/// Slots suportados de tela (1..=10)
pub const SCREEN_CHANNELS: [&str; 10] = [
    "screen", "screen2", "screen3", "screen4", "screen5", "screen6", "screen7", "screen8",
    "screen9", "screen10",
];

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SharedFile {
    pub channel: String,
    pub name: String,
    pub mode: String, // "video" | "music"
    pub control_mode: String,
    pub playing: bool,
    pub position_seconds: f64,
    pub duration: f64,
    pub index: usize,
    pub count: usize,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub device: Option<u32>,
    #[serde(default)]
    pub sharing: bool,
    #[serde(default)]
    pub screen: Option<bool>,
    #[serde(default)]
    pub camera: Option<bool>,
    #[serde(default)]
    pub phone_camera: Option<bool>,
    #[serde(default)]
    pub files: Vec<SharedFile>,
    #[serde(default)]
    pub ad_paused: bool,
    #[serde(default)]
    pub mic: bool,
    #[serde(default)]
    pub mics_muted: bool,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub screen_of: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub is_guest: Option<bool>,
    #[serde(default)]
    pub flags: Option<Vec<String>>,
    #[serde(default)]
    pub bot: Option<bool>,
    #[serde(default)]
    pub app: Option<bool>,
    #[serde(default)]
    pub mobile_app: Option<bool>,
    /// Conectado pelo Go Live - Lite (a API manda junto com `app`).
    #[serde(default, rename = "liteApp")]
    pub lite_app: Option<bool>,
    #[serde(default)]
    pub name_color: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default, rename = "desktopAppOS")]
    pub desktop_app_os: Option<String>,
    #[serde(default, rename = "mobileAppOS")]
    pub mobile_app_os: Option<String>,
    #[serde(default)]
    pub presence: Option<String>,
}

impl PeerInfo {
    pub fn is_obs(&self) -> bool {
        self.role.as_deref() == Some("obs")
    }

    pub fn is_silent_moderator(&self) -> bool {
        self.role.as_deref() == Some("moderator")
    }

    pub fn volume_key(&self) -> &str {
        self.user_id.as_deref().unwrap_or(&self.id)
    }

    pub fn display_label(&self) -> String {
        if self.is_guest == Some(true) {
            format!("{} (guest)", self.name)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatReplyTo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub from: String,
    pub name: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub device: Option<u32>,
    #[serde(default)]
    pub is_guest: Option<bool>,
    #[serde(default)]
    pub flags: Option<Vec<String>>,
    #[serde(default)]
    pub name_color: Option<String>,
    #[serde(default)]
    pub bot: Option<bool>,
    #[serde(default)]
    pub webhook: Option<bool>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub images: Option<Vec<String>>,
    #[serde(default)]
    pub reply_to: Option<ChatReplyTo>,
    #[serde(default)]
    pub ts: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoSource {
    pub id: String,
    pub kind: String, // "youtube" | "twitch" | "kick"
    pub video_id: String,
    #[serde(default)]
    pub playlist_id: Option<String>,
    #[serde(default)]
    pub added_by_id: String,
    #[serde(default)]
    pub added_by_name: String,
    #[serde(default)]
    pub control_mode: String,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub position_seconds: f64,
    #[serde(default)]
    pub playback_rate: f64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicSource {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub video_id: Option<String>,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub position_seconds: f64,
    #[serde(default)]
    pub playback_rate: f64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredAccount {
    pub username: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub bot: bool,
}

fn deserialize_ice_urls<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    Ok(match Option::<OneOrMany>::deserialize(deserializer)? {
        Some(OneOrMany::One(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s]
            }
        }
        Some(OneOrMany::Many(v)) => v.into_iter().filter(|s| !s.trim().is_empty()).collect(),
        None => Vec::new(),
    })
}

/// Validates external links before launching the system browser.
pub fn is_safe_http_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("https://") || trimmed.starts_with("http://")
}

/// Opens a validated `http(s)://` URL in the user's default browser without spawning cmd.exe.
pub fn open_external_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !is_safe_http_url(trimmed) {
        return Err("URL externa invalida".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(trimmed)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("falha ao abrir navegador: {e}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(trimmed)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("falha ao abrir navegador: {e}"))
    }
}

/// Abre um arquivo local (gerado pelo proprio app) com o programa padrao do Windows.
/// Usado so para a pagina de impressao do documento; nao e um site externo.
pub fn open_local_file(path: &std::path::Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("arquivo nao encontrado".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("falha ao abrir arquivo: {e}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("falha ao abrir arquivo: {e}"))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SiteStatusButton {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub href: String,
    #[serde(default)]
    pub target: Option<String>,
}

/// Status banner fetched from `https://bin.nemtudo.me/raw/golive-status`.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct SiteStatus {
    #[serde(default, alias = "api_error", alias = "apiError")]
    pub api_error: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub button: Option<SiteStatusButton>,
}

impl SiteStatus {
    pub fn should_display(&self) -> bool {
        self.api_error && !self.message.trim().is_empty()
    }
}

fn deserialize_announcement_sound<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SoundVal {
        Str(String),
        Bool(bool),
    }

    match Option::<SoundVal>::deserialize(deserializer)? {
        Some(SoundVal::Str(s)) => Ok(s),
        Some(SoundVal::Bool(true)) => Ok("always".to_string()),
        _ => Ok("off".to_string()),
    }
}

fn deserialize_opt_u64_or_string<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u64),
        Str(String),
    }

    Ok(match Option::<NumOrStr>::deserialize(deserializer)? {
        Some(NumOrStr::Num(n)) => Some(n),
        Some(NumOrStr::Str(s)) => s.trim().parse::<u64>().ok(),
        None => None,
    })
}

/// Live top announcement pushed over WebSocket `{ "type": "announcement", ... }`.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub has_button: bool,
    #[serde(default)]
    pub button_label: String,
    #[serde(default)]
    pub button_action: String,
    #[serde(default)]
    pub button_url: Option<String>,
    #[serde(default)]
    pub color: String,
    #[serde(default = "default_true")]
    pub dismissible: bool,
    #[serde(default)]
    pub visibility: String,
    #[serde(default, deserialize_with = "deserialize_announcement_sound")]
    pub sound: String,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub devices: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

impl Announcement {
    pub fn button_url_str(&self) -> &str {
        self.button_url.as_deref().unwrap_or("")
    }

    /// Aviso para o Lite: lista vazia (todos) ou com `desktop-lite` marcado no admin.
    pub fn targets_desktop_app(&self) -> bool {
        match &self.devices {
            None => true,
            Some(list) if list.is_empty() => true,
            Some(list) => list.iter().any(|d| d.trim().eq_ignore_ascii_case("desktop-lite")),
        }
    }

    pub fn is_valid_banner(&self) -> bool {
        !self.id.trim().is_empty() && !self.text.trim().is_empty() && self.targets_desktop_app()
    }

    /// Official preset banner colors (`(bg_rgb, fg_rgb)`).
    pub fn color_preset_rgb(&self) -> ([u8; 3], [u8; 3]) {
        match self.color.trim().to_ascii_lowercase().as_str() {
            "red" => ([127, 29, 29], [255, 255, 255]),
            "blue" => ([30, 58, 138], [255, 255, 255]),
            _ => ([6, 95, 70], [255, 255, 255]),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartnerSchedule {
    #[serde(default)]
    pub days: Option<Vec<u8>>,
    #[serde(default)]
    pub start_minute: Option<u16>,
    #[serde(default)]
    pub end_minute: Option<u16>,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub button_label: Option<String>,
    #[serde(default)]
    pub button_url: Option<String>,
    #[serde(default)]
    pub background_color: Option<String>,
    #[serde(default)]
    pub text_color: Option<String>,
    #[serde(default)]
    pub button_background_color: Option<String>,
    #[serde(default)]
    pub button_text_color: Option<String>,
}

/// Sidebar partner card payload (`GET /partner` or WS `{ "type": "partner" }`).
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartnerCardData {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub button_label: Option<String>,
    #[serde(default)]
    pub button_url: Option<String>,
    #[serde(default)]
    pub background_color: Option<String>,
    #[serde(default)]
    pub text_color: Option<String>,
    #[serde(default)]
    pub button_background_color: Option<String>,
    #[serde(default)]
    pub button_text_color: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_or_string")]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub reward_video_url: Option<String>,
    #[serde(default)]
    pub reward_points: Option<u64>,
    #[serde(default)]
    pub click_reward_points: Option<u64>,
    #[serde(default)]
    pub schedules: Option<Vec<PartnerSchedule>>,
}

impl PartnerCardData {
    pub fn fallback_house_ad() -> Self {
        Self {
            id: "golive-house-ad".to_string(),
            title: "Anuncie aqui!".to_string(),
            description: "Esta pagina e vista por mais de 60 mil pessoas diariamente! Coloque sua marca aqui!".to_string(),
            image_url: None,
            icon_url: None,
            button_label: Some("Entrar em contato".to_string()),
            button_url: Some("https://go.nemtudo.me/golive-partner-nemtudodiscord".to_string()),
            background_color: Some("#ffffff".to_string()),
            text_color: Some("#111827".to_string()),
            button_background_color: Some("#4f75ff".to_string()),
            button_text_color: Some("#ffffff".to_string()),
            expires_at: None,
            reward_video_url: None,
            reward_points: None,
            click_reward_points: None,
            schedules: None,
        }
    }

    pub fn is_house_ad(&self) -> bool {
        self.id.is_empty() || self.id == "golive-house-ad"
    }

    /// Resolves daypart schedule overrides against `(weekday_sun0 0..=6, minute_of_day 0..1440)`.
    pub fn resolve_for_local_minute(&self, weekday_sun0: u8, minute_of_day: u16) -> Self {
        let mut resolved = self.clone();
        let Some(schedules) = &self.schedules else {
            return resolved;
        };

        for sched in schedules {
            if let Some(days) = &sched.days {
                if !days.is_empty() && !days.contains(&weekday_sun0) {
                    continue;
                }
            }
            if !schedule_matches_minute(sched, minute_of_day) {
                continue;
            }
            if let Some(v) = sched.title.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.title = v.clone();
            }
            if let Some(v) = sched.description.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.description = v.clone();
            }
            if sched.image_url.is_some() {
                resolved.image_url = sched.image_url.clone();
            }
            if sched.icon_url.is_some() {
                resolved.icon_url = sched.icon_url.clone();
            }
            if let Some(v) = sched.button_label.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.button_label = Some(v.clone());
            }
            if let Some(v) = sched.button_url.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.button_url = Some(v.clone());
            }
            if let Some(v) = sched.background_color.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.background_color = Some(v.clone());
            }
            if let Some(v) = sched.text_color.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.text_color = Some(v.clone());
            }
            if let Some(v) = sched
                .button_background_color
                .as_ref()
                .filter(|s| !s.trim().is_empty())
            {
                resolved.button_background_color = Some(v.clone());
            }
            if let Some(v) = sched.button_text_color.as_ref().filter(|s| !s.trim().is_empty()) {
                resolved.button_text_color = Some(v.clone());
            }
            break;
        }

        resolved
    }
}

fn parse_hhmm(value: Option<&str>) -> Option<u16> {
    let raw = value?.trim();
    let (h, m) = raw.split_once(':')?;
    let hh: u16 = h.trim().parse().ok()?;
    let mm: u16 = m.trim().parse().ok()?;
    if hh <= 23 && mm <= 59 {
        Some(hh * 60 + mm)
    } else {
        None
    }
}

fn schedule_matches_minute(sched: &PartnerSchedule, minute_of_day: u16) -> bool {
    let start_opt = sched.start_minute.or_else(|| parse_hhmm(sched.start_time.as_deref()));
    let end_opt = sched.end_minute.or_else(|| parse_hhmm(sched.end_time.as_deref()));
    match (start_opt, end_opt) {
        (Some(start), Some(end)) => {
            if start <= end {
                minute_of_day >= start && minute_of_day <= end
            } else {
                // Overnight window (e.g. 22:00 -> 04:00)
                minute_of_day >= start || minute_of_day <= end
            }
        }
        (Some(start), None) => minute_of_day >= start,
        (None, Some(end)) => minute_of_day <= end,
        (None, None) => true,
    }
}

/// Parses `#RRGGBB` or `#RGB` into `[r, g, b]`, returning `fallback` on invalid values.
pub fn parse_hex_rgb(value: Option<&str>, fallback: [u8; 3]) -> [u8; 3] {
    let Some(raw) = value.map(str::trim) else {
        return fallback;
    };
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return [r, g, b];
        }
    } else if hex.len() == 3 {
        let mut out = [0u8; 3];
        let mut ok = true;
        for (i, ch) in hex.chars().enumerate() {
            if let Some(d) = ch.to_digit(16) {
                out[i] = (d * 17) as u8;
            } else {
                ok = false;
                break;
            }
        }
        if ok {
            return out;
        }
    }
    fallback
}

#[cfg(test)]
mod meta_feed_tests {
    use super::*;

    #[test]
    fn test_announcement_device_filtering_and_color_presets() {
        let mut ann = Announcement {
            id: "a1".into(),
            text: "Atualizacao".into(),
            color: "green".into(),
            devices: None,
            ..Default::default()
        };
        assert!(ann.is_valid_banner());
        assert_eq!(ann.color_preset_rgb().0, [6, 95, 70]);

        ann.devices = Some(vec!["web".into()]);
        assert!(!ann.targets_desktop_app());

        ann.devices = Some(vec!["desktop-app".into()]);
        assert!(!ann.targets_desktop_app(), "aviso so do Electron nao aparece no Lite");
        ann.devices = Some(vec!["desktop-lite".into()]);
        ann.color = "red".into();
        assert!(ann.targets_desktop_app());
        assert_eq!(ann.color_preset_rgb().0, [127, 29, 29]);

        ann.color = "blue".into();
        assert_eq!(ann.color_preset_rgb().0, [30, 58, 138]);
    }

    #[test]
    fn test_partner_schedule_resolution_applies_override_when_in_window() {
        let partner = PartnerCardData {
            id: "p1".into(),
            title: "Base".into(),
            description: "Desc".into(),
            schedules: Some(vec![PartnerSchedule {
                days: Some(vec![1, 2]),
                start_time: Some("18:00".into()),
                end_time: Some("23:00".into()),
                title: Some("Prime Time".into()),
                button_label: Some("Entrar".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };

        let outside = partner.resolve_for_local_minute(1, 12 * 60);
        assert_eq!(outside.title, "Base");

        let inside = partner.resolve_for_local_minute(2, 19 * 60 + 30);
        assert_eq!(inside.title, "Prime Time");
        assert_eq!(inside.button_label.as_deref(), Some("Entrar"));
    }

    #[test]
    fn test_is_safe_http_url_rejects_dangerous_schemes() {
        assert!(is_safe_http_url("https://golive.nemtudo.me"));
        assert!(is_safe_http_url("http://localhost:3000"));
        assert!(!is_safe_http_url("javascript:alert(1)"));
        assert!(!is_safe_http_url("file:///C:/Windows/System32/cmd.exe"));
        assert!(!is_safe_http_url("cmd://calc"));
    }

    #[test]
    fn test_live_announcement_json_with_string_sound_and_null_url() {
        let raw = serde_json::json!({
            "id": "warn-1",
            "version": 3,
            "text": "Novas atualizacoes disponiveis",
            "hasButton": true,
            "buttonLabel": "Ver mudancas",
            "buttonAction": "reload",
            "buttonUrl": null,
            "color": "green",
            "dismissible": true,
            "visibility": "all",
            "sound": "live-only",
            "persistent": false,
            "devices": ["desktop-lite", "desktop-browser"]
        });
        let ann: Announcement = serde_json::from_value(raw).expect("announcement json");
        assert!(ann.is_valid_banner());
        assert_eq!(ann.sound, "live-only");
        assert_eq!(ann.button_url_str(), "");
    }

    #[test]
    fn test_verified_badge_tone_and_name_color_and_points_format() {
        assert_eq!(verified_badge_tone(&["PRO".into()]), Some(VerifiedTone::Blue));
        assert_eq!(verified_badge_tone(&["VERIFIED".into()]), Some(VerifiedTone::Blue));
        assert_eq!(verified_badge_tone(&["PRO_MAX".into(), "PRO".into()]), Some(VerifiedTone::Gold));
        assert_eq!(
            verified_badge_tone(&["PRO_MAX".into(), "BADGE_TONE_BLUE".into()]),
            Some(VerifiedTone::Blue)
        );
        assert_eq!(
            verified_badge_tone(&["PRO_ULTRA".into(), "PRO_MAX".into()]),
            Some(VerifiedTone::Ruby)
        );
        assert_eq!(
            verified_badge_tone(&["PRO_ULTRA".into(), "BADGE_TONE_GOLD".into()]),
            Some(VerifiedTone::Gold)
        );
        assert_eq!(verified_badge_tone(&[]), None);

        assert_eq!(resolve_name_color_rgb(Some("#e879f9")), Some([232, 121, 249]));
        assert_eq!(resolve_name_color_rgb(Some("glow:#22d3ee")), Some([34, 211, 238]));
        assert_eq!(
            resolve_name_color_rgb(Some("shimmer:linear-gradient(90deg, #f43f5e, #fbbf24)")),
            Some([244, 63, 94])
        );
        assert_eq!(resolve_name_color_rgb(None), None);

        assert_eq!(format_points_br(0), "0");
        assert_eq!(format_points_br(950), "950");
        assert_eq!(format_points_br(6300), "6.300");
        assert_eq!(format_points_br(1250000), "1.250.000");

        let p = PartnerCardData {
            id: "p2".into(),
            title: "Default".into(),
            schedules: Some(vec![PartnerSchedule {
                start_minute: Some(600),
                end_minute: Some(720),
                title: Some("Window Match".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(p.resolve_for_local_minute(1, 630).title, "Window Match");
        assert_eq!(p.resolve_for_local_minute(1, 800).title, "Default");
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IceServerConfig {
    #[serde(default, deserialize_with = "deserialize_ice_urls")]
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
}

/// Estrutura dos sinais WebRTC dentro de `{ "type": "signal", "to"/"from": ..., "data": ... }`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SignalData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>, // "broadcaster" | "viewer"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // "offer" | "answer" | "ice" | "stop" | "resume" | "quality" | "reconnect-request" | "diag-request" | "diag"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ice_restart: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diag: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_room_handle_extraction() {
        assert_eq!(room_handle_from_input("minhasala"), "minhasala");
        assert_eq!(
            room_handle_from_input("https://golive.nemtudo.me/watch/amigos"),
            "amigos"
        );
        assert_eq!(
            room_handle_from_input("g.nemtudo.me/priv-teste-123456"),
            "priv-teste-123456"
        );
        assert!(is_valid_room_handle("priv-teste-123456"));
        assert!(!is_valid_room_handle("sala invalida!"));
    }

    #[test]
    fn test_private_room_split() {
        assert_eq!(
            split_private_room_handle("priv-familia-987654"),
            Some(("familia", "987654"))
        );
        assert_eq!(split_private_room_handle("publica"), None);
    }

    #[test]
    fn test_plan_tier_limits() {
        assert_eq!(PlanTier::Free.max_screens(), 2);
        assert_eq!(PlanTier::Account.max_screens(), 2);
        assert!(!PlanTier::Free.is_pro());
        let pro = PlanTier::from_flags(&["PRO".to_string()], true);
        assert!(pro.is_pro());
        assert_eq!(pro.max_screens(), 2);
        let pro_max = PlanTier::from_flags(&["PRO_MAX".to_string()], true);
        assert_eq!(pro_max.max_screens(), 5);
        let ultra = PlanTier::from_flags(&["PRO_ULTRA".to_string()], true);
        assert_eq!(ultra.max_screens(), 10);

        // Gates oficiais (Free: ate 1080p, 60fps, Ultra 8Mbps; Pro: 2K/4K, 120/240fps, Maximo/Extremo)
        assert!(!ShareResolution::P576.requires_pro());
        assert!(!ShareResolution::P720.requires_pro());
        assert!(!ShareResolution::P1080.requires_pro());
        assert!(ShareResolution::P1440.requires_pro());
        assert!(ShareResolution::P2160.requires_pro());

        assert!(!ShareFps::Fps15.requires_pro());
        assert!(!ShareFps::Fps30.requires_pro());
        assert!(!ShareFps::Fps60.requires_pro());
        assert!(ShareFps::Fps120.requires_pro());
        assert!(ShareFps::Fps240.requires_pro());

        assert!(!ShareBitrate::Low.requires_pro());
        assert!(!ShareBitrate::High.requires_pro());
        assert!(!ShareBitrate::Ultra.requires_pro());
        assert!(ShareBitrate::Maximo.requires_pro());
        assert!(ShareBitrate::Extremo.requires_pro());
    }

    #[test]
    fn test_signal_data_ice_restart_serialization() {
        let normal = SignalData {
            channel: Some("screen".to_string()),
            role: Some("broadcaster".to_string()),
            kind: Some("offer".to_string()),
            ..Default::default()
        };
        let normal_json = serde_json::to_value(&normal).unwrap();
        assert!(normal_json.get("iceRestart").is_none());

        let restart = SignalData {
            channel: Some("screen".to_string()),
            role: Some("broadcaster".to_string()),
            kind: Some("offer".to_string()),
            ice_restart: Some(true),
            ..Default::default()
        };
        let restart_json = serde_json::to_value(&restart).unwrap();
        assert_eq!(restart_json.get("iceRestart"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_ice_server_config_deserializes_string_or_array() {
        let single: IceServerConfig = serde_json::from_value(serde_json::json!({
            "urls": "turn:turn.cloudflare.com:3478?transport=udp",
            "username": "u",
            "credential": "c"
        }))
        .expect("single url string");
        assert_eq!(
            single.urls,
            vec!["turn:turn.cloudflare.com:3478?transport=udp".to_string()]
        );

        let many: IceServerConfig = serde_json::from_value(serde_json::json!({
            "urls": ["stun:a:3478", "  ", "turn:b:3478?transport=udp"],
            "username": "u2",
            "credential": "c2"
        }))
        .expect("array of urls");
        assert_eq!(
            many.urls,
            vec![
                "stun:a:3478".to_string(),
                "turn:b:3478?transport=udp".to_string()
            ]
        );
    }

    #[test]
    fn test_room_permissions_parsing_and_duration_format() {
        let perms = RoomPermissions::from_json(Some(&serde_json::json!({
            "mic": false,
            "tools": false,
            "clips": false
        })));
        assert!(!perms.get("mic"));
        assert!(!perms.get("tools"));
        assert!(!perms.get("clips"));
        assert!(perms.get("screen"));
        assert!(perms.get("chat"));

        assert_eq!(format_duration_pt(42), "42s");
        assert_eq!(format_duration_pt(185), "3min 05s");
        assert_eq!(format_duration_pt(3725), "1h 02min");
    }

    #[test]
    fn test_private_room_code_and_handle_helpers() {
        let c1 = generate_private_room_code();
        let c2 = generate_private_room_code();
        assert_eq!(c1.len(), 6);
        assert!(c1.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(c2.len(), 6);
        assert!(c2.bytes().all(|b| b.is_ascii_digit()));
        assert_ne!(c1, c2);

        let priv_h = to_private_room_handle("familia", "123456");
        assert_eq!(priv_h, "priv-familia-123456");
        assert_eq!(
            split_private_room_handle(&priv_h),
            Some(("familia", "123456"))
        );

        assert_eq!(
            normalize_private_join_handle("familia-123456"),
            "priv-familia-123456"
        );
        assert_eq!(
            normalize_private_join_handle("priv-familia-123456"),
            "priv-familia-123456"
        );
        assert_eq!(
            normalize_private_join_handle("https://golive.nemtudo.me/watch/priv-teste-596978"),
            "priv-teste-596978"
        );

        assert!(is_ephemeral_room_handle("priv-call-abcd123456-123456"));
        assert!(is_ephemeral_room_handle("priv-ch-a1b2c3d4-654321"));
        assert!(is_ephemeral_room_handle("priv-theme-creation-111222"));
        assert!(!is_ephemeral_room_handle("priv-call-me-maybe-123456"));
        assert!(!is_ephemeral_room_handle("priv-familia-123456"));
    }
}
