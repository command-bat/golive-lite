use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::net::protocol::{DegradationMode, ShareBitrate, ShareFps, ShareResolution};

pub const APP_DISPLAY_NAME: &str = "Go Live - Lite";
pub const INTERNAL_APP_DIR_NAME: &str = "Go Live Lite - Desktop";
const LEGACY_APP_DIR_NAME: &str = "GoLiveLite";

pub const MOD_FERRAMENTAS: &str = "ferramentas";
pub const MOD_PERMISSOES_CHAMADA: &str = "permissoes_chamada";
pub const MOD_PERFIL: &str = "perfil";
pub const MOD_AMIGOS: &str = "amigos";

pub const PRODUCTION_SIGNALING_URL: &str = "wss://apigolive.nemtudo.me/ws";

/// Resolve a URL do servidor de sinalizacao definida pelo programa (equivalente ao ELECTRON GOLIVE_APP_URL).
pub fn signaling_ws_url() -> String {
    std::env::var("GOLIVE_SIGNALING_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| PRODUCTION_SIGNALING_URL.to_string())
}

/// Configuracoes locais persistidas do GoLive Lite (em JSON minimo, sem SQLite).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub display_name: String,
    pub last_room: String,
    #[serde(default)]
    pub recent_rooms: Vec<String>,
    pub account_token: Option<String>,
    pub guest_token: Option<String>,
    pub client_id: String,
    pub install_id: String,
    pub device_id: String,

    // Audio
    pub mic_enabled_on_join: bool,
    pub mics_muted: bool,
    pub mic_gain: f32,
    pub mic_vad_threshold: f32,
    pub push_to_talk_enabled: bool,
    pub push_to_talk_vk: i32,
    pub mic_device_name: Option<String>,
    pub speaker_device_name: Option<String>,

    /// Volumes individuais por participante (0.0 a 2.0, padrao 1.0), chaveado por userId/peerId.
    pub peer_volumes: HashMap<String, f32>,
    /// Mutes individuais por participante.
    pub peer_muted: HashMap<String, bool>,
    /// Volumes individuais de transmissoes (ex: "screen:<id>", "camera:<id>", "video:<id>").
    pub transmission_volumes: HashMap<String, f32>,
    /// Estado de mute de transmissoes (por padrao transmissoes iniciam mutadas como no GoLive Web).
    pub transmission_muted: HashMap<String, bool>,

    // Qualidade de transmissao (Free vs Pro)
    pub share_resolution: ShareResolution,
    pub share_fps: ShareFps,
    pub share_bitrate: ShareBitrate,
    pub share_profile: DegradationMode,
    pub smart_quality: bool,
    pub share_audio: bool,
    pub grid_video_preview: bool,
    pub skipped_update_version: Option<String>,
    pub start_with_windows: bool,
    pub minimize_to_tray_on_close: bool,
    pub enabled_modifications: Vec<String>,
    pub hidden_announcement_ids: Vec<String>,
    pub persistent_announcement: Option<crate::net::protocol::Announcement>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let seed = simple_random_u64();
        Self {
            display_name: "UsuarioLite".to_string(),
            last_room: String::new(),
            recent_rooms: Vec::new(),
            account_token: None,
            guest_token: None,
            client_id: format!("rust-{:016x}", seed),
            install_id: new_install_id(seed.wrapping_mul(6364136223846793005)),
            device_id: format!("dev-{:016x}", seed.wrapping_add(0x9e3779b97f4a7c15)),

            mic_enabled_on_join: false,
            mics_muted: false,
            mic_gain: 1.0,
            mic_vad_threshold: 0.004,
            push_to_talk_enabled: false,
            push_to_talk_vk: 0x56, // Tecla 'V' por padrao
            mic_device_name: None,
            speaker_device_name: None,

            peer_volumes: HashMap::new(),
            peer_muted: HashMap::new(),
            transmission_volumes: HashMap::new(),
            transmission_muted: HashMap::new(),

            share_resolution: ShareResolution::P720,
            share_fps: ShareFps::Fps30,
            share_bitrate: ShareBitrate::High,
            share_profile: DegradationMode::Balanced,
            smart_quality: true,
            share_audio: true,
            grid_video_preview: false,
            skipped_update_version: None,
            start_with_windows: false,
            minimize_to_tray_on_close: true,
            enabled_modifications: Vec::new(),
            hidden_announcement_ids: Vec::new(),
            persistent_announcement: None,
        }
    }
}

impl AppConfig {
    pub fn has_modification(&self, id: &str) -> bool {
        self.enabled_modifications.iter().any(|m| m == id)
    }

    pub fn record_recent_room(&mut self, handle: &str) {
        let trimmed = handle.trim();
        if trimmed.is_empty() || crate::net::protocol::is_ephemeral_room_handle(trimmed) {
            return;
        }
        self.recent_rooms.retain(|h| h != trimmed);
        self.recent_rooms.insert(0, trimmed.to_string());
        if self.recent_rooms.len() > 6 {
            self.recent_rooms.truncate(6);
        }
    }

    pub fn remove_recent_room(&mut self, handle: &str) {
        let trimmed = handle.trim();
        self.recent_rooms.retain(|h| h != trimmed);
    }

    pub fn hide_announcement_id(&mut self, id: &str) {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return;
        }
        if !self.hidden_announcement_ids.iter().any(|existing| existing == trimmed) {
            self.hidden_announcement_ids.push(trimmed.to_string());
            if self.hidden_announcement_ids.len() > 64 {
                let excess = self.hidden_announcement_ids.len() - 64;
                self.hidden_announcement_ids.drain(0..excess);
            }
        }
        if self
            .persistent_announcement
            .as_ref()
            .is_some_and(|a| a.id == trimmed)
        {
            self.persistent_announcement = None;
        }
    }

    /// Desfaz o "x" de um aviso (botao "Mostrar no topo" da central de avisos).
    pub fn unhide_announcement_id(&mut self, id: &str) {
        let trimmed = id.trim();
        self.hidden_announcement_ids.retain(|existing| existing != trimmed);
    }

    pub fn config_path() -> PathBuf {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = PathBuf::from(appdata).join(INTERNAL_APP_DIR_NAME);
            let _ = fs::create_dir_all(&dir);
            return dir.join("config.json");
        }
        PathBuf::from("golive_lite_config.json")
    }

    fn legacy_config_path() -> Option<PathBuf> {
        std::env::var("APPDATA")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|appdata| PathBuf::from(appdata).join(LEGACY_APP_DIR_NAME).join("config.json"))
    }

    pub fn parse_modifications_manifest(raw: &str) -> Vec<String> {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
            return Vec::new();
        };
        val.get("installed")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let bytes = fs::read(&path).ok().or_else(|| {
            Self::legacy_config_path().and_then(|legacy| fs::read(legacy).ok())
        });
        let mut cfg = if let Some(bytes) = bytes {
            if let Ok(mut parsed) = serde_json::from_slice::<AppConfig>(&bytes) {
                if parsed.client_id.is_empty() {
                    parsed.client_id = format!("rust-{:016x}", simple_random_u64());
                }
                // A API so conta ids hex (16-64): o antigo "inst-<16 hex>" vira so o hex, assim a
                // instalacao continua sendo a mesma nas metricas.
                if let Some(hex) = parsed.install_id.strip_prefix("inst-") {
                    parsed.install_id = hex.to_string();
                }
                if !is_valid_install_id(&parsed.install_id) {
                    parsed.install_id = new_install_id(simple_random_u64());
                }
                if parsed.device_id.is_empty() {
                    parsed.device_id = format!("dev-{:016x}", simple_random_u64());
                }
                parsed.mic_gain = parsed.mic_gain.clamp(0.01, 2.0);
                if parsed.mic_vad_threshold > 0.01 {
                    parsed.mic_vad_threshold = 0.004;
                }
                parsed
            } else {
                Self::default()
            }
        } else {
            Self::default()
        };

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let mods_path = dir.join("modifications.json");
                if let Ok(raw) = fs::read_to_string(mods_path) {
                    cfg.enabled_modifications = Self::parse_modifications_manifest(&raw);
                }
            }
        }

        cfg
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn get_peer_volume(&self, key: &str) -> f32 {
        self.peer_volumes.get(key).copied().unwrap_or(1.0).clamp(0.0, 2.0)
    }

    pub fn set_peer_volume(&mut self, key: &str, vol: f32) {
        let clamped = vol.clamp(0.0, 2.0);
        if self.peer_volumes.len() >= 100 && !self.peer_volumes.contains_key(key) {
            if let Some(first) = self.peer_volumes.keys().next().cloned() {
                self.peer_volumes.remove(&first);
            }
        }
        self.peer_volumes.insert(key.to_string(), clamped);
    }

    pub fn is_peer_muted(&self, key: &str) -> bool {
        self.peer_muted.get(key).copied().unwrap_or(false)
    }

    pub fn set_peer_muted(&mut self, key: &str, muted: bool) {
        if muted {
            self.peer_muted.insert(key.to_string(), true);
        } else {
            self.peer_muted.remove(key);
        }
    }

    pub fn get_transmission_volume(&self, key: &str) -> f32 {
        self.transmission_volumes
            .get(key)
            .copied()
            .unwrap_or(1.0)
            .clamp(0.0, 2.0)
    }

    pub fn set_transmission_volume(&mut self, key: &str, vol: f32) {
        self.transmission_volumes
            .insert(key.to_string(), vol.clamp(0.0, 2.0));
    }

    pub fn is_transmission_muted(&self, key: &str) -> bool {
        self.transmission_muted.get(key).copied().unwrap_or(true)
    }

    pub fn set_transmission_muted(&mut self, key: &str, muted: bool) {
        self.transmission_muted.insert(key.to_string(), muted);
    }
}

pub fn simple_random_u64() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678_9abc_def0);
    let pid = std::process::id() as u64;
    let mut x = now ^ (pid << 32) ^ 0x517c_c1b7_2722_0a95;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

pub fn vk_name(vk: i32) -> &'static str {
    match vk {
        0x56 => "V",
        0x42 => "B",
        0x43 => "C",
        0x46 => "F",
        0x47 => "G",
        0x54 => "T",
        0x5A => "Z",
        0x58 => "X",
        0x12 => "Alt",
        0x11 => "Ctrl",
        0x10 => "Shift",
        0x20 => "Espaco",
        0x05 => "Mouse X1",
        0x06 => "Mouse X2",
        _ => "Tecla",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signaling_ws_url_defaults_to_production() {
        if std::env::var_os("GOLIVE_SIGNALING_URL").is_none() {
            assert_eq!(signaling_ws_url(), PRODUCTION_SIGNALING_URL);
        }
    }

    #[test]
    fn test_modifications_manifest_parsing_and_paths() {
        assert_eq!(APP_DISPLAY_NAME, "Go Live - Lite");
        assert_eq!(INTERNAL_APP_DIR_NAME, "Go Live Lite - Desktop");
        let parsed = AppConfig::parse_modifications_manifest(
            r#"{"schemaVersion":1,"installed":["ferramentas","perfil"]}"#,
        );
        assert_eq!(parsed, vec!["ferramentas".to_string(), "perfil".to_string()]);
        assert!(AppConfig::parse_modifications_manifest("invalid").is_empty());

        let mut cfg = AppConfig::default();
        assert!(!cfg.has_modification(MOD_FERRAMENTAS));
        assert!(!cfg.has_modification(MOD_PERMISSOES_CHAMADA));
        assert!(!cfg.has_modification(MOD_PERFIL));
        cfg.enabled_modifications = parsed;
        assert!(cfg.has_modification(MOD_FERRAMENTAS));
        assert!(!cfg.has_modification(MOD_PERMISSOES_CHAMADA));
        assert!(cfg.has_modification(MOD_PERFIL));
    }

    #[test]
    fn test_recent_rooms_record_dedup_cap_and_remove() {
        let mut cfg = AppConfig::default();
        assert!(cfg.recent_rooms.is_empty());

        for name in ["sala1", "priv-fam-123456", "sala2", "sala3", "sala4", "sala5", "sala6"] {
            cfg.record_recent_room(name);
        }
        assert_eq!(cfg.recent_rooms.len(), 6);
        assert_eq!(cfg.recent_rooms[0], "sala6");
        assert!(!cfg.recent_rooms.contains(&"sala1".to_string()));

        // Re-entering existing room moves it to front without duplicating
        cfg.record_recent_room("priv-fam-123456");
        assert_eq!(cfg.recent_rooms.len(), 6);
        assert_eq!(cfg.recent_rooms[0], "priv-fam-123456");

        // Ephemeral rooms must be ignored
        cfg.record_recent_room("priv-call-abc1234567-123456");
        cfg.record_recent_room("priv-theme-creation-123456");
        assert_eq!(cfg.recent_rooms[0], "priv-fam-123456");

        cfg.remove_recent_room("priv-fam-123456");
        assert!(!cfg.recent_rooms.contains(&"priv-fam-123456".to_string()));
    }
}

/// Id da instalacao (32 hex), o mesmo que o setup grava no config.json: e por ele que o
/// admin conta instalacoes, usuarios ativos e desinstalacoes do Go Live - Lite.
pub fn new_install_id(seed: u64) -> String {
    let other = simple_random_u64() ^ seed.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    format!("{:016x}{:016x}", seed, other)
}

pub fn is_valid_install_id(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
