use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::config::{signaling_ws_url, AppConfig, MOD_AMIGOS, MOD_FERRAMENTAS, MOD_PERFIL, MOD_PERMISSOES_CHAMADA};
use crate::media::audio::{CapturedAudioFrame, MicCapture};
use crate::media::audio_mixer::AudioMixer;
use crate::media::camera_capture::CameraCapturer;
use crate::media::codecs::H264VideoEncoder;
use crate::media::gpu_capture::{try_run_gpu_screen_pipeline, GpuCaptureConfig, GpuCaptureSource};
use crate::media::screen_capture::{
    capture_monitor_thumbnail_rgba, capture_window_thumbnail_rgba,
    compute_target_capture_dimensions, list_monitors, list_windows, make_preview_rgba_from_bgra,
    sleep_until_precise, GdiScreenCapturer, HighResTimerGuard, MonitorTarget, WindowTarget,
};
use crate::media::system_audio::{spawn_system_audio_worker, SystemAudioMode};
use crate::media::video_stream::VideoTexturePool;
#[cfg(feature = "mod_perfil")]
use crate::net::auth::{fetch_user_profile, UserProfileData};
use crate::net::auth::{fetch_ice_servers, fetch_me, login_account, AccountInfo};
#[cfg(feature = "mod_permissoes_chamada")]
use crate::net::protocol::RoomBan;
use crate::net::protocol::{
    is_valid_room_handle, room_handle_from_input, ChatMessage, ChatReplyTo, MusicSource, PeerInfo,
    PlanTier, RoomAdmin, RoomPermissions, ShareBitrate, ShareFps, ShareResolution,
    VideoSource, SCREEN_CHANNELS,
};
#[cfg(feature = "mod_ferramentas")]
use crate::net::room_tools::RoomToolsState;
use crate::net::signaling::{
    now_epoch_ms, spawn_signaling_actor, SignalingCommand, SignalingEvent, SignalingStatus,
};
use crate::net::updater::{AvailableUpdate, UpdateDownloadState, UpdateEvent};
use crate::net::webrtc::{
    channel_keyframe_flag, new_keyframe_signals, spawn_webrtc_actor, AudioFanoutBus,
    KeyframeSignals, RemoteVideoMailbox, StatsTarget, TileStatsSnapshot, VideoFanoutBus,
    VideoRenderPolicyMap, WebRtcCommand, WebRtcEvent, VIEW_POLICY_FOCUSED, VIEW_POLICY_GRID_LARGE,
    VIEW_POLICY_GRID_MULTI, VIEW_POLICY_HIDDEN,
};
use crate::ui::home_view::render_home_view;
#[cfg(feature = "mod_permissoes_chamada")]
use crate::ui::manage_room_modal::ManageRoomView;
#[cfg(feature = "mod_perfil")]
use crate::ui::profile_modal::ProfileTarget;
use crate::ui::room_view::render_room_view;
use crate::ui::theme::apply_golive_theme;
use crate::ui::update_modal::{render_mandatory_update_overlay, render_optional_update_banner};
use crate::win_tray::{
    hide_main_window_win32, sync_autostart_registry, TrayController, TrayEvent,
};

pub const PIP_VIEWPORT_ID_STR: &str = "golive_pip_viewport";

#[derive(Debug, Clone)]
pub struct MediaTileEntry {
    pub tile_id: String,
    pub texture_key: String,
    pub title: String,
    pub badge: String,
    pub is_self: bool,
    pub is_stopped_by_us: bool,
    pub is_resuming: bool,
    pub is_ad_paused: bool,
    pub placeholder_note: Option<String>,
    pub channel: Option<String>,
    pub peer_id: Option<String>,
    pub volume_key: Option<String>,
    pub resolution_label: Option<String>,
    pub tool_id: Option<String>,
    pub tool_kind: Option<String>,
}

enum AuthTaskResult {
    LoginOk(String, AccountInfo),
    LoginErr(String),
    MeOk(AccountInfo),
    MeErr,
}

enum MetaPollCmd {
    CheckUpdateNow,
    /// Botao "reload" de um aviso do topo: refaz agora o status do site e os feeds.
    RefreshNow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSystemTab {
    System,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSettingsTab {
    Account,
    Audio,
    Room,
}

enum MetaFeedEvent {
    SiteStatus(Option<crate::net::protocol::SiteStatus>),
    PeopleOnline(Option<u64>),
    Partner(Option<crate::net::protocol::PartnerCardData>),
    GuestPoints(u64),
    AccountPointsOk(AccountInfo),
    PartnerImageReady {
        url: String,
        kind: PartnerImageKind,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    PartnerImageFailed {
        url: String,
    },
    AvatarReady {
        url: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    AvatarFailed {
        url: String,
    },
    #[cfg(feature = "mod_perfil")]
    UserProfileLoaded(String, Result<UserProfileData, String>),
    #[cfg(feature = "mod_perfil")]
    ProfileBannerReady {
        url: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    #[cfg(feature = "mod_perfil")]
    ProfileBannerFailed {
        url: String,
    },
    ShareThumbReady {
        gen: u64,
        key: String,
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartnerImageKind {
    Banner,
    Icon,
}

#[repr(C)]
struct WinLocalTime {
    w_year: u16,
    w_month: u16,
    w_day_of_week: u16,
    w_day: u16,
    w_hour: u16,
    w_minute: u16,
    w_second: u16,
    w_milliseconds: u16,
}

#[cfg(target_os = "windows")]
extern "system" {
    fn GetLocalTime(lp_system_time: *mut WinLocalTime);
}

fn current_local_weekday_and_minute() -> (u8, u16) {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut st = std::mem::zeroed::<WinLocalTime>();
        GetLocalTime(&mut st);
        let wd = (st.w_day_of_week.min(6)) as u8;
        let mins = st.w_hour.min(23) * 60 + st.w_minute.min(59);
        (wd, mins)
    }
    #[cfg(not(target_os = "windows"))]
    {
        (0, 12 * 60)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareTarget {
    Monitor(MonitorTarget),
    Window(WindowTarget),
}

impl ShareTarget {
    pub fn same_source(&self, other: &ShareTarget) -> bool {
        match (self, other) {
            (ShareTarget::Monitor(a), ShareTarget::Monitor(b)) => a.index == b.index,
            (ShareTarget::Window(a), ShareTarget::Window(b)) => a.hwnd == b.hwnd,
            _ => false,
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            ShareTarget::Monitor(m) => format!("Tela {}", m.index + 1),
            ShareTarget::Window(w) => {
                if !w.process_name.is_empty() {
                    w.process_name.clone()
                } else {
                    w.title.clone()
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePickerTab {
    Monitors,
    Windows,
}

pub struct ActiveScreenWorker {
    pub slot: String,
    pub target: ShareTarget,
    pub target_label: String,
    pub has_audio: bool,
    pub output_width: u32,
    pub output_height: u32,
    preview_enabled: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
    audio_join_handle: Option<std::thread::JoinHandle<()>>,
}

struct CapturedScreenTick {
    slot: String,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

#[cfg(feature = "mod_perfil")]
pub struct ProfileModState {
    pub open_profile_target: Option<ProfileTarget>,
    pub profile_cache: HashMap<String, UserProfileData>,
    pub profile_loading_id: Option<String>,
    pub profile_error: Option<String>,
    pub profile_banner_textures: HashMap<String, egui::TextureHandle>,
    pub profile_banner_in_flight: HashSet<String>,
}

#[cfg(feature = "mod_perfil")]
impl ProfileModState {
    pub fn new() -> Self {
        Self {
            open_profile_target: None,
            profile_cache: HashMap::new(),
            profile_loading_id: None,
            profile_error: None,
            profile_banner_textures: HashMap::new(),
            profile_banner_in_flight: HashSet::new(),
        }
    }
}

#[cfg(feature = "mod_permissoes_chamada")]
pub struct ManageRoomModState {
    pub show_manage_room_modal: bool,
    pub manage_room_view: ManageRoomView,
    pub manage_room_limit_draft: String,
    pub room_bans: Vec<RoomBan>,
}

#[cfg(feature = "mod_permissoes_chamada")]
impl ManageRoomModState {
    pub fn new() -> Self {
        Self {
            show_manage_room_modal: false,
            manage_room_view: ManageRoomView::Menu,
            manage_room_limit_draft: String::new(),
            room_bans: Vec::new(),
        }
    }
}

pub struct GoLiveLiteApp {
    pub rt: Arc<tokio::runtime::Runtime>,
    pub egui_ctx: egui::Context,
    pub config: AppConfig,

    // Home & Auth
    pub room_input: String,
    pub home_error: Option<String>,
    pub show_login_box: bool,
    pub login_username: String,
    pub login_password: String,
    pub login_error: Option<String>,
    pub login_in_flight: bool,
    pub logged_account: Option<AccountInfo>,
    auth_rx: mpsc::UnboundedReceiver<AuthTaskResult>,
    auth_tx: mpsc::UnboundedSender<AuthTaskResult>,

    // Sinalizacao & Sala
    sig_tx: mpsc::UnboundedSender<SignalingCommand>,
    sig_rx: mpsc::UnboundedReceiver<SignalingEvent>,
    pub signaling_status: SignalingStatus,
    pub current_room: Option<String>,
    pub self_id: Option<String>,
    pub self_user_id: Option<String>,
    pub peers: Vec<PeerInfo>,
    pub chat_messages: Vec<ChatMessage>,
    pub typing_peer_ids: HashSet<String>,
    pub chat_blocked_message: Option<String>,
    pub video_sources: Vec<VideoSource>,
    pub room_music: Option<MusicSource>,
    pub device_conflict: Option<(u32, u32)>,
    pub room_banner_error: Option<String>,
    pub server_clock_offset_ms: i64,
    pub room_owner_id: Option<String>,
    pub room_admins: Vec<RoomAdmin>,
    pub room_silenced: Vec<String>,
    pub room_description: String,
    pub room_category: Option<String>,
    pub room_description_draft: String,
    pub room_member_limit: Option<u32>,
    pub room_capture_protected: bool,
    pub room_capture_watermark: bool,
    pub protected_room_blocked: Option<String>,
    pub room_permissions: RoomPermissions,
    pub self_device_no: Option<u32>,
    pub guest_points: u64,
    pub chat_notifications_muted: bool,

    // Chat UI
    pub chat_draft: String,
    pub chat_reply_to: Option<ChatReplyTo>,
    pub chat_was_typing: bool,
    pub right_tab_room: bool,

    // Icone, Top Warns, Anuncios & Online
    pub brand_icon_tex: Option<egui::TextureHandle>,
    meta_tx: mpsc::UnboundedSender<MetaFeedEvent>,
    meta_rx: mpsc::UnboundedReceiver<MetaFeedEvent>,
    pub site_status: Option<crate::net::protocol::SiteStatus>,
    pub announcement: Option<crate::net::protocol::Announcement>,
    pub announcement_live: bool,
    last_announcement_view_id: Option<String>,
    /// Aviso "reload" nao persistente: fica escondido nas proximas vezes, mas aparece nesta sessao.
    announcement_once_id: Option<String>,
    /// Ultimo aviso que ja tocou o som (o site toca uma vez por id, nao a cada edicao).
    announcement_sound_id: Option<String>,
    /// Aviso dispensado no "x": continua consultavel na central de avisos (sino).
    pub dismissed_announcement: Option<crate::net::protocol::Announcement>,
    pub partner_data: Option<crate::net::protocol::PartnerCardData>,
    partner_current_id: Arc<std::sync::Mutex<Option<String>>>,
    shared_account_token: Arc<std::sync::Mutex<Option<String>>>,
    shared_guest_token: Arc<std::sync::Mutex<Option<String>>>,
    pub show_partner_house_override: bool,
    pub people_online: Option<u64>,
    pub partner_banner_tex: Option<egui::TextureHandle>,
    pub partner_icon_tex: Option<egui::TextureHandle>,
    partner_banner_url_loaded: Option<String>,
    partner_icon_url_loaded: Option<String>,
    partner_image_in_flight: HashSet<String>,
    avatar_textures: HashMap<String, egui::TextureHandle>,
    default_avatar_tex: Option<egui::TextureHandle>,
    #[cfg(feature = "mod_ferramentas")]
    wb_image_textures: HashMap<String, egui::TextureHandle>,
    avatar_in_flight: HashSet<String>,
    last_partner_view_id: Option<String>,
    last_partner_session_view_id: Option<String>,
    pub room_link_copied_until_ms: u64,

    // Atualizacao remota
    pub available_update: Option<AvailableUpdate>,
    pub update_state: UpdateDownloadState,
    pub update_dismissed_session: bool,
    update_tx: mpsc::UnboundedSender<UpdateEvent>,
    update_rx: mpsc::UnboundedReceiver<UpdateEvent>,
    meta_wake_tx: std::sync::mpsc::Sender<MetaPollCmd>,

    // Bandeja do sistema e ciclo de vida
    tray: TrayController,
    tray_rx: mpsc::UnboundedReceiver<TrayEvent>,
    force_quit_requested: bool,
    window_hidden: bool,

    // Audio
    pub mic: Arc<MicCapture>,
    pub mixer: Arc<AudioMixer>,

    // WebRTC & Video
    webrtc_tx: mpsc::UnboundedSender<WebRtcCommand>,
    webrtc_rx: mpsc::UnboundedReceiver<WebRtcEvent>,
    keyframe_signals: KeyframeSignals,
    policy_map: VideoRenderPolicyMap,
    pub enabled_grid_preview_tiles: HashSet<String>,
    pub texture_pool: VideoTexturePool,
    remote_resolutions: HashMap<(String, String), (usize, usize)>,
    stopped_remote_tiles: HashSet<(String, String)>,
    resuming_remote_tiles: HashSet<(String, String)>,

    // Transmissao Local de Tela / Janela (Free 2 / Pro Multi-Tela) e Camera
    pub available_monitors: Vec<MonitorTarget>,
    pub available_windows: Vec<WindowTarget>,
    pub picker_selected_target: Option<ShareTarget>,
    pub share_picker_tab: SharePickerTab,
    pub share_thumb_textures: HashMap<String, egui::TextureHandle>,
    share_thumb_gen: Arc<AtomicU64>,
    pub active_screen_slots: Vec<ActiveScreenWorker>,
    audio_bus: AudioFanoutBus,
    video_bus: VideoFanoutBus,
    remote_video_mailbox: RemoteVideoMailbox,
    local_preview_mailbox: Arc<std::sync::Mutex<HashMap<String, CapturedScreenTick>>>,
    pub cached_mic_devices: Vec<String>,
    pub cached_speaker_devices: Vec<String>,
    pub camera_active: bool,
    camera_stop_flag: Option<Arc<AtomicBool>>,
    camera_join_handle: Option<std::thread::JoinHandle<()>>,

    // Layout & Popovers
    pub left_sidebar_collapsed: bool,
    pub right_sidebar_collapsed: bool,
    pub focused_tile_id: Option<String>,
    /// Area do palco no ultimo frame (o painel de Ferramentas fica sobre ela, como no site).
    pub stage_rect: Option<egui::Rect>,
    pub fullscreen_tile_id: Option<String>,
    was_os_fullscreen: bool,
    pub pip_tile_id: Option<String>,
    pub home_room_private: bool,
    pub home_private_create: bool,
    pub home_user_menu_open: bool,
    pub participant_menu_peer_id: Option<String>,
    pub participant_menu_pos: Option<egui::Pos2>,
    pub show_mic_popover: bool,
    pub show_speaker_popover: bool,
    pub show_share_settings_modal: bool,
    pub show_stream_quality_modal: bool,
    pub show_stop_screens_popover: bool,
    #[cfg(feature = "mod_permissoes_chamada")]
    pub manage_room_mod: Option<Box<ManageRoomModState>>,
    #[cfg(feature = "mod_ferramentas")]
    pub room_tools: Option<Box<RoomToolsState>>,
    #[cfg(feature = "mod_perfil")]
    pub profile_mod: Option<Box<ProfileModState>>,
    /// Modificacao "Amigos": so existe com ela instalada e uma conta logada.
    #[cfg(feature = "mod_amigos")]
    pub amigos: Option<Box<crate::ui::friends_view::AmigosState>>,
    pub show_user_settings_modal: bool,
    pub user_settings_tab: UserSettingsTab,
    pub show_app_system_modal: bool,
    pub app_system_tab: AppSystemTab,
    pub show_alerts_popover: bool,
    pub copied_feedback_until_ms: u64,
    pub open_stats_tile_id: Option<String>,
    pub active_stats_snapshot: Option<TileStatsSnapshot>,
    pub stats_copied_until_ms: u64,
}

impl GoLiveLiteApp {
    pub fn new(cc: &eframe::CreationContext<'_>, start_hidden: bool) -> Self {
        apply_golive_theme(&cc.egui_ctx);

        // Runtime Tokio enxuto (2 threads fixas, stacks compactas) para poupar memoria
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(4)
                .thread_stack_size(512 * 1024)
                .enable_all()
                .build()
                .expect("Falha ao criar Tokio runtime"),
        );
        std::thread::Builder::new()
            .name("golive-trim".into())
            .spawn(|| {
                // A partida tem tres picos: contexto OpenGL/fontes (~2s), feeds + banner de
                // parceiro decodificado (~3-6s) e checagem de atualizacao (~5s). Devolver as
                // paginas depois de cada um faz o app chegar no repouso logo, nao "apos um tempo".
                // Medido: 238 MB aos 2s sem isso (pico de criacao do GL), 26-28 MB depois.
                for wait_ms in [900u64, 1300, 4000, 9000] {
                    std::thread::sleep(Duration::from_millis(wait_ms));
                    crate::win_tray::trim_process_working_set();
                }
            })
            .ok();

        let config = AppConfig::load();
        if config.start_with_windows {
            let _ = sync_autostart_registry(true);
        }
        let (tray, tray_rx) = TrayController::spawn(
            cc.egui_ctx.clone(),
            config.start_with_windows,
            config.minimize_to_tray_on_close,
        );
        let room_input = config.last_room.clone();
        let enable_tools_mod =
            cfg!(feature = "mod_ferramentas") && config.has_modification(MOD_FERRAMENTAS);
        let enable_perms_mod = cfg!(feature = "mod_permissoes_chamada")
            && config.has_modification(MOD_PERMISSOES_CHAMADA);
        #[cfg(feature = "mod_perfil")]
        let enable_perfil_mod = cfg!(feature = "mod_perfil") && config.has_modification(MOD_PERFIL);
        let (sig_tx, sig_rx) = spawn_signaling_actor(
            &rt,
            cc.egui_ctx.clone(),
            enable_tools_mod,
            enable_perms_mod,
        );

        // AudioMixer iniciado sob demanda apenas ao entrar na sala (economiza CPU/threads/WASAPI na Home)
        let mixer = Arc::new(AudioMixer::new());
        mixer.deafened.store(config.mics_muted, Ordering::Relaxed);

        let (mic_frame_tx, mic_frame_rx) = std::sync::mpsc::sync_channel::<CapturedAudioFrame>(12);
        let mic = Arc::new(MicCapture::new(mic_frame_tx));
        mic.state.set_gain(config.mic_gain);
        mic.state.set_vad_threshold(config.mic_vad_threshold);
        mic.state
            .push_to_talk
            .store(config.push_to_talk_enabled, Ordering::Relaxed);
        mic.state
            .ptt_vk
            .store(config.push_to_talk_vk as u32, Ordering::Relaxed);

        let keyframe_signals = new_keyframe_signals();
        let policy_map = VideoRenderPolicyMap::new();
        let (webrtc_tx, webrtc_rx, audio_bus, video_bus, remote_video_mailbox) =
            spawn_webrtc_actor(
                &rt,
                sig_tx.clone(),
                mixer.clone(),
                cc.egui_ctx.clone(),
                keyframe_signals.clone(),
                mic_frame_rx,
                policy_map.clone(),
            );

        let (auth_tx, auth_rx) = mpsc::unbounded_channel::<AuthTaskResult>();
        let local_preview_mailbox = Arc::new(std::sync::Mutex::new(HashMap::new()));

        let sig_url = signaling_ws_url();

        // Restaura conta automaticamente em background se houver token salvo
        if let Some(ref tok) = config.account_token {
            let ws = sig_url.clone();
            let t = tok.clone();
            let atx = auth_tx.clone();
            let ctx_clone = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                match fetch_me(&ws, &t) {
                    Ok(acc) => {
                        let _ = atx.send(AuthTaskResult::MeOk(acc));
                    }
                    Err(_) => {
                        let _ = atx.send(AuthTaskResult::MeErr);
                    }
                }
                ctx_clone.request_repaint();
            });
        }

        let (meta_tx, meta_rx) = mpsc::unbounded_channel();
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        let (meta_wake_tx, meta_wake_rx) = std::sync::mpsc::channel();
        let partner_current_id = Arc::new(std::sync::Mutex::new(None::<String>));
        let shared_account_token = Arc::new(std::sync::Mutex::new(config.account_token.clone()));
        let shared_guest_token = Arc::new(std::sync::Mutex::new(config.guest_token.clone()));
        spawn_meta_feed_poller(
            crate::config::signaling_ws_url(),
            meta_tx.clone(),
            update_tx.clone(),
            meta_wake_rx,
            Arc::clone(&partner_current_id),
            Arc::clone(&shared_account_token),
            Arc::clone(&shared_guest_token),
            cc.egui_ctx.clone(),
        );

        let initial_announcement = config
            .persistent_announcement
            .clone()
            .filter(|a| {
                a.is_valid_banner()
                    && !config.hidden_announcement_ids.iter().any(|h| h == &a.id)
            });

        // Conecta WebSocket em background logo ao abrir o app para receber Top Warns / Partner ao vivo
        if config.account_token.is_some()
            || config.guest_token.is_some()
            || !config.display_name.trim().is_empty()
        {
            let conn_name = if config.display_name.trim().is_empty() {
                "UsuarioLite".to_string()
            } else {
                config.display_name.clone()
            };
            let _ = sig_tx.send(SignalingCommand::Connect {
                ws_url: crate::config::signaling_ws_url(),
                name: conn_name,
                client_id: config.client_id.clone(),
                token: config
                    .account_token
                    .clone()
                    .or_else(|| config.guest_token.clone()),
                install_id: config.install_id.clone(),
                device_id: config.device_id.clone(),
            });
        }

        let brand_icon_tex = Some(crate::ui::app_icon::load_brand_icon_texture(
            &cc.egui_ctx,
            64,
        ));

        let monitors = list_monitors();

        Self {
            rt,
            egui_ctx: cc.egui_ctx.clone(),
            config,
            room_input,
            home_error: None,
            show_login_box: false,
            login_username: String::new(),
            login_password: String::new(),
            login_error: None,
            login_in_flight: false,
            logged_account: None,
            auth_rx,
            auth_tx,
            sig_tx,
            sig_rx,
            signaling_status: SignalingStatus::Idle,
            current_room: None,
            self_id: None,
            self_user_id: None,
            peers: Vec::new(),
            chat_messages: Vec::new(),
            typing_peer_ids: HashSet::new(),
            chat_blocked_message: None,
            right_tab_room: false,
            brand_icon_tex,
            meta_tx,
            meta_rx,
            site_status: None,
            announcement: initial_announcement,
            announcement_live: false,
            last_announcement_view_id: None,
            announcement_once_id: None,
            announcement_sound_id: None,
            dismissed_announcement: None,
            partner_data: None,
            partner_current_id,
            shared_account_token,
            shared_guest_token,
            show_partner_house_override: false,
            people_online: None,
            partner_banner_tex: None,
            partner_icon_tex: None,
            partner_banner_url_loaded: None,
            partner_icon_url_loaded: None,
            partner_image_in_flight: HashSet::new(),
            avatar_textures: HashMap::new(),
            default_avatar_tex: None,
            #[cfg(feature = "mod_ferramentas")]
            wb_image_textures: HashMap::new(),
            avatar_in_flight: HashSet::new(),
            last_partner_view_id: None,
            last_partner_session_view_id: None,
            room_link_copied_until_ms: 0,
            available_update: None,
            update_state: UpdateDownloadState::Idle,
            update_dismissed_session: false,
            update_tx,
            update_rx,
            meta_wake_tx,
            tray,
            tray_rx,
            force_quit_requested: false,
            window_hidden: start_hidden,
            video_sources: Vec::new(),
            room_music: None,
            device_conflict: None,
            room_banner_error: None,
            server_clock_offset_ms: 0,
            room_owner_id: None,
            room_admins: Vec::new(),
            room_silenced: Vec::new(),
            room_description: String::new(),
            room_category: None,
            room_description_draft: String::new(),
            room_member_limit: None,
            room_capture_protected: false,
            room_capture_watermark: false,
            protected_room_blocked: None,
            room_permissions: RoomPermissions::all_allowed(),
            self_device_no: None,
            guest_points: 0,
            chat_notifications_muted: false,
            chat_draft: String::new(),
            chat_reply_to: None,
            chat_was_typing: false,
            mic,
            mixer,
            webrtc_tx,
            webrtc_rx,
            keyframe_signals,
            policy_map,
            enabled_grid_preview_tiles: HashSet::new(),
            texture_pool: VideoTexturePool::new(),
            remote_resolutions: HashMap::new(),
            stopped_remote_tiles: HashSet::new(),
            resuming_remote_tiles: HashSet::new(),
            available_monitors: monitors,
            available_windows: Vec::new(),
            picker_selected_target: None,
            share_picker_tab: SharePickerTab::Monitors,
            share_thumb_textures: HashMap::new(),
            share_thumb_gen: Arc::new(AtomicU64::new(0)),
            active_screen_slots: Vec::new(),
            audio_bus,
            video_bus,
            remote_video_mailbox,
            local_preview_mailbox,
            cached_mic_devices: Vec::new(),
            cached_speaker_devices: Vec::new(),
            camera_active: false,
            camera_stop_flag: None,
            camera_join_handle: None,
            left_sidebar_collapsed: false,
            right_sidebar_collapsed: false,
            focused_tile_id: None,
            stage_rect: None,
            fullscreen_tile_id: None,
            was_os_fullscreen: false,
            pip_tile_id: None,
            home_room_private: false,
            home_private_create: false,
            home_user_menu_open: false,
            participant_menu_peer_id: None,
            participant_menu_pos: None,
            show_mic_popover: false,
            show_speaker_popover: false,
            show_share_settings_modal: false,
            show_stream_quality_modal: false,
            show_stop_screens_popover: false,
            #[cfg(feature = "mod_permissoes_chamada")]
            manage_room_mod: if enable_perms_mod {
                Some(Box::new(ManageRoomModState::new()))
            } else {
                None
            },
            #[cfg(feature = "mod_ferramentas")]
            room_tools: if enable_tools_mod {
                Some(Box::new(RoomToolsState::new()))
            } else {
                None
            },
            #[cfg(feature = "mod_perfil")]
            profile_mod: if enable_perfil_mod {
                Some(Box::new(ProfileModState::new()))
            } else {
                None
            },
            #[cfg(feature = "mod_amigos")]
            amigos: None,
            show_user_settings_modal: false,
            user_settings_tab: UserSettingsTab::Account,
            show_app_system_modal: false,
            app_system_tab: AppSystemTab::System,
            show_alerts_popover: false,
            copied_feedback_until_ms: 0,
            open_stats_tile_id: None,
            active_stats_snapshot: None,
            stats_copied_until_ms: 0,
        }
    }

    pub fn effective_name(&self) -> &str {
        if let Some(ref acc) = self.logged_account {
            &acc.display_name
        } else {
            &self.config.display_name
        }
    }

    pub fn current_plan_tier(&self) -> PlanTier {
        if let Some(ref acc) = self.logged_account {
            PlanTier::from_flags(&acc.flags, true)
        } else {
            PlanTier::from_flags(&[], false)
        }
    }

    pub fn has_feature(&self, feature: &str) -> bool {
        self.logged_account
            .as_ref()
            .is_some_and(|acc| acc.features.iter().any(|f| f == feature))
    }

    pub fn can_use_resolution(&self, res: ShareResolution) -> bool {
        !res.requires_pro()
            || self.current_plan_tier().is_pro()
            || self.has_feature("quality_2160p")
    }

    pub fn can_use_fps(&self, fps: ShareFps) -> bool {
        !fps.requires_pro()
            || self.current_plan_tier().is_pro()
            || self.has_feature("fps_120")
    }

    pub fn can_use_bitrate(&self, br: ShareBitrate) -> bool {
        !br.requires_pro()
            || self.current_plan_tier().is_pro()
            || self.has_feature("bitrate_maximo")
    }

    pub fn refresh_share_sources(&mut self) {
        self.available_monitors = list_monitors();
        self.available_windows = list_windows();
        let monitors = &self.available_monitors;
        let windows = &self.available_windows;
        if let Some(ref sel) = self.picker_selected_target {
            let still_exists = match sel {
                ShareTarget::Monitor(m) => monitors.iter().any(|am| am.index == m.index),
                ShareTarget::Window(w) => windows.iter().any(|aw| aw.hwnd == w.hwnd),
            };
            if !still_exists {
                self.picker_selected_target = None;
            }
        }
    }

    pub fn clear_share_thumbnails(&mut self) {
        self.share_thumb_gen.fetch_add(1, Ordering::AcqRel);
        self.share_thumb_textures.clear();
        self.share_thumb_textures.shrink_to_fit();
        self.available_windows.clear();
        self.available_windows.shrink_to_fit();
        self.egui_ctx.request_repaint();
        crate::win_tray::schedule_delayed_memory_trim(Some(self.egui_ctx.clone()));
    }

    pub fn spawn_share_thumbnails_capture(&mut self) {
        let gen = self.share_thumb_gen.fetch_add(1, Ordering::AcqRel) + 1;
        self.share_thumb_textures.clear();

        let monitors = self.available_monitors.clone();
        let windows: Vec<WindowTarget> = self.available_windows.iter().take(16).cloned().collect();
        let gen_ref = Arc::clone(&self.share_thumb_gen);
        let mtx = self.meta_tx.clone();
        let ctx = self.egui_ctx.clone();

        std::thread::Builder::new()
            .name("golive-thumbs".into())
            .spawn(move || {
                for mon in monitors {
                    if gen_ref.load(Ordering::Acquire) != gen {
                        return;
                    }
                    if let Some((width, height, rgba)) = capture_monitor_thumbnail_rgba(&mon, 240, 135)
                    {
                        if gen_ref.load(Ordering::Acquire) != gen {
                            return;
                        }
                        let _ = mtx.send(MetaFeedEvent::ShareThumbReady {
                            gen,
                            key: format!("mon:{}", mon.index),
                            width,
                            height,
                            rgba,
                        });
                        ctx.request_repaint();
                    }
                }
                for win in windows {
                    if gen_ref.load(Ordering::Acquire) != gen {
                        return;
                    }
                    if let Some((width, height, rgba)) =
                        capture_window_thumbnail_rgba(win.hwnd, 240, 135)
                    {
                        if gen_ref.load(Ordering::Acquire) != gen {
                            return;
                        }
                        let _ = mtx.send(MetaFeedEvent::ShareThumbReady {
                            gen,
                            key: format!("win:{}", win.hwnd),
                            width,
                            height,
                            rgba,
                        });
                        ctx.request_repaint();
                    }
                }
            })
            .ok();
    }

    pub fn open_share_picker(&mut self) {
        self.picker_selected_target = None;
        self.refresh_share_sources();
        self.show_mic_popover = false;
        self.show_speaker_popover = false;
        self.show_stop_screens_popover = false;
        self.show_stream_quality_modal = false;
        self.show_share_settings_modal = true;
        self.spawn_share_thumbnails_capture();
    }

    pub fn next_free_screen_slot(&self) -> Option<&'static str> {
        let max_screens = self.current_plan_tier().max_screens();
        let active_slots: Vec<&str> = self
            .active_screen_slots
            .iter()
            .map(|w| w.slot.as_str())
            .collect();
        find_free_screen_slot(&active_slots, max_screens)
    }

    pub fn start_account_login(&mut self) {
        if self.login_in_flight {
            return;
        }
        self.login_in_flight = true;
        self.login_error = None;
        let ws = signaling_ws_url();
        let user = self.login_username.clone();
        let pass = self.login_password.clone();
        let atx = self.auth_tx.clone();
        let ctx = self.egui_ctx.clone();

        std::thread::spawn(move || {
            match login_account(&ws, &user, &pass) {
                Ok((tok, acc)) => {
                    let _ = atx.send(AuthTaskResult::LoginOk(tok, acc));
                }
                Err(e) => {
                    let _ = atx.send(AuthTaskResult::LoginErr(e));
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn logout_account(&mut self) {
        self.logged_account = None;
        #[cfg(feature = "mod_amigos")]
        if self.amigos.take().is_some() {
            self.send_tool_json(serde_json::json!({ "type": "presence-watch", "ids": [] }));
        }
        self.config.account_token = None;
        self.config.save();
        if let Ok(mut guard) = self.shared_account_token.lock() {
            *guard = None;
        }
        let ws = signaling_ws_url();
        let wtx = self.webrtc_tx.clone();
        std::thread::spawn(move || {
            let ice = fetch_ice_servers(&ws, None);
            let _ = wtx.send(WebRtcCommand::SetIceServers(ice));
        });
    }

    pub fn join_room_by_input(&mut self) {
        let handle = if !self.home_room_private {
            room_handle_from_input(&self.room_input)
        } else if !self.home_private_create {
            crate::net::protocol::normalize_private_join_handle(&self.room_input)
        } else {
            let parsed = room_handle_from_input(&self.room_input);
            let base = parsed
                .strip_prefix(crate::net::protocol::PRIVATE_ROOM_PREFIX)
                .unwrap_or(&parsed);
            if base.is_empty()
                || base.len() > crate::net::protocol::MAX_PRIVATE_ROOM_NAME_LENGTH
                || !base
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                self.home_error = Some(format!(
                    "Nome da sala privada invalido (1 a {} caracteres: letras, numeros, '-' ou '_').",
                    crate::net::protocol::MAX_PRIVATE_ROOM_NAME_LENGTH
                ));
                return;
            }
            let code = crate::net::protocol::generate_private_room_code();
            crate::net::protocol::to_private_room_handle(base, &code)
        };
        self.join_room_handle(handle);
    }

    pub fn join_room_handle(&mut self, handle: String) {
        if !is_valid_room_handle(&handle) {
            self.home_error = Some(
                "Codigo de sala invalido. Use letras, numeros, '-' ou '_' (max 32 caracteres)."
                    .to_string(),
            );
            return;
        }
        let name = self.effective_name().trim().to_string();
        if name.is_empty() {
            self.home_error = Some("Informe seu nome antes de entrar.".to_string());
            return;
        }

        self.home_error = None;
        self.home_user_menu_open = false;
        self.protected_room_blocked = None;
        self.room_banner_error = None;
        self.device_conflict = None;
        self.config.last_room = handle.clone();
        self.config.record_recent_room(&handle);
        self.config.save();
        let _ = self.mixer.start(self.config.speaker_device_name.as_deref());

        let token = self
            .config
            .account_token
            .clone()
            .or_else(|| self.config.guest_token.clone());

        // Atualiza servidores ICE/TURN ao entrar na sala (credenciais dinamicas do Cloudflare/VPS).
        {
            let ws = signaling_ws_url();
            let acc_tok = self.config.account_token.clone();
            let wtx = self.webrtc_tx.clone();
            std::thread::spawn(move || {
                let ice = fetch_ice_servers(&ws, acc_tok.as_deref());
                let _ = wtx.send(WebRtcCommand::SetIceServers(ice));
            });
        }

        let _ = self.sig_tx.send(SignalingCommand::Connect {
            ws_url: signaling_ws_url(),
            name,
            client_id: self.config.client_id.clone(),
            token,
            install_id: self.config.install_id.clone(),
            device_id: self.config.device_id.clone(),
        });
        let _ = self.sig_tx.send(SignalingCommand::JoinRoom {
            room: handle.clone(),
            confirm_device: false,
        });
        self.current_room = Some(handle);
    }

    pub fn confirm_device_join(&mut self) {
        self.device_conflict = None;
        let _ = self.sig_tx.send(SignalingCommand::ConfirmDeviceJoin);
    }

    pub fn leave_room(&mut self) {
        self.close_stats_overlay();
        self.stop_all_screen_shares();
        self.stop_camera();
        self.mic.stop();
        let _ = self.webrtc_tx.send(WebRtcCommand::CloseAll);
        let _ = self.sig_tx.send(SignalingCommand::LeaveRoom);

        self.current_room = None;
        self.right_tab_room = false;
        self.last_partner_view_id = None;
        self.room_owner_id = None;
        self.room_admins.clear();
        #[cfg(feature = "mod_permissoes_chamada")]
        if let Some(ref mut m) = self.manage_room_mod {
            m.room_bans.clear();
            m.room_bans.shrink_to_fit();
            m.show_manage_room_modal = false;
            m.manage_room_view = ManageRoomView::Menu;
            m.manage_room_limit_draft.clear();
        }
        self.room_silenced.clear();
        self.room_description.clear();
        self.room_description_draft.clear();
        self.room_category = None;
        self.room_member_limit = None;
        self.room_capture_protected = false;
        self.room_capture_watermark = false;
        self.room_permissions = RoomPermissions::all_allowed();
        self.show_stream_quality_modal = false;
        self.show_stop_screens_popover = false;
        self.show_share_settings_modal = false;
        self.clear_share_thumbnails();
        if self.user_settings_tab == UserSettingsTab::Room {
            self.user_settings_tab = UserSettingsTab::Account;
        }
        #[cfg(feature = "mod_ferramentas")]
        if let Some(ref mut tools) = self.room_tools {
            tools.reset();
        }
        #[cfg(feature = "mod_perfil")]
        if let Some(ref mut prof) = self.profile_mod {
            prof.open_profile_target = None;
        }
        self.participant_menu_peer_id = None;
        self.participant_menu_pos = None;
        self.show_alerts_popover = false;
        self.peers.clear();
        self.chat_messages.clear();
        self.typing_peer_ids.clear();
        self.video_sources.clear();
        self.room_music = None;
        self.device_conflict = None;
        self.room_banner_error = None;
        self.focused_tile_id = None;
        let ctx = self.egui_ctx.clone();
        self.exit_fullscreen(&ctx);
        self.pip_tile_id = None;
        self.stats_copied_until_ms = 0;
        self.stopped_remote_tiles.clear();
        self.resuming_remote_tiles.clear();
        self.remote_resolutions.clear();
        self.remote_video_mailbox.clear();
        if let Ok(mut guard) = self.local_preview_mailbox.lock() {
            guard.clear();
            guard.shrink_to_fit();
        }
        self.texture_pool.clear();
        crate::ui::native_emoji::clear_cache();
        // Estado por widget que o egui guarda para sempre (cursor de cada campo, rolagem de cada
        // lista, cada mensagem/tile/bloco ja visto): sem isso cresce a cada sala e nunca volta.
        self.egui_ctx.memory_mut(|m| m.data.clear());
        self.mixer.clear();
        self.mixer.stop();
        self.policy_map.clear();
        self.enabled_grid_preview_tiles.clear();

        // Libera texturas e caches de sala/perfis/parceiros para retornar ao baseline minimo de RAM na Home
        self.partner_banner_tex = None;
        self.partner_icon_tex = None;
        self.partner_banner_url_loaded = None;
        self.partner_icon_url_loaded = None;
        self.partner_image_in_flight.clear();
        #[cfg(feature = "mod_perfil")]
        if let Some(ref mut prof) = self.profile_mod {
            prof.profile_cache.clear();
            prof.profile_cache.shrink_to_fit();
            prof.profile_banner_textures.clear();
            prof.profile_banner_textures.shrink_to_fit();
            prof.profile_banner_in_flight.clear();
        }
        #[cfg(feature = "mod_ferramentas")]
        {
            self.wb_image_textures.clear();
            self.wb_image_textures.shrink_to_fit();
        }
        let own_av = self
            .logged_account
            .as_ref()
            .and_then(|a| a.avatar_url.as_deref())
            .and_then(resolve_media_url);
        self.avatar_textures
            .retain(|k, _| own_av.as_deref() == Some(k.as_str()));
        self.avatar_textures.shrink_to_fit();
        self.avatar_in_flight.clear();

        self.peers.shrink_to_fit();
        self.chat_messages.shrink_to_fit();
        self.video_sources.shrink_to_fit();
        self.room_admins.shrink_to_fit();
        self.room_silenced.shrink_to_fit();
        self.typing_peer_ids.shrink_to_fit();
        self.stopped_remote_tiles.shrink_to_fit();
        self.resuming_remote_tiles.shrink_to_fit();
        self.remote_resolutions.shrink_to_fit();
        crate::win_tray::trim_process_working_set();
    }

    pub fn close_stats_overlay(&mut self) {
        self.open_stats_tile_id = None;
        self.active_stats_snapshot = None;
        let _ = self.webrtc_tx.send(WebRtcCommand::SetWatchedStats(None));
    }

    pub fn toggle_tile_stats(&mut self, tile: &MediaTileEntry) {
        if self.open_stats_tile_id.as_deref() == Some(tile.tile_id.as_str()) {
            self.close_stats_overlay();
            return;
        }
        let Some(ref ch) = tile.channel else {
            return;
        };
        let target = if tile.is_self {
            StatsTarget::Own {
                channel: ch.clone(),
            }
        } else if let Some(ref pid) = tile.peer_id {
            StatsTarget::Remote {
                channel: ch.clone(),
                peer_id: pid.clone(),
            }
        } else {
            return;
        };
        self.open_stats_tile_id = Some(tile.tile_id.clone());
        self.active_stats_snapshot = None;
        let _ = self
            .webrtc_tx
            .send(WebRtcCommand::SetWatchedStats(Some(target)));
    }

    pub fn toggle_mic(&mut self) {
        let was_on = self.mic.state.enabled.load(Ordering::Relaxed);
        if was_on {
            self.mic.stop();
            let _ = self.sig_tx.send(SignalingCommand::SetMic(false));
            let _ = self.webrtc_tx.send(WebRtcCommand::StopChannel {
                channel: "mic".to_string(),
            });
            self.config.mic_enabled_on_join = false;
            self.config.save();
        } else {
            let mic_blocked = if !self.room_permissions.mic && !self.is_room_manager() {
                #[cfg(feature = "mod_ferramentas")]
                let granted = self.room_tools.as_ref().is_some_and(|rt| {
                    rt.speakers.iter().any(|sid| {
                        self.self_user_id.as_deref() == Some(sid.as_str())
                            || self.self_id.as_deref() == Some(sid.as_str())
                    })
                });
                #[cfg(not(feature = "mod_ferramentas"))]
                let granted = false;
                !granted
            } else {
                false
            };
            if mic_blocked {
                self.room_banner_error = Some(
                    "Microfones bloqueados na sala. Levante a mao para pedir a fala.".to_string(),
                );
                return;
            }
            let dev = self.config.mic_device_name.as_deref();
            match self.mic.start(dev) {
                Ok(()) => {
                    let _ = self.sig_tx.send(SignalingCommand::SetMic(true));
                    let peers: Vec<String> = self.peers.iter().map(|p| p.id.clone()).collect();
                    let _ = self.webrtc_tx.send(WebRtcCommand::StartChannel {
                        channel: "mic".to_string(),
                        peers,
                        with_audio: false,
                    });
                    self.config.mic_enabled_on_join = true;
                    self.config.save();
                }
                Err(e) => {
                    self.room_banner_error = Some(e);
                }
            }
        }
    }

    pub fn restart_mic_if_active(&mut self) {
        if self.mic.state.enabled.load(Ordering::Relaxed) {
            let dev = self.config.mic_device_name.as_deref();
            if let Err(e) = self.mic.start(dev) {
                self.room_banner_error = Some(e);
            }
        }
    }

    pub fn toggle_deafen(&mut self) {
        let next = !self.mixer.deafened.load(Ordering::Relaxed);
        self.mixer.deafened.store(next, Ordering::Relaxed);
        self.config.mics_muted = next;
        self.config.save();
        let _ = self.sig_tx.send(SignalingCommand::SetMicsMuted(next));
    }

    pub fn start_screen_share(&mut self, target: ShareTarget) {
        let tier = self.current_plan_tier();
        let max_screens = tier.max_screens();
        if self.active_screen_slots.len() >= max_screens {
            self.room_banner_error = Some(format!(
                "Limite de {} telas/janelas simultaneas atingido no plano {}.",
                max_screens,
                tier.label()
            ));
            return;
        }
        let Some(slot_str) = self.next_free_screen_slot() else {
            self.room_banner_error = Some("Nenhum canal de transmissao disponivel.".to_string());
            return;
        };

        self.refresh_share_sources();
        let (src_w, src_h, gpu_source, audio_mode, target_label, fresh_target) = match &target {
            ShareTarget::Monitor(mon) => {
                let Some(fresh_mon) = self
                    .available_monitors
                    .iter()
                    .find(|m| m.index == mon.index)
                    .cloned()
                else {
                    self.room_banner_error =
                        Some("O monitor selecionado nao esta mais disponivel.".to_string());
                    return;
                };
                (
                    fresh_mon.width.max(16) as u32,
                    fresh_mon.height.max(16) as u32,
                    GpuCaptureSource::Monitor(fresh_mon.clone()),
                    SystemAudioMode::ExcludeProcessTree(std::process::id()),
                    target.short_label(),
                    ShareTarget::Monitor(fresh_mon),
                )
            }
            ShareTarget::Window(win) => {
                let Some(fresh_win) = self
                    .available_windows
                    .iter()
                    .find(|w| w.hwnd == win.hwnd)
                    .cloned()
                else {
                    self.room_banner_error =
                        Some("A janela selecionada nao esta mais aberta.".to_string());
                    return;
                };
                let pid = fresh_win.pid;
                (
                    fresh_win.width.max(16) as u32,
                    fresh_win.height.max(16) as u32,
                    GpuCaptureSource::Window(fresh_win.clone()),
                    SystemAudioMode::IncludeProcessTree(pid),
                    target.short_label(),
                    ShareTarget::Window(fresh_win),
                )
            }
        };

        let (out_w, out_h) = self.current_effective_resolution();
        let target_fps = self.current_effective_fps().clamp(5, 240);
        let bitrate_bps = self.current_effective_bitrate_kbps() * 1000;
        let share_audio = self.config.share_audio;
        let peer_ids: Vec<String> = self.peers.iter().map(|p| p.id.clone()).collect();

        let slot = slot_str.to_string();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let initial_prev = self.config.grid_video_preview
            || self
                .focused_tile_id
                .as_deref()
                .is_some_and(|f| f == format!("self:{}", slot));
        let preview_enabled = Arc::new(AtomicBool::new(initial_prev));
        let preview_enabled_gpu = preview_enabled.clone();
        let preview_enabled_sw = preview_enabled.clone();
        let stop_clone = stop_flag.clone();
        let slot_clone = slot.clone();
        let preview_mb = self.local_preview_mailbox.clone();
        let wtx = self.webrtc_tx.clone();
        let vbus = self.video_bus.clone();
        let ctx = self.egui_ctx.clone();
        let kf_flag = channel_keyframe_flag(&self.keyframe_signals, &slot);

        let (cap_w, cap_h) = compute_target_capture_dimensions(src_w, src_h, out_w, out_h);

        crate::diag_info!(
            "share",
            "iniciando transmissao slot={} alvo='{}' {}x{} @ {}fps {}kbps audio={} peers={}",
            slot,
            target_label,
            cap_w,
            cap_h,
            target_fps,
            bitrate_bps / 1000,
            share_audio,
            peer_ids.len()
        );

        let _ = self.webrtc_tx.send(WebRtcCommand::StartChannel {
            channel: slot.clone(),
            peers: peer_ids,
            with_audio: share_audio,
        });
        let _ = self.webrtc_tx.send(WebRtcCommand::UpdateLocalStreamMeta {
            channel: slot.clone(),
            width: cap_w,
            height: cap_h,
            target_fps,
            target_bitrate_kbps: bitrate_bps / 1000,
            encoder_name: "GPU H264 / OpenH264".to_string(),
            hardware: true,
        });

        let audio_join_handle = if share_audio {
            spawn_system_audio_worker(
                slot.clone(),
                audio_mode,
                stop_flag.clone(),
                self.audio_bus.clone(),
            )
        } else {
            None
        };

        let gpu_source_fb = gpu_source.clone();
        let join_handle = std::thread::Builder::new()
            .name(format!("golive-screen-{}", slot_clone))
            .spawn(move || {
                // 1. Caminho rapido zero-copy GPU: DXGI/WGC -> D3D11 VideoProcessor -> MF H.264 HW
                let gpu_cfg = GpuCaptureConfig {
                    source: gpu_source,
                    out_width: cap_w,
                    out_height: cap_h,
                    target_fps,
                    bitrate_bps,
                    preview_enabled: preview_enabled_gpu,
                };
                let slot_ready = slot_clone.clone();
                let slot_h264 = slot_clone.clone();
                let slot_prev = slot_clone.clone();
                let wtx_ready = wtx.clone();
                let vbus_h264 = vbus.clone();
                let stop_h264 = stop_clone.clone();
                let prev_gpu = preview_mb.clone();
                let stop_prev_gpu = stop_clone.clone();
                let ctx_gpu = ctx.clone();

                let gpu_res = try_run_gpu_screen_pipeline(
                    gpu_cfg,
                    stop_clone.clone(),
                    kf_flag.clone(),
                    |info| {
                        let _ = wtx_ready.send(WebRtcCommand::UpdateLocalStreamMeta {
                            channel: slot_ready.clone(),
                            width: info.width,
                            height: info.height,
                            target_fps: info.target_fps,
                            target_bitrate_kbps: bitrate_bps / 1000,
                            encoder_name: info.encoder_name,
                            hardware: true,
                        });
                    },
                    move |au, is_keyframe, pts| {
                        if !stop_h264.load(Ordering::Acquire) {
                            vbus_h264.send_encoded_video(&slot_h264, au, is_keyframe, pts);
                        }
                    },
                    |pw, ph, rgba| {
                        if !stop_prev_gpu.load(Ordering::Acquire) {
                            if let Ok(mut guard) = prev_gpu.lock() {
                                guard.insert(
                                    slot_prev.clone(),
                                    CapturedScreenTick {
                                        slot: slot_prev.clone(),
                                        width: pw,
                                        height: ph,
                                        rgba,
                                    },
                                );
                            }
                            ctx_gpu.request_repaint();
                        }
                    },
                );
                if stop_clone.load(Ordering::Relaxed) || gpu_res.is_ok() {
                    crate::diag_info!(
                        "share",
                        "captura GPU encerrada normalmente slot={}",
                        slot_clone
                    );
                    return;
                }
                if let Err(ref gpu_err) = gpu_res {
                    crate::diag_warn!(
                        "share",
                        "pipeline GPU encerrou/indisponivel no slot={} ({}); iniciando fallback OpenH264",
                        slot_clone,
                        gpu_err
                    );
                    // Se a janela foi fechada externamente durante WGC, nao inicia fallback GDI zumbi.
                    if let GpuCaptureSource::Window(ref w) = gpu_source_fb {
                        if gpu_err.contains("nao esta mais aberta") {
                            let _ = w;
                            return;
                        }
                    }
                }

                // 2. Fallback em software limitado a 720p30 (4 Mbps) para nao travar CPU em multi-midia
                let (sw_w, sw_h) = compute_target_capture_dimensions(cap_w, cap_h, 1280, 720);
                let sw_fps = target_fps.min(30);
                let sw_bitrate = bitrate_bps.min(4_000_000);
                let _ = wtx.send(WebRtcCommand::UpdateLocalStreamMeta {
                    channel: slot_clone.clone(),
                    width: sw_w,
                    height: sw_h,
                    target_fps: sw_fps,
                    target_bitrate_kbps: sw_bitrate / 1000,
                    encoder_name: "OpenH264".to_string(),
                    hardware: false,
                });

                let (init_w, init_h) = match &gpu_source_fb {
                    GpuCaptureSource::Monitor(m) => (m.width, m.height),
                    GpuCaptureSource::Window(w) => (w.width, w.height),
                };
                let Ok(mut capturer) = GdiScreenCapturer::new(init_w, init_h) else {
                    crate::diag_warn!("share", "GdiScreenCapturer falhou ao iniciar slot={}", slot_clone);
                    return;
                };

                let _timer_guard = HighResTimerGuard::new();
                let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
                let (recycle_tx, recycle_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(2);
                let buf_bytes = (sw_w as usize) * (sw_h as usize) * 4;
                let _ = recycle_tx.try_send(vec![0u8; buf_bytes]);
                let _ = recycle_tx.try_send(vec![0u8; buf_bytes]);

                let slot_enc = slot_clone.clone();
                let vbus_enc = vbus.clone();
                let kf_enc = kf_flag.clone();
                let stop_enc = stop_clone.clone();
                let recycle_back = recycle_tx.clone();

                let enc_thread = std::thread::Builder::new()
                    .name(format!("golive-swenc-{}", slot_enc))
                    .spawn(move || {
                        let mut encoder = H264VideoEncoder::new(true);
                        let mut frame_idx: u64 = 0;
                        let gop_frames = (sw_fps as u64 * 2).clamp(30, 480);
                        let capture_start = Instant::now();

                        while let Ok(bgra) = frame_rx.recv() {
                            if stop_enc.load(Ordering::Relaxed) {
                                break;
                            }
                            let frame_pts = capture_start.elapsed();
                            let periodic_idr = frame_idx == 0 || frame_idx % gop_frames == 0;
                            let demanded_idr = kf_enc.swap(false, Ordering::AcqRel);
                            let encoded = encoder.encode_bgra_with_keyframe(
                                &bgra,
                                sw_w as usize,
                                sw_h as usize,
                                sw_bitrate,
                                sw_fps,
                                periodic_idr || demanded_idr,
                            );
                            let _ = recycle_back.try_send(bgra);
                            if let Some((h264_au, is_keyframe)) = encoded {
                                vbus_enc.send_encoded_video(
                                    &slot_enc,
                                    h264_au,
                                    is_keyframe,
                                    frame_pts,
                                );
                            }
                            frame_idx = frame_idx.wrapping_add(1);
                        }
                    })
                    .ok();

                let frame_interval =
                    Duration::from_micros((1_000_000 / sw_fps as u64).max(4_000));
                let mut next_at = Instant::now();
                let mut last_preview_at = Instant::now() - Duration::from_secs(1);

                while !stop_clone.load(Ordering::Relaxed) {
                    let mut bgra = recycle_rx
                        .try_recv()
                        .unwrap_or_else(|_| Vec::with_capacity(buf_bytes));

                    let captured = match &gpu_source_fb {
                        GpuCaptureSource::Monitor(mon) => {
                            capturer.capture_monitor_bgra_into(mon, sw_w, sw_h, &mut bgra)
                        }
                        GpuCaptureSource::Window(win) => {
                            if !win.is_alive() {
                                break;
                            }
                            capturer.capture_window_bgra_into(win.hwnd, sw_w, sw_h, &mut bgra)
                        }
                    };

                    if captured {
                        if preview_enabled_sw.load(Ordering::Relaxed)
                            && last_preview_at.elapsed() >= Duration::from_millis(50)
                        {
                            if let Some((pw, ph, prev_rgba)) =
                                make_preview_rgba_from_bgra(&bgra, sw_w, sw_h, 1280, 720)
                            {
                                if !stop_clone.load(Ordering::Acquire) {
                                    if let Ok(mut guard) = preview_mb.lock() {
                                        guard.insert(
                                            slot_clone.clone(),
                                            CapturedScreenTick {
                                                slot: slot_clone.clone(),
                                                width: pw,
                                                height: ph,
                                                rgba: prev_rgba,
                                            },
                                        );
                                    }
                                    ctx.request_repaint();
                                }
                                last_preview_at = Instant::now();
                            }
                        }
                        let _ = frame_tx.try_send(bgra);
                    } else {
                        let _ = recycle_tx.try_send(bgra);
                    }

                    next_at += frame_interval;
                    let now = Instant::now();
                    if now > next_at + frame_interval * 2 {
                        next_at = now + frame_interval;
                    }
                    if next_at > now {
                        sleep_until_precise(next_at);
                    }
                }

                drop(frame_tx);
                if let Some(eh) = enc_thread {
                    let _ = eh.join();
                }
            })
            .ok();

        self.active_screen_slots.push(ActiveScreenWorker {
            slot,
            target: fresh_target,
            target_label,
            has_audio: share_audio,
            output_width: cap_w,
            output_height: cap_h,
            preview_enabled,
            stop_flag,
            join_handle,
            audio_join_handle,
        });

        self.announce_sharing();
    }

    fn spawn_worker_reaper(
        slot: String,
        join_handle: Option<std::thread::JoinHandle<()>>,
        audio_join_handle: Option<std::thread::JoinHandle<()>>,
        ctx: egui::Context,
    ) {
        let thread_name = format!("golive-stop-{}", slot);
        let _ = std::thread::Builder::new().name(thread_name).spawn(move || {
            if let Some(handle) = join_handle {
                let _ = handle.join();
            }
            if let Some(ah) = audio_join_handle {
                let _ = ah.join();
            }
            ctx.request_repaint();
            std::thread::sleep(Duration::from_millis(160));
            ctx.request_repaint();
            crate::win_tray::trim_process_working_set();
            std::thread::sleep(Duration::from_millis(550));
            crate::win_tray::trim_process_working_set();
        });
    }

    pub fn stop_screen_slot(&mut self, slot: &str) {
        let Some(pos) = self.active_screen_slots.iter().position(|w| w.slot == slot) else {
            return;
        };
        let w = self.active_screen_slots.remove(pos);
        w.stop_flag.store(true, Ordering::Release);
        let _ = self.webrtc_tx.send(WebRtcCommand::StopChannel {
            channel: w.slot.clone(),
        });
        if let Ok(mut guard) = self.local_preview_mailbox.lock() {
            guard.remove(&w.slot);
            if guard.is_empty() {
                guard.shrink_to_fit();
            }
        }
        let tile_key = format!("self:{}", w.slot);
        self.texture_pool.remove(&tile_key);
        if self.focused_tile_id.as_deref() == Some(tile_key.as_str()) {
            self.focused_tile_id = None;
        }
        if self.open_stats_tile_id.as_deref() == Some(tile_key.as_str()) {
            self.close_stats_overlay();
        }
        self.egui_ctx.request_repaint();
        Self::spawn_worker_reaper(
            w.slot,
            w.join_handle,
            w.audio_join_handle,
            self.egui_ctx.clone(),
        );
        self.announce_sharing();
        crate::win_tray::schedule_delayed_memory_trim(Some(self.egui_ctx.clone()));
    }

    pub fn stop_all_screen_shares(&mut self) {
        if self.active_screen_slots.is_empty() {
            return;
        }
        let drained: Vec<_> = self.active_screen_slots.drain(..).collect();
        for w in drained {
            w.stop_flag.store(true, Ordering::Release);
            let _ = self.webrtc_tx.send(WebRtcCommand::StopChannel {
                channel: w.slot.clone(),
            });
            if let Ok(mut guard) = self.local_preview_mailbox.lock() {
                guard.remove(&w.slot);
                if guard.is_empty() {
                    guard.shrink_to_fit();
                }
            }
            let tile_key = format!("self:{}", w.slot);
            self.texture_pool.remove(&tile_key);
            if self.focused_tile_id.as_deref() == Some(tile_key.as_str()) {
                self.focused_tile_id = None;
            }
            if self.open_stats_tile_id.as_deref() == Some(tile_key.as_str()) {
                self.close_stats_overlay();
            }
            Self::spawn_worker_reaper(
                w.slot,
                w.join_handle,
                w.audio_join_handle,
                self.egui_ctx.clone(),
            );
        }
        self.egui_ctx.request_repaint();
        self.announce_sharing();
        crate::win_tray::schedule_delayed_memory_trim(Some(self.egui_ctx.clone()));
    }

    pub fn toggle_camera(&mut self) {
        if self.camera_active {
            self.stop_camera();
        } else {
            self.start_camera();
        }
    }

    fn start_camera(&mut self) {
        self.stop_camera();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let preview_mb = self.local_preview_mailbox.clone();
        let vbus = self.video_bus.clone();
        let ctx = self.egui_ctx.clone();
        let kf_flag = channel_keyframe_flag(&self.keyframe_signals, "camera");
        let peer_ids: Vec<String> = self.peers.iter().map(|p| p.id.clone()).collect();

        let _ = self.webrtc_tx.send(WebRtcCommand::StartChannel {
            channel: "camera".to_string(),
            peers: peer_ids,
            with_audio: false,
        });
        let _ = self.webrtc_tx.send(WebRtcCommand::UpdateLocalStreamMeta {
            channel: "camera".to_string(),
            width: 640,
            height: 360,
            target_fps: 15,
            target_bitrate_kbps: 500,
            encoder_name: "OpenH264".to_string(),
            hardware: false,
        });

        let handle = std::thread::Builder::new()
            .name("golive-camera".to_string())
            .spawn(move || {
            let mut cam = CameraCapturer::new();
            let mut encoder = H264VideoEncoder::new(false);
            let interval = Duration::from_millis(66); // ~15fps leve para camera
            let mut frame_idx: u64 = 0;
            let capture_start = Instant::now();
            while !stop_clone.load(Ordering::Relaxed) {
                let start = Instant::now();
                let (w, h) = (640u32, 360u32);
                let rgba = cam.capture_frame(w, h);
                let frame_pts = capture_start.elapsed();
                if !stop_clone.load(Ordering::Acquire) {
                    if let Ok(mut guard) = preview_mb.lock() {
                        guard.insert(
                            "camera".to_string(),
                            CapturedScreenTick {
                                slot: "camera".to_string(),
                                width: w as usize,
                                height: h as usize,
                                rgba: rgba.to_vec(),
                            },
                        );
                    }
                }
                let periodic_idr = frame_idx == 0 || frame_idx % 30 == 0;
                let demanded_idr = kf_flag.swap(false, Ordering::AcqRel);
                if let Some((h264_au, is_keyframe)) = encoder.encode_rgba_with_keyframe(
                    rgba,
                    w as usize,
                    h as usize,
                    500_000,
                    15,
                    periodic_idr || demanded_idr,
                ) {
                    if !stop_clone.load(Ordering::Acquire) {
                        vbus.send_encoded_video("camera", h264_au, is_keyframe, frame_pts);
                    }
                }
                frame_idx = frame_idx.wrapping_add(1);
                ctx.request_repaint();
                let elapsed = start.elapsed();
                if elapsed < interval {
                    std::thread::sleep(interval - elapsed);
                }
            }
        })
        .ok();

        self.camera_join_handle = handle;
        self.camera_stop_flag = Some(stop_flag);
        self.camera_active = true;
        self.announce_sharing();
    }

    pub fn stop_camera(&mut self) {
        if let Some(flag) = self.camera_stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        let cam_handle = self.camera_join_handle.take();
        if let Ok(mut guard) = self.local_preview_mailbox.lock() {
            guard.remove("camera");
            if guard.is_empty() {
                guard.shrink_to_fit();
            }
        }
        if self.camera_active || cam_handle.is_some() {
            self.camera_active = false;
            let _ = self.webrtc_tx.send(WebRtcCommand::StopChannel {
                channel: "camera".to_string(),
            });
            self.texture_pool.remove("self:camera");
            if self.focused_tile_id.as_deref() == Some("self:camera") {
                self.focused_tile_id = None;
            }
            if self.open_stats_tile_id.as_deref() == Some("self:camera") {
                self.close_stats_overlay();
            }
            Self::spawn_worker_reaper(
                "camera".to_string(),
                cam_handle,
                None,
                self.egui_ctx.clone(),
            );
            self.announce_sharing();
            crate::win_tray::schedule_delayed_memory_trim(Some(self.egui_ctx.clone()));
        }
    }

    fn announce_sharing(&self) {
        let screen_count = self.active_screen_slots.len();
        let _ = self.sig_tx.send(SignalingCommand::SetSharing {
            screen: screen_count > 0,
            camera: self.camera_active,
            screens: screen_count,
            cameras: if self.camera_active { 1 } else { 0 },
        });
    }

    pub fn send_chat(&self, text: String, reply_to: Option<ChatReplyTo>) {
        let _ = self
            .sig_tx
            .send(SignalingCommand::SendChat { text, reply_to });
    }

    pub fn send_typing(&self, typing: bool) {
        let _ = self.sig_tx.send(SignalingCommand::SendTyping(typing));
    }

    pub fn stop_watching_peer(&mut self, channel: String, peer_id: String) {
        self.stopped_remote_tiles
            .insert((channel.clone(), peer_id.clone()));
        self.resuming_remote_tiles
            .remove(&(channel.clone(), peer_id.clone()));
        let _ = self
            .webrtc_tx
            .send(WebRtcCommand::StopWatchingPeer { channel, peer_id });
    }

    pub fn prune_stopped_peer_channels(&mut self, peer: &PeerInfo, ctx: Option<&egui::Context>) {
        let mut stale: Vec<(String, String)> = Vec::new();
        for (ch, pid) in self
            .stopped_remote_tiles
            .iter()
            .chain(self.resuming_remote_tiles.iter())
            .chain(self.remote_resolutions.keys())
        {
            if pid == &peer.id && !is_peer_video_channel_active(peer, ch) {
                let key = (ch.clone(), pid.clone());
                if !stale.contains(&key) {
                    stale.push(key);
                }
            }
        }
        for ch in self.remote_video_mailbox.keys_for_peer(&peer.id) {
            if !is_peer_video_channel_active(peer, &ch) {
                let key = (ch, peer.id.clone());
                if !stale.contains(&key) {
                    stale.push(key);
                }
            }
        }
        if stale.is_empty() {
            return;
        }
        for (ch, pid) in stale {
            self.stopped_remote_tiles.remove(&(ch.clone(), pid.clone()));
            self.resuming_remote_tiles.remove(&(ch.clone(), pid.clone()));
            self.remote_resolutions.remove(&(ch.clone(), pid.clone()));
            self.policy_map.set(&ch, &pid, VIEW_POLICY_HIDDEN);
            self.remote_video_mailbox.remove(&ch, &pid);
            let tile_id = format!("{}:{}", ch, pid);
            self.texture_pool.remove(&format!("remote:{}:{}", ch, pid));
            self.texture_pool.remove(&tile_id);
            self.enabled_grid_preview_tiles.remove(&tile_id);
            if self.focused_tile_id.as_deref() == Some(tile_id.as_str()) {
                self.focused_tile_id = None;
            }
            if self.pip_tile_id.as_deref() == Some(tile_id.as_str()) {
                self.pip_tile_id = None;
            }
            if self.fullscreen_tile_id.as_deref() == Some(tile_id.as_str()) {
                if let Some(c) = ctx {
                    self.exit_fullscreen(c);
                } else {
                    self.fullscreen_tile_id = None;
                }
            }
            if self.open_stats_tile_id.as_deref() == Some(tile_id.as_str()) {
                self.close_stats_overlay();
            }
            let _ = self
                .webrtc_tx
                .send(WebRtcCommand::CleanupPeerChannel { channel: ch, peer_id: pid });
        }
        crate::win_tray::trim_process_working_set();
    }

    pub fn resume_watching_peer(&mut self, channel: String, peer_id: String) {
        self.stopped_remote_tiles
            .remove(&(channel.clone(), peer_id.clone()));
        self.resuming_remote_tiles
            .insert((channel.clone(), peer_id.clone()));
        let _ = self
            .webrtc_tx
            .send(WebRtcCommand::ResumeWatchingPeer { channel, peer_id });
    }

    /// Anuncio ativo no topo respeitando `desktop-app`, visibilidade `online-only` e lista dispensada.
    pub fn active_announcement(&self) -> Option<crate::net::protocol::Announcement> {
        let ann = self.announcement.as_ref()?;
        if !ann.is_valid_banner() {
            return None;
        }
        let shown_once_now = self.announcement_once_id.as_deref() == Some(ann.id.as_str());
        if !shown_once_now
            && self
                .config
                .hidden_announcement_ids
                .iter()
                .any(|id| id == &ann.id)
        {
            return None;
        }
        if ann.visibility.trim().eq_ignore_ascii_case("online-only")
            && self.signaling_status != SignalingStatus::Open
        {
            return None;
        }
        Some(ann.clone())
    }

    pub fn dismiss_announcement(&mut self, id: &str) {
        self.config.hide_announcement_id(id);
        self.config.save();
        if self.announcement.as_ref().is_some_and(|a| a.id == id) {
            self.dismissed_announcement = self.announcement.take();
            self.announcement_live = false;
        }
        if self.announcement_once_id.as_deref() == Some(id) {
            self.announcement_once_id = None;
        }
        let _ = self.sig_tx.send(SignalingCommand::ReportAnnouncementDismiss {
            id: id.to_string(),
        });
    }

    /// Volta a mostrar no topo um aviso dispensado (central de avisos do sino).
    pub fn restore_dismissed_announcement(&mut self) {
        let Some(ann) = self.dismissed_announcement.take() else {
            return;
        };
        self.config.unhide_announcement_id(&ann.id);
        if ann.persistent {
            self.config.persistent_announcement = Some(ann.clone());
        }
        self.config.save();
        self.announcement = Some(ann);
        self.egui_ctx.request_repaint();
    }

    pub fn click_announcement_button(&mut self, ann: &crate::net::protocol::Announcement) {
        let _ = self
            .sig_tx
            .send(SignalingCommand::ReportAnnouncementButtonClick {
                id: ann.id.clone(),
            });
        match ann.button_action.trim() {
            "reload" => {
                // No site o botao recarrega a pagina (= pegar a versao nova). No app nativo:
                // refaz agora status/feeds e procura atualizacao do proprio app.
                let _ = self.meta_wake_tx.send(MetaPollCmd::RefreshNow);
                let _ = self.meta_wake_tx.send(MetaPollCmd::CheckUpdateNow);
                if !ann.persistent {
                    self.announcement_once_id = None;
                    if self.announcement.as_ref().is_some_and(|a| a.id == ann.id) {
                        self.announcement = None;
                    }
                }
                self.egui_ctx.request_repaint();
            }
            _ => {
                crate::ui::external_link::request(ann.button_url_str());
            }
        }
    }

    /// Resolve o card de parceiro considerando horarios locais e fallback oficial GoLive.
    pub fn effective_partner(&self) -> crate::net::protocol::PartnerCardData {
        if self.show_partner_house_override {
            return crate::net::protocol::PartnerCardData::fallback_house_ad();
        }
        let base = self
            .partner_data
            .clone()
            .unwrap_or_else(crate::net::protocol::PartnerCardData::fallback_house_ad);
        let (weekday, minute) = current_local_weekday_and_minute();
        base.resolve_for_local_minute(weekday, minute)
    }

    pub fn effective_points(&self) -> u64 {
        if let Some(ref acc) = self.logged_account {
            acc.points
        } else {
            self.guest_points
        }
    }

    pub fn effective_self_flags(&self) -> &[String] {
        if let Some(ref acc) = self.logged_account {
            &acc.flags
        } else {
            &[]
        }
    }

    pub fn effective_self_name_color(&self) -> Option<&str> {
        self.logged_account
            .as_ref()
            .and_then(|a| a.equipped_name_color.as_deref())
    }

    pub fn effective_self_avatar_url(&self) -> Option<&str> {
        self.logged_account
            .as_ref()
            .and_then(|a| a.avatar_url.as_deref())
    }

    /// Liga a modificacao "Amigos" ao logar (le o grafo uma vez para o numero de pedidos).
    #[cfg(feature = "mod_amigos")]
    pub fn start_amigos(&mut self) {
        if !self.has_modification(MOD_AMIGOS) {
            return;
        }
        let Some(tok) = self.config.account_token.clone() else { return };
        let st = self.amigos.get_or_insert_with(|| Box::new(crate::ui::friends_view::AmigosState::new()));
        st.refresh(&tok, &self.egui_ctx);
    }

    pub fn has_modification(&self, mod_id: &str) -> bool {
        let compiled = match mod_id {
            MOD_AMIGOS => cfg!(feature = "mod_amigos"),
            MOD_FERRAMENTAS => cfg!(feature = "mod_ferramentas"),
            MOD_PERMISSOES_CHAMADA => cfg!(feature = "mod_permissoes_chamada"),
            MOD_PERFIL => cfg!(feature = "mod_perfil"),
            _ => false,
        };
        compiled && self.config.has_modification(mod_id)
    }

    pub fn has_active_media_tools(&self) -> bool {
        #[cfg(feature = "mod_ferramentas")]
        {
            self.has_modification(MOD_FERRAMENTAS)
                && self.room_tools.as_ref().is_some_and(|rt| {
                    rt.tools
                        .iter()
                        .any(|t| crate::net::room_tools::is_media_tool_kind(&t.kind))
                })
        }
        #[cfg(not(feature = "mod_ferramentas"))]
        {
            false
        }
    }

    pub fn is_room_owner(&self) -> bool {
        let Some(ref uid) = self.self_user_id else {
            return false;
        };
        self.room_owner_id.as_deref() == Some(uid.as_str())
    }

    pub fn is_room_manager(&self) -> bool {
        let Some(ref uid) = self.self_user_id else {
            return false;
        };
        self.room_owner_id.as_deref() == Some(uid.as_str())
            || self.room_admins.iter().any(|a| &a.id == uid)
    }

    pub fn is_peer_owner(&self, user_id: Option<&str>) -> bool {
        let Some(uid) = user_id else {
            return false;
        };
        self.room_owner_id.as_deref() == Some(uid)
    }

    pub fn is_peer_admin(&self, user_id: Option<&str>) -> bool {
        let Some(uid) = user_id else {
            return false;
        };
        self.room_admins.iter().any(|a| a.id == uid)
    }

    pub fn is_peer_silenced(&self, user_id: Option<&str>) -> bool {
        let Some(uid) = user_id else {
            return false;
        };
        self.room_silenced.iter().any(|s| s == uid)
    }

    pub fn save_room_info(&mut self, description: Option<String>, category: Option<Option<String>>) {
        let _ = self.sig_tx.send(SignalingCommand::SetRoomInfo {
            description,
            category,
        });
    }

    pub fn set_room_permission(&self, key: &str, allowed: bool) {
        let _ = self.sig_tx.send(SignalingCommand::SetRoomPermission {
            key: key.to_string(),
            allowed,
        });
    }

    pub fn add_room_admin(&self, user_id: &str) {
        let _ = self.sig_tx.send(SignalingCommand::AddRoomAdmin {
            user_id: user_id.to_string(),
        });
    }

    pub fn remove_room_admin(&self, user_id: &str) {
        let _ = self.sig_tx.send(SignalingCommand::RemoveRoomAdmin {
            user_id: user_id.to_string(),
        });
    }

    pub fn set_room_member_limit(&self, limit: Option<u32>) {
        let _ = self.sig_tx.send(SignalingCommand::SetRoomMemberLimit { limit });
    }

    pub fn set_room_capture_protection(&self, enabled: Option<bool>, watermark: Option<bool>) {
        let _ = self
            .sig_tx
            .send(SignalingCommand::SetRoomCaptureProtection { enabled, watermark });
    }

    pub fn request_room_bans(&self) {
        let _ = self.sig_tx.send(SignalingCommand::RequestRoomBans);
    }

    pub fn unban_member(&self, user_id: &str) {
        let _ = self.sig_tx.send(SignalingCommand::RoomUnban {
            user_id: user_id.to_string(),
        });
    }

    pub fn send_tool_json(&self, payload: serde_json::Value) {
        let _ = self.sig_tx.send(SignalingCommand::SendRawJson(payload));
    }

    #[cfg(feature = "mod_perfil")]
    pub fn open_profile_account(&mut self, user_id: String) {
        if !self.has_modification(MOD_PERFIL) || user_id.trim().is_empty() {
            return;
        }
        let Some(ref mut prof) = self.profile_mod else {
            return;
        };
        prof.open_profile_target = Some(ProfileTarget::Account(user_id.clone()));
        prof.profile_error = None;
        prof.profile_loading_id = Some(user_id.clone());

        let ws = signaling_ws_url();
        let tok = self.config.account_token.clone();
        let mtx = self.meta_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new()
            .name("golive-profile".into())
            .spawn(move || {
                let res = fetch_user_profile(&ws, &user_id, tok.as_deref());
                let _ = mtx.send(MetaFeedEvent::UserProfileLoaded(user_id, res));
                ctx.request_repaint();
            })
            .ok();
    }

    #[cfg(feature = "mod_perfil")]
    pub fn open_profile_guest(&mut self, name: String, avatar_url: Option<String>) {
        if !self.has_modification(MOD_PERFIL) {
            return;
        }
        let Some(ref mut prof) = self.profile_mod else {
            return;
        };
        prof.open_profile_target = Some(ProfileTarget::Guest { name, avatar_url });
        prof.profile_loading_id = None;
        prof.profile_error = None;
    }

    #[cfg(feature = "mod_perfil")]
    pub fn open_self_profile(&mut self) {
        if !self.has_modification(MOD_PERFIL) {
            return;
        }
        if let Some(ref acc) = self.logged_account {
            self.open_profile_account(acc.id.clone());
        } else if let Some(ref uid) = self.self_user_id {
            if !uid.starts_with("guest:") {
                self.open_profile_account(uid.clone());
                return;
            }
            self.open_profile_guest(self.effective_name().to_string(), None);
        } else {
            self.open_profile_guest(self.effective_name().to_string(), None);
        }
    }

    #[cfg(feature = "mod_perfil")]
    pub fn close_profile_modal(&mut self) {
        if let Some(ref mut prof) = self.profile_mod {
            prof.open_profile_target = None;
            prof.profile_loading_id = None;
            prof.profile_error = None;
            prof.profile_banner_textures.clear();
            prof.profile_banner_textures.shrink_to_fit();
            prof.profile_banner_in_flight.clear();
        }
        #[cfg(feature = "mod_ferramentas")]
        {
            self.wb_image_textures.clear();
            self.wb_image_textures.shrink_to_fit();
        }
        crate::win_tray::trim_process_working_set();
    }

    #[cfg(feature = "mod_perfil")]
    pub fn resolve_and_load_profile_banner(&mut self, raw_url: Option<&str>) -> Option<egui::TextureId> {
        let raw = raw_url?.trim();
        if raw.is_empty() {
            return None;
        }
        let full = resolve_media_url(raw)?;

        let prof = self.profile_mod.as_mut()?;
        if let Some(tex) = prof.profile_banner_textures.get(&full) {
            return Some(tex.id());
        }
        if prof.profile_banner_textures.len() >= 4 || !prof.profile_banner_in_flight.insert(full.clone()) {
            return None;
        }

        let mtx = self.meta_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new()
            .name("golive-prof-banner".into())
            .spawn(move || {
                let res = crate::net::auth::build_http_agent(Duration::from_secs(5))
                    .and_then(|agent| crate::net::auth::fetch_image_bytes(&agent, &full))
                    .and_then(|bytes| crate::media::image_decode::decode_image_to_rgba(&bytes, 520, 180));
                match res {
                    Ok((width, height, rgba)) => {
                        let _ = mtx.send(MetaFeedEvent::ProfileBannerReady {
                            url: full,
                            width,
                            height,
                            rgba,
                        });
                    }
                    Err(_) => {
                        let _ = mtx.send(MetaFeedEvent::ProfileBannerFailed { url: full });
                    }
                }
                ctx.request_repaint();
            })
            .ok();
        None
    }

    pub fn room_silence_peer(&self, user_id: &str, silenced: bool) {
        let _ = self.sig_tx.send(SignalingCommand::RoomSilence {
            user_id: user_id.to_string(),
            silenced,
        });
    }

    pub fn room_kick_peer(&self, user_id: &str) {
        let _ = self.sig_tx.send(SignalingCommand::RoomKick {
            user_id: user_id.to_string(),
        });
    }

    pub fn room_ban_peer(&self, user_id: &str) {
        let _ = self.sig_tx.send(SignalingCommand::RoomBan {
            user_id: user_id.to_string(),
        });
    }

    pub fn default_avatar_texture_id(&mut self) -> Option<egui::TextureId> {
        if let Some(ref tex) = self.default_avatar_tex {
            return Some(tex.id());
        }
        let bytes = include_bytes!("../assets/default_avatar_01.png");
        let (rw, rh, r_rgba) =
            crate::media::image_decode::decode_image_to_rgba(bytes, 128, 128).ok()?;
        let (w, h, rgba) = crate::media::image_decode::crop_and_mask_circle_rgba(rw, rh, &r_rgba);
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let handle = self.egui_ctx.load_texture(
            "golive_default_avatar_01",
            image,
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.default_avatar_tex = Some(handle);
        Some(id)
    }

    #[cfg(feature = "mod_ferramentas")]
    pub fn get_or_decode_wb_image(&mut self, stroke_id: &str, data_url: &str) -> Option<egui::TextureId> {
        if let Some(tex) = self.wb_image_textures.get(stroke_id) {
            return Some(tex.id());
        }
        let raw = crate::net::room_tools::decode_data_url_image(data_url)?;
        let (w, h, rgba) = crate::media::image_decode::decode_image_to_rgba(&raw, 720, 720).ok()?;
        if self.wb_image_textures.len() >= 24 {
            self.wb_image_textures.clear();
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let handle = self.egui_ctx.load_texture(
            format!("golive_wb_img_{}", stroke_id),
            image,
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.wb_image_textures.insert(stroke_id.to_string(), handle);
        Some(id)
    }

    pub fn resolve_and_load_avatar(&mut self, raw_url: Option<&str>) -> Option<egui::TextureId> {
        let Some(raw) = raw_url.map(str::trim).filter(|s| !s.is_empty()) else {
            return self.default_avatar_texture_id();
        };
        let Some(full) = resolve_media_url(raw) else {
            return self.default_avatar_texture_id();
        };

        if let Some(tex) = self.avatar_textures.get(&full) {
            return Some(tex.id());
        }
        if self.avatar_textures.len() >= 20 || !self.avatar_in_flight.insert(full.clone()) {
            return self.default_avatar_texture_id();
        }

        let mtx = self.meta_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new()
            .name("golive-avatar".into())
            .spawn(move || {
                let res = crate::net::auth::build_http_agent(Duration::from_secs(5))
                    .and_then(|agent| crate::net::auth::fetch_image_bytes(&agent, &full))
                    .and_then(|bytes| crate::media::image_decode::decode_image_to_rgba(&bytes, 96, 96))
                    .map(|(rw, rh, r_rgba)| {
                        crate::media::image_decode::crop_and_mask_circle_rgba(rw, rh, &r_rgba)
                    });
                match res {
                    Ok((width, height, rgba)) => {
                        let _ = mtx.send(MetaFeedEvent::AvatarReady {
                            url: full,
                            width,
                            height,
                            rgba,
                        });
                    }
                    Err(_) => {
                        let _ = mtx.send(MetaFeedEvent::AvatarFailed { url: full });
                    }
                }
                ctx.request_repaint();
            })
            .ok();
        self.default_avatar_texture_id()
    }

    pub fn click_partner_cta(&self, partner: &crate::net::protocol::PartnerCardData) {
        if !partner.is_house_ad() {
            let _ = self.sig_tx.send(SignalingCommand::ReportPartnerClick {
                id: partner.id.clone(),
                source: "card".to_string(),
            });
        }
        if let Some(ref url) = partner.button_url {
            crate::ui::external_link::request(url);
        }
    }

    pub fn room_invite_url(&self) -> String {
        let room = self.current_room.as_deref().unwrap_or("").trim();
        if room.is_empty() {
            "https://golive.nemtudo.me".to_string()
        } else {
            format!("https://golive.nemtudo.me/watch/{}", room)
        }
    }

    pub fn copy_room_invite_url(&mut self, ctx: &egui::Context) {
        let url = self.room_invite_url();
        ctx.copy_text(url);
        self.room_link_copied_until_ms = now_epoch_ms() + 2000;
    }

    fn ensure_partner_images_loaded(&mut self) {
        let partner = self.effective_partner();

        match partner.image_url.as_deref().map(str::trim) {
            Some(url) if crate::net::protocol::is_safe_http_url(url) => {
                if self.partner_banner_url_loaded.as_deref() != Some(url) {
                    self.partner_banner_tex = None;
                    self.spawn_partner_image_fetch(url.to_string(), PartnerImageKind::Banner);
                }
            }
            _ => {
                self.partner_banner_tex = None;
                self.partner_banner_url_loaded = None;
            }
        }

        match partner.icon_url.as_deref().map(str::trim) {
            Some(url) if crate::net::protocol::is_safe_http_url(url) => {
                if self.partner_icon_url_loaded.as_deref() != Some(url) {
                    self.partner_icon_tex = None;
                    self.spawn_partner_image_fetch(url.to_string(), PartnerImageKind::Icon);
                }
            }
            _ => {
                self.partner_icon_tex = None;
                self.partner_icon_url_loaded = None;
            }
        }
    }

    fn spawn_partner_image_fetch(&mut self, url: String, kind: PartnerImageKind) {
        if !self.partner_image_in_flight.insert(url.clone()) {
            return;
        }
        let mtx = self.meta_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new()
            .name("golive-partner-img".into())
            .spawn(move || {
                let Ok(agent) = crate::net::auth::build_http_agent(Duration::from_secs(6)) else {
                    let _ = mtx.send(MetaFeedEvent::PartnerImageFailed { url });
                    return;
                };
                let (max_w, max_h) = match kind {
                    PartnerImageKind::Banner => (520, 260),
                    PartnerImageKind::Icon => (96, 96),
                };
                match crate::net::auth::fetch_image_bytes(&agent, &url)
                    .and_then(|bytes| crate::media::image_decode::decode_image_to_rgba(&bytes, max_w, max_h))
                {
                    Ok((width, height, rgba)) => {
                        let _ = mtx.send(MetaFeedEvent::PartnerImageReady {
                            url,
                            kind,
                            width,
                            height,
                            rgba,
                        });
                    }
                    Err(err) => {
                        crate::diag_warn!("partner", "falha ao carregar imagem do anuncio {}: {}", url, err);
                        let _ = mtx.send(MetaFeedEvent::PartnerImageFailed { url });
                    }
                }
                ctx.request_repaint();
            })
            .ok();
    }

    pub fn is_tile_high_priority(&self, tile_id: &str) -> bool {
        self.focused_tile_id.as_deref() == Some(tile_id)
            || self.fullscreen_tile_id.as_deref() == Some(tile_id)
            || self.pip_tile_id.as_deref() == Some(tile_id)
    }

    pub fn enter_fullscreen(&mut self, tile_id: String, ctx: &egui::Context) {
        self.fullscreen_tile_id = Some(tile_id);
        if !self.was_os_fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            self.was_os_fullscreen = true;
        }
        self.sync_video_render_policies();
    }

    pub fn exit_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen_tile_id = None;
        if self.was_os_fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            self.was_os_fullscreen = false;
        }
        self.sync_video_render_policies();
    }

    pub fn toggle_fullscreen(&mut self, tile_id: String, ctx: &egui::Context) {
        if self.fullscreen_tile_id.as_deref() == Some(tile_id.as_str()) {
            self.exit_fullscreen(ctx);
        } else {
            self.enter_fullscreen(tile_id, ctx);
        }
    }

    pub fn toggle_pip(&mut self, tile_id: String, ctx: &egui::Context) {
        if self.pip_tile_id.as_deref() == Some(tile_id.as_str()) {
            self.pip_tile_id = None;
        } else {
            self.pip_tile_id = Some(tile_id);
            if self.fullscreen_tile_id.is_some() {
                self.exit_fullscreen(ctx);
                return;
            }
        }
        self.sync_video_render_policies();
    }

    pub fn close_pip(&mut self) {
        if self.pip_tile_id.take().is_some() {
            self.sync_video_render_policies();
        }
    }

    pub fn set_window_visible_from_ui(&mut self, ctx: &egui::Context) {
        self.window_hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    pub fn is_tile_preview_enabled(&self, tile_id: &str, is_focused: bool) -> bool {
        should_enable_tile_preview(
            is_focused,
            self.fullscreen_tile_id.as_deref() == Some(tile_id),
            self.pip_tile_id.as_deref() == Some(tile_id),
            self.config.grid_video_preview,
            self.enabled_grid_preview_tiles.contains(tile_id),
        )
    }

    pub fn toggle_tile_preview(&mut self, tile_id: &str) {
        if self.config.grid_video_preview {
            let tiles = self.collect_media_tiles();
            self.config.grid_video_preview = false;
            self.enabled_grid_preview_tiles.clear();
            for t in tiles {
                if t.tile_id != tile_id {
                    self.enabled_grid_preview_tiles.insert(t.tile_id);
                }
            }
            self.config.save();
        } else if !self.enabled_grid_preview_tiles.remove(tile_id) {
            self.enabled_grid_preview_tiles.insert(tile_id.to_string());
        }
        self.sync_video_render_policies();
    }

    pub fn trigger_manual_update_check(&mut self) {
        self.update_state = UpdateDownloadState::Checking;
        self.update_dismissed_session = false;
        let _ = self.meta_wake_tx.send(MetaPollCmd::CheckUpdateNow);
    }

    pub fn start_update_install(&mut self, update: &AvailableUpdate) {
        self.update_state = UpdateDownloadState::Downloading {
            downloaded: 0,
            total: None,
        };
        crate::net::updater::start_update_download_and_apply(
            update.clone(),
            self.update_tx.clone(),
            self.egui_ctx.clone(),
        );
    }

    pub fn set_start_with_windows(&mut self, enabled: bool) {
        self.config.start_with_windows = enabled;
        if let Err(err) = sync_autostart_registry(enabled) {
            self.home_error = Some(err.clone());
            self.room_banner_error = Some(err);
        }
        self.tray
            .set_flags(self.config.start_with_windows, self.config.minimize_to_tray_on_close);
        self.config.save();
    }

    pub fn set_minimize_to_tray_on_close(&mut self, enabled: bool) {
        self.config.minimize_to_tray_on_close = enabled;
        self.tray
            .set_flags(self.config.start_with_windows, self.config.minimize_to_tray_on_close);
        self.config.save();
    }

    pub fn request_real_quit(&mut self, ctx: &egui::Context) {
        self.force_quit_requested = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    pub fn skip_optional_update(&mut self, version: &str) {
        self.config.skipped_update_version = Some(version.to_string());
        self.config.save();
        self.update_dismissed_session = true;
    }

    pub fn sync_video_render_policies(&mut self) {
        if self.window_hidden && self.pip_tile_id.is_none() {
            for w in &self.active_screen_slots {
                w.preview_enabled.store(false, Ordering::Relaxed);
            }
            for peer in &self.peers {
                if peer.is_obs() || peer.is_silent_moderator() {
                    continue;
                }
                self.policy_map.set("screen", &peer.id, VIEW_POLICY_HIDDEN);
                self.policy_map.set("camera", &peer.id, VIEW_POLICY_HIDDEN);
                for &slot in SCREEN_CHANNELS.iter().skip(1) {
                    if self
                        .remote_resolutions
                        .contains_key(&(slot.to_string(), peer.id.clone()))
                    {
                        self.policy_map.set(slot, &peer.id, VIEW_POLICY_HIDDEN);
                    }
                }
                for f in &peer.files {
                    if f.mode != "music" {
                        self.policy_map.set(&f.channel, &peer.id, VIEW_POLICY_HIDDEN);
                    }
                }
            }
            self.texture_pool.clear();
            self.remote_video_mailbox.clear();
            if !self.share_thumb_textures.is_empty() {
                self.clear_share_thumbnails();
            }
            return;
        }

        for w in &self.active_screen_slots {
            let tile_id = format!("self:{}", w.slot);
            let active_view = self.is_tile_high_priority(&tile_id);
            let show = self.is_tile_preview_enabled(&tile_id, active_view);
            w.preview_enabled.store(show, Ordering::Relaxed);
            if !show {
                self.texture_pool.remove(&tile_id);
            }
        }

        let mut seen_remote: Vec<(String, String, bool)> = Vec::new();
        for peer in &self.peers {
            if peer.is_obs() || peer.is_silent_moderator() {
                continue;
            }
            let pid = &peer.id;
            seen_remote.push(("screen".to_string(), pid.clone(), false));
            seen_remote.push(("camera".to_string(), pid.clone(), false));
            for &slot in SCREEN_CHANNELS.iter().skip(1) {
                if self.remote_resolutions.contains_key(&(slot.to_string(), pid.clone())) {
                    seen_remote.push((slot.to_string(), pid.clone(), false));
                }
            }
            for f in &peer.files {
                if f.mode != "music" {
                    seen_remote.push((f.channel.clone(), pid.clone(), true));
                }
            }
        }

        let mut visible_remote_count = 0usize;
        for (ch, pid, is_file) in &seen_remote {
            let tile_id = if *is_file {
                format!("file:{}:{}", ch, pid)
            } else {
                format!("{}:{}", ch, pid)
            };
            let stopped = self.stopped_remote_tiles.contains(&(ch.clone(), pid.clone()));
            if !stopped
                && (self.is_tile_high_priority(&tile_id)
                    || self.config.grid_video_preview
                    || self.enabled_grid_preview_tiles.contains(&tile_id))
            {
                visible_remote_count += 1;
            }
        }

        let grid_policy = if visible_remote_count <= 2 {
            VIEW_POLICY_GRID_LARGE
        } else {
            VIEW_POLICY_GRID_MULTI
        };

        for (ch, pid, is_file) in seen_remote {
            let tile_id = if is_file {
                format!("file:{}:{}", ch, pid)
            } else {
                format!("{}:{}", ch, pid)
            };
            let high_prio = self.is_tile_high_priority(&tile_id);
            let stopped = self.stopped_remote_tiles.contains(&(ch.clone(), pid.clone()));
            let mode = if stopped {
                VIEW_POLICY_HIDDEN
            } else if high_prio {
                VIEW_POLICY_FOCUSED
            } else if self.config.grid_video_preview
                || self.enabled_grid_preview_tiles.contains(&tile_id)
            {
                grid_policy
            } else {
                VIEW_POLICY_HIDDEN
            };
            self.policy_map.set(&ch, &pid, mode);
            if mode == VIEW_POLICY_HIDDEN {
                self.texture_pool.remove(&format!("remote:{}:{}", ch, pid));
            }
        }
    }

    pub fn collect_media_tiles(&self) -> Vec<MediaTileEntry> {
        let mut tiles = Vec::new();

        // 1. Telas locais (Free 1 / Pro multi-tela)
        for (idx, worker) in self.active_screen_slots.iter().enumerate() {
            let tile_id = format!("self:{}", worker.slot);
            tiles.push(MediaTileEntry {
                tile_id,
                texture_key: format!("self:{}", worker.slot),
                title: if idx == 0 {
                    format!("{} (Voce)", self.effective_name())
                } else {
                    format!("{} (Tela {})", self.effective_name(), idx + 1)
                },
                badge: if worker.has_audio {
                    format!("{} • Com som", worker.target_label)
                } else {
                    worker.target_label.clone()
                },
                is_self: true,
                is_stopped_by_us: false,
                is_resuming: false,
                is_ad_paused: false,
                placeholder_note: None,
                channel: Some(worker.slot.clone()),
                peer_id: None,
                volume_key: None,
                resolution_label: Some(format!(
                    "{}x{} {}fps",
                    worker.output_width,
                    worker.output_height,
                    self.current_effective_fps()
                )),
                tool_id: None,
                tool_kind: None,
            });
        }

        // 2. Camera local
        if self.camera_active {
            tiles.push(MediaTileEntry {
                tile_id: "self:camera".to_string(),
                texture_key: "self:camera".to_string(),
                title: format!("{} (Voce)", self.effective_name()),
                badge: "Sua camera".to_string(),
                is_self: true,
                is_stopped_by_us: false,
                is_resuming: false,
                is_ad_paused: false,
                placeholder_note: None,
                channel: Some("camera".to_string()),
                peer_id: None,
                volume_key: None,
                resolution_label: Some("640x360".to_string()),
                tool_id: None,
                tool_kind: None,
            });
        }

        // 3. Todas as midias remotas possiveis de outros participantes (telas, cameras, arquivos locais)
        for peer in &self.peers {
            if peer.is_obs() || peer.is_silent_moderator() {
                continue;
            }
            let user_vol_id = peer.volume_key().to_string();

            // Tela principal ou generica
            let announces_screen =
                peer.screen == Some(true) || (peer.screen.is_none() && peer.sharing && peer.camera != Some(true) && peer.files.is_empty());
            let has_screen_frames = self
                .remote_resolutions
                .contains_key(&("screen".to_string(), peer.id.clone()));
            if announces_screen || has_screen_frames || self.stopped_remote_tiles.contains(&("screen".to_string(), peer.id.clone())) {
                let res = self.remote_resolutions.get(&("screen".to_string(), peer.id.clone()));
                tiles.push(MediaTileEntry {
                    tile_id: format!("screen:{}", peer.id),
                    texture_key: format!("remote:screen:{}", peer.id),
                    title: peer.display_label(),
                    badge: "Tela ao vivo".to_string(),
                    is_self: false,
                    is_stopped_by_us: self.stopped_remote_tiles.contains(&("screen".to_string(), peer.id.clone())),
                    is_resuming: self.resuming_remote_tiles.contains(&("screen".to_string(), peer.id.clone())),
                    is_ad_paused: peer.ad_paused,
                    placeholder_note: if res.is_none() {
                        Some("Aguardando stream de video WebRTC do transmissor...".to_string())
                    } else {
                        None
                    },
                    channel: Some("screen".to_string()),
                    peer_id: Some(peer.id.clone()),
                    volume_key: Some(format!("screen:{}", user_vol_id)),
                    resolution_label: res.map(|(w, h)| format!("{}x{}", w, h)),
                    tool_id: None,
                    tool_kind: None,
                });
            }

            // Telas extras (screen2..screen10) se recebendo frames ou pausadas
            for (idx, &slot) in SCREEN_CHANNELS.iter().enumerate().skip(1) {
                let key = (slot.to_string(), peer.id.clone());
                let res = self.remote_resolutions.get(&key);
                if res.is_some() || self.stopped_remote_tiles.contains(&key) {
                    tiles.push(MediaTileEntry {
                        tile_id: format!("{}:{}", slot, peer.id),
                        texture_key: format!("remote:{}:{}", slot, peer.id),
                        title: format!("{} ({})", peer.display_label(), idx + 1),
                        badge: "Tela extra".to_string(),
                        is_self: false,
                        is_stopped_by_us: self.stopped_remote_tiles.contains(&key),
                        is_resuming: self.resuming_remote_tiles.contains(&key),
                        is_ad_paused: peer.ad_paused,
                        placeholder_note: None,
                        channel: Some(slot.to_string()),
                        peer_id: Some(peer.id.clone()),
                        volume_key: Some(format!("{}:{}", slot, user_vol_id)),
                        resolution_label: res.map(|(w, h)| format!("{}x{}", w, h)),
                        tool_id: None,
                        tool_kind: None,
                    });
                }
            }

            // Camera principal + extras
            let announces_cam = peer.camera == Some(true);
            let cam_key = ("camera".to_string(), peer.id.clone());
            if announces_cam || self.remote_resolutions.contains_key(&cam_key) || self.stopped_remote_tiles.contains(&cam_key) {
                let res = self.remote_resolutions.get(&cam_key);
                tiles.push(MediaTileEntry {
                    tile_id: format!("camera:{}", peer.id),
                    texture_key: format!("remote:camera:{}", peer.id),
                    title: peer.display_label(),
                    badge: "Camera ao vivo".to_string(),
                    is_self: false,
                    is_stopped_by_us: self.stopped_remote_tiles.contains(&cam_key),
                    is_resuming: self.resuming_remote_tiles.contains(&cam_key),
                    is_ad_paused: peer.ad_paused,
                    placeholder_note: if res.is_none() {
                        Some("Aguardando video da camera...".to_string())
                    } else {
                        None
                    },
                    channel: Some("camera".to_string()),
                    peer_id: Some(peer.id.clone()),
                    volume_key: Some(format!("camera:{}", user_vol_id)),
                    resolution_label: res.map(|(w, h)| format!("{}x{}", w, h)),
                    tool_id: None,
                    tool_kind: None,
                });
            }

            // Arquivos locais transmitidos pelo participante (file1..file3)
            for f in &peer.files {
                if f.mode == "music" {
                    continue;
                }
                let fkey = (f.channel.clone(), peer.id.clone());
                let res = self.remote_resolutions.get(&fkey);
                tiles.push(MediaTileEntry {
                    tile_id: format!("file:{}:{}", f.channel, peer.id),
                    texture_key: format!("remote:{}:{}", f.channel, peer.id),
                    title: f.name.clone(),
                    badge: format!("{} enviou arquivo", peer.name),
                    is_self: false,
                    is_stopped_by_us: self.stopped_remote_tiles.contains(&fkey),
                    is_resuming: self.resuming_remote_tiles.contains(&fkey),
                    is_ad_paused: false,
                    placeholder_note: Some(format!(
                        "{} • {:.0}s / {:.0}s",
                        if f.playing { "Reproduzindo" } else { "Pausado" },
                        f.position_seconds,
                        f.duration
                    )),
                    channel: Some(f.channel.clone()),
                    peer_id: Some(peer.id.clone()),
                    volume_key: Some(format!("file:{}:{}", f.channel, user_vol_id)),
                    resolution_label: res.map(|(w, h)| format!("{}x{}", w, h)),
                    tool_id: None,
                    tool_kind: None,
                });
            }
        }

        // 4. Fontes de video sincronizadas da sala (YouTube / Twitch / Kick) - visualizacao leve
        for vs in &self.video_sources {
            let status_txt = if vs.playing { "Reproduzindo" } else { "Pausado" };
            tiles.push(MediaTileEntry {
                tile_id: format!("video-source:{}", vs.id),
                texture_key: format!("vs:{}", vs.id),
                title: format!("{} • {}", vs.kind.to_uppercase(), vs.video_id),
                badge: format!("{} adicionou", vs.added_by_name),
                is_self: false,
                is_stopped_by_us: false,
                is_resuming: false,
                is_ad_paused: false,
                placeholder_note: Some(format!(
                    "Video {} ({}) • Posicao: {:.0}s\n(Reproducao pesada de embed web desativada no Modo Lite)",
                    vs.kind, status_txt, vs.position_seconds
                )),
                channel: None,
                peer_id: None,
                volume_key: None,
                resolution_label: None,
                tool_id: None,
                tool_kind: None,
            });
        }

        // 5. Ferramentas da sala como midia (Calculadora, Roleta, Lousa, Bloco de Notas, Codigo, Documento, Planilha)
        #[cfg(feature = "mod_ferramentas")]
        if self.has_modification(MOD_FERRAMENTAS) {
            if let Some(ref tools) = self.room_tools {
                for tool in &tools.tools {
                    if crate::net::room_tools::is_media_tool_kind(&tool.kind) {
                        tiles.push(MediaTileEntry {
                            tile_id: format!("tool:{}", tool.id),
                            texture_key: format!("tool:{}", tool.id),
                            title: tool.title.clone(),
                            badge: crate::ui::room_tools_modal::kind_badge_upper(&tool.kind)
                                .to_string(),
                            is_self: false,
                            is_stopped_by_us: false,
                            is_resuming: false,
                            is_ad_paused: false,
                            placeholder_note: None,
                            channel: None,
                            peer_id: None,
                            volume_key: None,
                            resolution_label: None,
                            tool_id: Some(tool.id.clone()),
                            tool_kind: Some(tool.kind.clone()),
                        });
                    }
                }
            }
        }

        tiles
    }

    fn current_effective_resolution(&self) -> (u32, u32) {
        if !self.can_use_resolution(self.config.share_resolution) {
            ShareResolution::P1080.dimensions()
        } else {
            self.config.share_resolution.dimensions()
        }
    }

    pub fn current_effective_fps(&self) -> u32 {
        if !self.can_use_fps(self.config.share_fps) {
            60
        } else {
            self.config.share_fps.as_u32()
        }
    }

    fn current_effective_bitrate_kbps(&self) -> u32 {
        if !self.can_use_bitrate(self.config.share_bitrate) {
            ShareBitrate::Ultra.kbps()
        } else {
            self.config.share_bitrate.kbps()
        }
    }

    fn sync_peer_transmission_mixer(&self, peer: &PeerInfo) {
        let vkey = peer.volume_key();
        for ch in ["screen", "camera"] {
            let cfg_key = format!("{}:{}", ch, vkey);
            let mixer_key = format!("{}:{}", ch, peer.id);
            self.mixer.ensure_source_defaults(
                &mixer_key,
                self.config.get_transmission_volume(&cfg_key),
                self.config.is_transmission_muted(&cfg_key),
            );
        }
        for &slot in SCREEN_CHANNELS.iter().skip(1) {
            let cfg_key = format!("{}:{}", slot, vkey);
            let mixer_key = format!("{}:{}", slot, peer.id);
            self.mixer.ensure_source_defaults(
                &mixer_key,
                self.config.get_transmission_volume(&cfg_key),
                self.config.is_transmission_muted(&cfg_key),
            );
        }
        for f in &peer.files {
            let cfg_key = format!("file:{}:{}", f.channel, vkey);
            let mixer_key = format!("{}:{}", f.channel, peer.id);
            self.mixer.ensure_source_defaults(
                &mixer_key,
                self.config.get_transmission_volume(&cfg_key),
                self.config.is_transmission_muted(&cfg_key),
            );
        }
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        // -2. Eventos da bandeja do sistema (tray)
        while let Ok(tev) = self.tray_rx.try_recv() {
            match tev {
                TrayEvent::ShowWindow => {
                    self.window_hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::ToggleAutostart => {
                    let next = !self.config.start_with_windows;
                    self.set_start_with_windows(next);
                }
                TrayEvent::ToggleCloseToTray => {
                    let next = !self.config.minimize_to_tray_on_close;
                    self.set_minimize_to_tray_on_close(next);
                }
                TrayEvent::QuitApp => {
                    self.request_real_quit(ctx);
                }
            }
        }

        // -1. Eventos do atualizador remoto
        while let Ok(uev) = self.update_rx.try_recv() {
            match uev {
                UpdateEvent::CheckFinished { available, manual } => {
                    if let Some(upd) = available {
                        let forced = upd.is_mandatory;
                        self.available_update = Some(upd);
                        self.update_state = UpdateDownloadState::Idle;
                        if forced && self.current_room.is_some() {
                            self.leave_room();
                        }
                    } else {
                        self.available_update = None;
                        if manual {
                            self.update_state = UpdateDownloadState::UpToDate;
                        }
                    }
                }
                UpdateEvent::CheckFailed { error, manual } => {
                    if manual {
                        self.update_state = UpdateDownloadState::Error(error);
                    }
                }
                UpdateEvent::DownloadProgress { downloaded, total } => {
                    self.update_state = UpdateDownloadState::Downloading { downloaded, total };
                }
                UpdateEvent::DownloadReadyToApply => {
                    self.update_state = UpdateDownloadState::ReadyToRestart;
                    self.force_quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                UpdateEvent::DownloadFailed(err) => {
                    self.update_state = UpdateDownloadState::Error(err);
                }
            }
        }

        // 0. Feeds de Top Warns, Contador Online e Anuncios de Parceiros
        while let Ok(ev) = self.meta_rx.try_recv() {
            match ev {
                MetaFeedEvent::SiteStatus(st) => {
                    // Como no site: "nao consegui perguntar" nao apaga o aviso; so uma resposta
                    // real troca o status (senao o aviso some justo quando a rede cai).
                    if st.is_some() {
                        self.site_status = st;
                    }
                }
                MetaFeedEvent::PeopleOnline(count) => {
                    if count.is_some() {
                        self.people_online = count;
                    }
                }
                MetaFeedEvent::Partner(maybe_partner) => {
                    if let Ok(mut guard) = self.partner_current_id.lock() {
                        *guard = maybe_partner.as_ref().map(|p| p.id.clone());
                    }
                    self.partner_data = maybe_partner;
                    self.ensure_partner_images_loaded();
                }
                MetaFeedEvent::PartnerImageReady {
                    url,
                    kind,
                    width,
                    height,
                    rgba,
                } => {
                    self.partner_image_in_flight.remove(&url);
                    let partner = self.effective_partner();
                    let still_matches = match kind {
                        PartnerImageKind::Banner => {
                            partner.image_url.as_deref().map(str::trim) == Some(url.as_str())
                        }
                        PartnerImageKind::Icon => {
                            partner.icon_url.as_deref().map(str::trim) == Some(url.as_str())
                        }
                    };
                    if still_matches {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [width as usize, height as usize],
                            &rgba,
                        );
                        let tex_name = match kind {
                            PartnerImageKind::Banner => "golive_partner_banner",
                            PartnerImageKind::Icon => "golive_partner_icon",
                        };
                        let handle = ctx.load_texture(tex_name, image, egui::TextureOptions::LINEAR);
                        match kind {
                            PartnerImageKind::Banner => {
                                self.partner_banner_tex = Some(handle);
                                self.partner_banner_url_loaded = Some(url);
                            }
                            PartnerImageKind::Icon => {
                                self.partner_icon_tex = Some(handle);
                                self.partner_icon_url_loaded = Some(url);
                            }
                        }
                    }
                }
                MetaFeedEvent::PartnerImageFailed { url } => {
                    self.partner_image_in_flight.remove(&url);
                }
                MetaFeedEvent::GuestPoints(pts) => {
                    self.guest_points = pts;
                }
                MetaFeedEvent::AccountPointsOk(acc) => {
                    self.logged_account = Some(acc);
                }
                MetaFeedEvent::AvatarReady {
                    url,
                    width,
                    height,
                    rgba,
                } => {
                    self.avatar_in_flight.remove(&url);
                    if self.avatar_textures.len() < 32 {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [width as usize, height as usize],
                            &rgba,
                        );
                        let handle = ctx.load_texture(
                            format!("golive_av_{}", self.avatar_textures.len()),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.avatar_textures.insert(url, handle);
                    }
                }
                MetaFeedEvent::AvatarFailed { url } => {
                    self.avatar_in_flight.remove(&url);
                }
                #[cfg(feature = "mod_perfil")]
                MetaFeedEvent::UserProfileLoaded(uid, res) => {
                    if let Some(ref mut prof_st) = self.profile_mod {
                        if prof_st.profile_loading_id.as_deref() == Some(uid.as_str()) {
                            prof_st.profile_loading_id = None;
                        }
                        match res {
                            Ok(prof) => {
                                if prof_st.profile_cache.len() >= 16
                                    && !prof_st.profile_cache.contains_key(&uid)
                                {
                                    if let Some(first) = prof_st.profile_cache.keys().next().cloned() {
                                        prof_st.profile_cache.remove(&first);
                                    }
                                }
                                prof_st.profile_cache.insert(uid, prof);
                                prof_st.profile_error = None;
                            }
                            Err(err) => {
                                prof_st.profile_error = Some(err);
                            }
                        }
                    }
                }
                #[cfg(feature = "mod_perfil")]
                MetaFeedEvent::ProfileBannerReady {
                    url,
                    width,
                    height,
                    rgba,
                } => {
                    if let Some(ref mut prof_st) = self.profile_mod {
                        prof_st.profile_banner_in_flight.remove(&url);
                        if prof_st.profile_banner_textures.len() < 12 {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [width as usize, height as usize],
                                &rgba,
                            );
                            let handle = ctx.load_texture(
                                format!("golive_pban_{}", prof_st.profile_banner_textures.len()),
                                image,
                                egui::TextureOptions::LINEAR,
                            );
                            prof_st.profile_banner_textures.insert(url, handle);
                        }
                    }
                }
                #[cfg(feature = "mod_perfil")]
                MetaFeedEvent::ProfileBannerFailed { url } => {
                    if let Some(ref mut prof_st) = self.profile_mod {
                        prof_st.profile_banner_in_flight.remove(&url);
                    }
                }
                MetaFeedEvent::ShareThumbReady {
                    gen,
                    key,
                    width,
                    height,
                    rgba,
                } => {
                    if self.show_share_settings_modal
                        && self.share_thumb_gen.load(Ordering::Acquire) == gen
                        && width > 0
                        && height > 0
                        && rgba.len() >= width * height * 4
                    {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [width, height],
                            &rgba[..width * height * 4],
                        );
                        let handle = ctx.load_texture(
                            format!("golive_thumb_{}_{}", gen, key),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.share_thumb_textures.insert(key, handle);
                    }
                }
            }
        }

        // Reporta views unicas de anuncio do topo e do parceiro na sidebar via WS
        if self.signaling_status == SignalingStatus::Open {
            if let Some(ann) = self.active_announcement() {
                if self.last_announcement_view_id.as_deref() != Some(ann.id.as_str()) {
                    self.last_announcement_view_id = Some(ann.id.clone());
                    let _ = self
                        .sig_tx
                        .send(SignalingCommand::ReportAnnouncementView { id: ann.id });
                }
            }
            let eff_partner = self.effective_partner();
            if !eff_partner.is_house_ad() {
                if self.last_partner_session_view_id.as_deref() != Some(eff_partner.id.as_str()) {
                    self.last_partner_session_view_id = Some(eff_partner.id.clone());
                    let _ = self.sig_tx.send(SignalingCommand::ReportPartnerSessionView {
                        id: eff_partner.id.clone(),
                    });
                }
                if self.last_partner_view_id.as_deref() != Some(eff_partner.id.as_str())
                    && self.current_room.is_some()
                {
                    self.last_partner_view_id = Some(eff_partner.id.clone());
                    let _ = self.sig_tx.send(SignalingCommand::ReportPartnerView {
                        id: eff_partner.id.clone(),
                    });
                }
            }
        }

        // 1. Auth HTTP results
        while let Ok(res) = self.auth_rx.try_recv() {
            match res {
                AuthTaskResult::LoginOk(tok, acc) => {
                    self.login_in_flight = false;
                    self.login_error = None;
                    self.login_password.clear();
                    self.config.account_token = Some(tok.clone());
                    self.config.save();
                    if let Ok(mut guard) = self.shared_account_token.lock() {
                        *guard = Some(tok.clone());
                    }
                    self.logged_account = Some(acc);
                    #[cfg(feature = "mod_amigos")]
                    self.start_amigos();
                    let ws = signaling_ws_url();
                    let wtx = self.webrtc_tx.clone();
                    std::thread::spawn(move || {
                        let ice = fetch_ice_servers(&ws, Some(&tok));
                        let _ = wtx.send(WebRtcCommand::SetIceServers(ice));
                    });
                }
                AuthTaskResult::LoginErr(e) => {
                    self.login_in_flight = false;
                    self.login_error = Some(e);
                }
                AuthTaskResult::MeOk(acc) => {
                    self.logged_account = Some(acc);
                    #[cfg(feature = "mod_amigos")]
                    self.start_amigos();
                }
                AuthTaskResult::MeErr => {
                    self.config.account_token = None;
                    self.config.save();
                }
            }
        }

        // 2. Recolhe transmissoes cuja janela foi fechada externamente e atualiza preview local
        let finished_slots: Vec<String> = self
            .active_screen_slots
            .iter()
            .filter(|w| w.join_handle.as_ref().is_some_and(|h| h.is_finished()))
            .map(|w| w.slot.clone())
            .collect();
        for slot in finished_slots {
            self.stop_screen_slot(&slot);
        }

        let latest_local: Vec<CapturedScreenTick> = self
            .local_preview_mailbox
            .lock()
            .map(|mut guard| {
                if guard.is_empty() {
                    Vec::new()
                } else {
                    std::mem::take(&mut *guard).into_values().collect()
                }
            })
            .unwrap_or_default();
        let mut any_video_updated = false;
        for tick in latest_local {
            let still_active = (tick.slot == "camera" && self.camera_active)
                || self
                    .active_screen_slots
                    .iter()
                    .any(|w| w.slot == tick.slot);
            if still_active {
                let key = format!("self:{}", tick.slot);
                self.texture_pool
                    .update_texture(ctx, &key, tick.width, tick.height, &tick.rgba);
                any_video_updated = true;
            }
        }

        // 3. WebRTC state events & ultimo frame remoto por tile (mailbox O(1))
        while let Ok(ev) = self.webrtc_rx.try_recv() {
            match ev {
                WebRtcEvent::PeerStoppedSharing { channel, peer_id } => {
                    self.remote_video_mailbox.remove(&channel, &peer_id);
                    self.remote_video_mailbox.shrink_idle();
                    self.texture_pool
                        .remove(&format!("remote:{}:{}", channel, peer_id));
                    self.policy_map.remove(&channel, &peer_id);
                    self.remote_resolutions
                        .remove(&(channel.clone(), peer_id.clone()));
                    self.stopped_remote_tiles
                        .remove(&(channel.clone(), peer_id.clone()));
                    self.resuming_remote_tiles.remove(&(channel, peer_id));
                    if self.remote_resolutions.is_empty() && self.active_screen_slots.is_empty() && !self.camera_active {
                        crate::win_tray::schedule_delayed_memory_trim(Some(ctx.clone()));
                    }
                }
                WebRtcEvent::PeerResuming { channel, peer_id } => {
                    self.resuming_remote_tiles.insert((channel, peer_id));
                }
                WebRtcEvent::StatsUpdated(snap) => {
                    let expected = match &snap {
                        TileStatsSnapshot::Remote(r) => {
                            if r.channel.starts_with("file") {
                                format!("file:{}:{}", r.channel, r.peer_id)
                            } else {
                                format!("{}:{}", r.channel, r.peer_id)
                            }
                        }
                        TileStatsSnapshot::Own(o) => format!("self:{}", o.channel),
                    };
                    if self.open_stats_tile_id.as_deref() == Some(expected.as_str()) {
                        self.active_stats_snapshot = Some(snap);
                    }
                }
            }
        }
        for frame in self.remote_video_mailbox.drain_latest() {
            let key = format!("remote:{}:{}", frame.channel, frame.peer_id);
            let tile_id = if frame.channel.starts_with("file") {
                format!("file:{}:{}", frame.channel, frame.peer_id)
            } else {
                format!("{}:{}", frame.channel, frame.peer_id)
            };
            let active_view = self.is_tile_high_priority(&tile_id);
            if self.is_tile_preview_enabled(&tile_id, active_view) {
                self.texture_pool.update_color_image(
                    ctx,
                    &key,
                    frame.width,
                    frame.height,
                    frame.pixels,
                );
                any_video_updated = true;
            } else {
                self.remote_video_mailbox.recycle_buf(frame.pixels);
            }
            self.remote_resolutions.insert(
                (frame.channel.clone(), frame.peer_id.clone()),
                (frame.orig_width, frame.orig_height),
            );
            self.resuming_remote_tiles
                .remove(&(frame.channel, frame.peer_id));
        }
        if any_video_updated && self.pip_tile_id.is_some() {
            ctx.request_repaint_of(egui::ViewportId::from_hash_of(PIP_VIEWPORT_ID_STR));
        }

        // 4. Signaling events
        while let Ok(ev) = self.sig_rx.try_recv() {
            match ev {
                SignalingEvent::StatusChanged(s) => {
                    self.signaling_status = s;
                }
                SignalingEvent::Registered {
                    self_id,
                    guest_token,
                    ..
                } => {
                    let _ = self
                        .webrtc_tx
                        .send(WebRtcCommand::SetSelfId(self_id.clone()));
                    self.self_id = Some(self_id);
                    if let Some(gt) = guest_token {
                        if let Ok(mut guard) = self.shared_guest_token.lock() {
                            *guard = Some(gt.clone());
                        }
                        self.config.guest_token = Some(gt);
                        self.config.save();
                    }
                }
                SignalingEvent::RegisterError { message, .. } => {
                    self.home_error = Some(message);
                    self.current_room = None;
                }
                SignalingEvent::Banned { reason } => {
                    self.home_error = Some(reason.unwrap_or_else(|| "Voce foi banido.".to_string()));
                    self.leave_room();
                }
                SignalingEvent::RoomState {
                    room,
                    self_id,
                    self_user_id,
                    self_device,
                    peers,
                    messages,
                    video_sources,
                    music,
                    owner_id,
                    admins,
                    silenced,
                    description,
                    category,
                    member_limit,
                    capture_protected,
                    capture_watermark,
                    permissions,
                    ..
                } => {
                    let _ = self
                        .webrtc_tx
                        .send(WebRtcCommand::SetSelfId(self_id.clone()));
                    self.current_room = Some(room);
                    self.self_id = Some(self_id);
                    self.self_user_id = self_user_id;
                    self.self_device_no = self_device;
                    self.room_owner_id = owner_id;
                    self.room_admins = admins;
                    #[cfg(feature = "mod_permissoes_chamada")]
                    if let Some(ref mut m) = self.manage_room_mod {
                        m.room_bans.clear();
                    }
                    self.room_silenced = silenced;
                    self.room_description = description.clone();
                    self.room_description_draft = description;
                    self.room_category = category;
                    self.room_member_limit = member_limit;
                    self.room_capture_protected = capture_protected;
                    self.room_capture_watermark = capture_watermark;
                    self.room_permissions = permissions;
                    if capture_protected {
                        let blocked_room = self.current_room.clone().unwrap_or_default();
                        self.leave_room();
                        self.protected_room_blocked = Some(blocked_room);
                        continue;
                    }
                    self.peers = peers;
                    self.chat_messages = messages;
                    self.video_sources = video_sources;
                    self.room_music = music;
                    self.device_conflict = None;
                    self.room_banner_error = None;

                    // Aplica volumes/mutes salvos para cada participante no mixer
                    for p in &self.peers {
                        let vk = p.volume_key().to_string();
                        self.mixer
                            .set_source_volume(&p.id, self.config.get_peer_volume(&vk));
                        self.mixer
                            .set_source_muted(&p.id, self.config.is_peer_muted(&vk));
                        self.sync_peer_transmission_mixer(p);
                    }

                    // Sincroniza peers WebRTC e re-anuncia audio/mutes locais
                    let peer_ids: Vec<String> = self.peers.iter().map(|p| p.id.clone()).collect();
                    let _ = self
                        .webrtc_tx
                        .send(WebRtcCommand::SyncPeers { peers: peer_ids });
                    let _ = self.sig_tx.send(SignalingCommand::SetMicsMuted(
                        self.mixer.deafened.load(Ordering::Relaxed),
                    ));
                    let _ = self.sig_tx.send(SignalingCommand::SetMic(
                        self.mic.state.enabled.load(Ordering::Relaxed),
                    ));
                    self.announce_sharing();
                }
                SignalingEvent::JoinError { message, .. } => {
                    self.home_error = Some(message);
                    self.current_room = None;
                }
                SignalingEvent::DeviceConflict {
                    devices,
                    max_devices,
                } => {
                    self.device_conflict = Some((devices, max_devices));
                }
                SignalingEvent::CaptchaRequired { message } => {
                    self.home_error = Some(message);
                    self.current_room = None;
                }
                SignalingEvent::PeerJoined(peer) => {
                    let vk = peer.volume_key().to_string();
                    self.mixer
                        .set_source_volume(&peer.id, self.config.get_peer_volume(&vk));
                    self.mixer
                        .set_source_muted(&peer.id, self.config.is_peer_muted(&vk));
                    self.sync_peer_transmission_mixer(&peer);

                    if let Some(existing) = self.peers.iter_mut().find(|p| p.id == peer.id) {
                        *existing = peer;
                    } else {
                        self.peers.push(peer);
                    }
                    let peer_ids: Vec<String> = self.peers.iter().map(|p| p.id.clone()).collect();
                    let _ = self
                        .webrtc_tx
                        .send(WebRtcCommand::SyncPeers { peers: peer_ids });
                }
                SignalingEvent::PeerLeft(id) => {
                    self.peers.retain(|p| p.id != id);
                    self.typing_peer_ids.remove(&id);
                    self.mixer.remove_source(&id);
                    self.texture_pool.remove_peer_textures(&id);
                    self.policy_map.remove_peer(&id);
                    self.remote_resolutions.retain(|(_, p), _| p != &id);
                    self.stopped_remote_tiles.retain(|(_, p)| p != &id);
                    self.resuming_remote_tiles.retain(|(_, p)| p != &id);
                    let suffix = format!(":{}", id);
                    self.enabled_grid_preview_tiles
                        .retain(|tid| !tid.ends_with(&suffix));
                    if self
                        .focused_tile_id
                        .as_deref()
                        .is_some_and(|tid| tid.ends_with(&suffix))
                    {
                        self.focused_tile_id = None;
                    }
                    if self
                        .pip_tile_id
                        .as_deref()
                        .is_some_and(|tid| tid.ends_with(&suffix))
                    {
                        self.pip_tile_id = None;
                    }
                    if self
                        .open_stats_tile_id
                        .as_deref()
                        .is_some_and(|tid| tid.ends_with(&suffix))
                    {
                        self.close_stats_overlay();
                    }
                    let _ = self.webrtc_tx.send(WebRtcCommand::HandleSignal {
                        from: id,
                        data: crate::net::protocol::SignalData {
                            kind: Some("peer-left".to_string()),
                            ..Default::default()
                        },
                    });
                    crate::win_tray::trim_process_working_set();
                }
                SignalingEvent::PeerRenamed {
                    id,
                    name,
                    flags,
                    name_color,
                    avatar_url,
                } => {
                    if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
                        p.name = name;
                        if let Some(f) = flags {
                            p.flags = Some(f);
                        }
                        if name_color.is_some() {
                            p.name_color = name_color;
                        }
                        if avatar_url.is_some() {
                            p.avatar_url = avatar_url;
                        }
                    }
                }
                SignalingEvent::PeerSharing {
                    id,
                    sharing,
                    screen,
                    camera,
                    files,
                    ad_paused,
                } => {
                    if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
                        p.sharing = sharing;
                        p.screen = screen;
                        p.camera = camera;
                        p.files = files;
                        p.ad_paused = ad_paused;
                    }
                    if let Some(p) = self.peers.iter().find(|p| p.id == id).cloned() {
                        let egui_ctx = self.egui_ctx.clone();
                        self.prune_stopped_peer_channels(&p, Some(&egui_ctx));
                        self.sync_peer_transmission_mixer(&p);
                    }
                    self.sync_video_render_policies();
                }
                SignalingEvent::PeerMic { id, mic } => {
                    if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
                        p.mic = mic;
                    }
                }
                SignalingEvent::PeerMicsMuted { id, mics_muted } => {
                    if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
                        p.mics_muted = mics_muted;
                    }
                }
                SignalingEvent::PeerTyping { id, typing } => {
                    if typing {
                        self.typing_peer_ids.insert(id);
                    } else {
                        self.typing_peer_ids.remove(&id);
                    }
                }
                SignalingEvent::ChatMessage(msg) => {
                    self.typing_peer_ids.remove(&msg.from);
                    self.chat_messages.push(msg);
                    if self.chat_messages.len() > 250 {
                        let excess = self.chat_messages.len() - 250;
                        self.chat_messages.drain(..excess);
                    }
                }
                SignalingEvent::ChatDeleted(id) => {
                    self.chat_messages.retain(|m| m.id != id);
                }
                SignalingEvent::ChatBlocked(msg) => {
                    self.chat_blocked_message = Some(msg);
                }
                SignalingEvent::VideoSourceAdded(src) => {
                    self.video_sources.push(src);
                }
                SignalingEvent::VideoSourceRemoved(id) => {
                    self.video_sources.retain(|v| v.id != id);
                }
                SignalingEvent::VideoSourceState {
                    id,
                    playing,
                    position_seconds,
                    playback_rate,
                    updated_at,
                } => {
                    if let Some(vs) = self.video_sources.iter_mut().find(|v| v.id == id) {
                        vs.playing = playing;
                        vs.position_seconds = position_seconds;
                        vs.playback_rate = playback_rate;
                        vs.updated_at = updated_at;
                    }
                }
                SignalingEvent::MusicChanged(m) => {
                    self.room_music = m;
                }
                SignalingEvent::MusicState {
                    id,
                    playing,
                    position_seconds,
                    playback_rate,
                    updated_at,
                } => {
                    if let Some(ref mut m) = self.room_music {
                        if m.id == id {
                            m.playing = playing;
                            m.position_seconds = position_seconds;
                            m.playback_rate = playback_rate;
                            m.updated_at = updated_at;
                        }
                    }
                }
                SignalingEvent::RoomSettings {
                    owner_id,
                    admins,
                    silenced,
                    description,
                    category,
                    member_limit,
                    capture_protected,
                    capture_watermark,
                    permissions,
                } => {
                    self.room_owner_id = owner_id;
                    self.room_admins = admins;
                    self.room_silenced = silenced;
                    if self.room_description != description {
                        self.room_description = description.clone();
                        self.room_description_draft = description;
                    }
                    self.room_category = category;
                    self.room_member_limit = member_limit;
                    self.room_capture_protected = capture_protected;
                    self.room_capture_watermark = capture_watermark;
                    self.room_permissions = permissions;
                    if capture_protected {
                        let blocked_room = self.current_room.clone().unwrap_or_default();
                        self.leave_room();
                        self.protected_room_blocked = Some(blocked_room);
                        continue;
                    }
                }
                #[cfg(feature = "mod_permissoes_chamada")]
                SignalingEvent::RoomBans(bans) => {
                    if let Some(ref mut m) = self.manage_room_mod {
                        m.room_bans = bans;
                    }
                }
                #[cfg(feature = "mod_ferramentas")]
                SignalingEvent::RoomToolMessage(val) => {
                    if !self.has_modification(MOD_FERRAMENTAS) {
                        continue;
                    }
                    let removed_id = if val.get("type").and_then(|v| v.as_str()) == Some("tool-removed") {
                        val.get("id").and_then(|v| v.as_str()).map(str::to_string)
                    } else {
                        None
                    };
                    let self_id = self.self_id.clone();
                    let follow_ups = if let Some(ref mut tools) = self.room_tools {
                        tools.handle_server_message(&val, self_id.as_deref())
                    } else {
                        Vec::new()
                    };
                    for cmd in follow_ups {
                        self.send_tool_json(cmd);
                    }
                    if removed_id.is_some() {
                        let has_wb = self.room_tools.as_ref().is_some_and(|rt| {
                            rt.tools
                                .iter()
                                .any(|t| t.kind == "whiteboard" || t.kind == "annotate")
                        });
                        if !has_wb {
                            self.wb_image_textures.clear();
                            self.wb_image_textures.shrink_to_fit();
                        }
                        crate::win_tray::trim_process_working_set();
                    }
                }
                SignalingEvent::RoomPermissionDenied(msg) => {
                    self.room_banner_error = Some(msg);
                }
                SignalingEvent::RoomRemoved { banned } => {
                    self.home_error = Some(if banned {
                        "Voce foi banido desta sala.".to_string()
                    } else {
                        "Voce foi removido desta sala.".to_string()
                    });
                    self.leave_room();
                }
                SignalingEvent::AnnouncementUpdated { announcement, live } => {
                    match announcement {
                        Some(ann) if ann.is_valid_banner() => {
                            let hidden = self
                                .config
                                .hidden_announcement_ids
                                .iter()
                                .any(|id| id == &ann.id);
                            if !hidden {
                                // Site: um aviso "reload" nao persistente aparece uma vez so —
                                // ja fica marcado como visto, mas continua na tela agora.
                                if !ann.persistent && ann.button_action.trim() == "reload" {
                                    self.config.hide_announcement_id(&ann.id);
                                    self.config.save();
                                    self.announcement_once_id = Some(ann.id.clone());
                                }
                                // Som de aviso uma vez por id (editar o aviso nao toca de novo).
                                if self.announcement_sound_id.as_deref() != Some(ann.id.as_str()) {
                                    let sound = ann.sound.trim();
                                    if sound == "always" || (sound == "live-only" && live) {
                                        play_warning_sound();
                                    }
                                    self.announcement_sound_id = Some(ann.id.clone());
                                }
                                if self.dismissed_announcement.as_ref().is_some_and(|d| d.id == ann.id) {
                                    self.dismissed_announcement = None;
                                }
                                self.announcement = Some(ann.clone());
                                self.announcement_live = live;
                                self.last_announcement_view_id = None;
                                if ann.persistent {
                                    self.config.persistent_announcement = Some(ann);
                                    self.config.save();
                                }
                            }
                        }
                        _ => {
                            self.announcement = None;
                            self.announcement_live = false;
                            if live && self.config.persistent_announcement.is_some() {
                                self.config.persistent_announcement = None;
                                self.config.save();
                            }
                        }
                    }
                }
                SignalingEvent::ServerError(message) => {
                    // Os erros que importam para a pessoa sao os das ferramentas (o resto o
                    // servidor ja trata com mensagens proprias, como join-error).
                    #[cfg(feature = "mod_ferramentas")]
                    if let Some(rt) = self.room_tools.as_mut() {
                        if let Some(text) = crate::ui::room_tools_modal::tool_error_text(&message) {
                            rt.server_error = Some(text);
                            rt.panel_open = true;
                        }
                    }
                    #[cfg(not(feature = "mod_ferramentas"))]
                    let _ = message;
                }
                #[cfg(feature = "mod_amigos")]
                SignalingEvent::SocialUpdate => {
                    if let (Some(st), Some(tok)) = (self.amigos.as_mut(), self.config.account_token.clone()) {
                        st.refresh(&tok, &self.egui_ctx);
                    }
                }
                #[cfg(feature = "mod_amigos")]
                SignalingEvent::PresenceState(entries) => {
                    if let Some(st) = self.amigos.as_mut() {
                        st.apply_presence(entries);
                    }
                }
                #[cfg(not(feature = "mod_amigos"))]
                SignalingEvent::SocialUpdate | SignalingEvent::PresenceState(_) => {}
                SignalingEvent::LiteUpdateCheck => {
                    // Versao nova publicada no admin: checagem silenciosa agora.
                    let _ = self.meta_wake_tx.send(MetaPollCmd::RefreshNow);
                }
                SignalingEvent::PartnerUpdated(maybe_partner) => {
                    if let Ok(mut guard) = self.partner_current_id.lock() {
                        *guard = maybe_partner.as_ref().map(|p| p.id.clone());
                    }
                    self.partner_data = maybe_partner;
                    self.last_partner_view_id = None;
                    self.ensure_partner_images_loaded();
                }
                SignalingEvent::TimeSync { offset_ms } => {
                    self.server_clock_offset_ms = offset_ms;
                }
                SignalingEvent::Signal { from, data } => {
                    let _ = self
                        .webrtc_tx
                        .send(WebRtcCommand::HandleSignal { from, data });
                }
            }
        }
    }
}

impl eframe::App for GoLiveLiteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);

        // Midias fluindo agora, para a amostra de uso das metricas (win_tray::lite_usage_sample).
        // Dois stores atomicos por frame: custo nulo.
        {
            let media_in = self
                .remote_resolutions
                .keys()
                .filter(|k| !self.stopped_remote_tiles.contains(*k))
                .count() as u32;
            let media_out = self.active_screen_slots.len() as u32 + u32::from(self.camera_active);
            crate::win_tray::MEDIA_IN.store(media_in, Ordering::Relaxed);
            crate::win_tray::MEDIA_OUT.store(media_out, Ordering::Relaxed);
        }

        #[cfg(feature = "mod_amigos")]
        if self.amigos.is_some() {
            crate::ui::friends_view::tick(self, ctx);
        }

        // Desenvolvimento: `GOLIVE_LITE_AUTOJOIN=<sala>` entra na sala ao abrir (para prints e
        // testes de layout sem clicar). Lido uma vez; sem a variavel nao faz nada.
        static AUTOJOIN_CHECKED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !AUTOJOIN_CHECKED.swap(true, Ordering::Relaxed) {
            if let Ok(room) = std::env::var("GOLIVE_LITE_AUTOJOIN") {
                let room = room.trim().to_string();
                if !room.is_empty() && self.current_room.is_none() {
                    self.join_room_handle(room.clone());
                    self.config.remove_recent_room(&room);
                    self.config.save();
                }
            }
        }
        // Desenvolvimento (so no build de debug): `GOLIVE_LITE_OPEN_PAGE=amigos` abre a pagina
        // de uma modificacao assim que a conta carrega, para prints sem clicar.
        #[cfg(all(debug_assertions, feature = "mod_amigos"))]
        {
            static OPEN_PAGE_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !OPEN_PAGE_DONE.load(Ordering::Relaxed)
                && std::env::var("GOLIVE_LITE_OPEN_PAGE").is_ok_and(|p| p == "amigos")
            {
                OPEN_PAGE_DONE.store(true, Ordering::Relaxed);
                self.start_amigos();
                self.amigos
                    .get_or_insert_with(|| Box::new(crate::ui::friends_view::AmigosState::new()))
                    .page_open = true;
            }
        }
        // Desenvolvimento: `GOLIVE_LITE_OPEN_PAGE=login` abre a janela de login uma vez.
        #[cfg(debug_assertions)]
        {
            static LOGIN_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if self.logged_account.is_none()
                && std::env::var("GOLIVE_LITE_OPEN_PAGE").is_ok_and(|p| p == "login")
                && !LOGIN_DONE.swap(true, Ordering::Relaxed)
            {
                self.show_login_box = true;
            }
        }
        // Desenvolvimento: `GOLIVE_LITE_OPEN_PAGE=ferramentas[:tipo]` abre o painel de Ferramentas
        // na sala (e, com `:tipo`, tenta criar aquela ferramenta uma vez), para prints e testes.
        #[cfg(all(debug_assertions, feature = "mod_ferramentas"))]
        {
            static TOOLS_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if let Ok(p) = std::env::var("GOLIVE_LITE_OPEN_PAGE") {
                if let Some(rest) = p.strip_prefix("ferramentas") {
                    if self.current_room.is_some()
                        && self.self_id.is_some()
                        && !TOOLS_DONE.load(Ordering::Relaxed)
                    {
                        static SEEN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                        let since = SEEN.get_or_init(std::time::Instant::now).elapsed();
                        ctx.request_repaint_after(std::time::Duration::from_millis(500));
                        if let Some(rt) = self.room_tools.as_mut().filter(|_| since.as_secs() >= 5) {
                            TOOLS_DONE.store(true, Ordering::Relaxed);
                            rt.panel_open = true;
                            if let Some(kind) = rest.strip_prefix(':') {
                                let cmd = crate::net::room_tools::cmd_create_tool(kind, serde_json::json!({ "access": "everyone" }));
                                self.send_tool_json(cmd);
                            }
                        }
                    }
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested())
            && self.config.minimize_to_tray_on_close
            && !self.force_quit_requested
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            hide_main_window_win32();
            self.window_hidden = true;
            crate::win_tray::trim_process_working_set();
        }

        render_optional_update_banner(self, ctx);
        if self.current_room.is_some() {
            render_room_view(self, ctx);
        } else {
            render_home_view(self, ctx);
        }
        render_mandatory_update_overlay(self, ctx);
        crate::ui::external_link::render(ctx);
        self.sync_video_render_policies();

        // Economia maxima de CPU: em repouso ou escondido na bandeja nao dispara repaint continuo.
        #[cfg(feature = "mod_ferramentas")]
        let tools_anim = !self.window_hidden
            && self.has_modification(MOD_FERRAMENTAS)
            && self
                .room_tools
                .as_ref()
                .is_some_and(|rt| rt.has_active_wheel_spin() || rt.has_active_reactions());
        #[cfg(not(feature = "mod_ferramentas"))]
        let tools_anim = false;
        if tools_anim {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if !self.window_hidden
            && (self.mic.state.enabled.load(Ordering::Relaxed)
                || self.stats_copied_until_ms > now_epoch_ms()
                || self.room_link_copied_until_ms > now_epoch_ms())
        {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray.shutdown();
        for w in &self.active_screen_slots {
            w.stop_flag.store(true, Ordering::Release);
        }
        if let Some(ref flag) = self.camera_stop_flag {
            flag.store(true, Ordering::Release);
        }
        self.mic.stop();
        let _ = self.webrtc_tx.send(WebRtcCommand::CloseAll);
    }
}

pub(crate) fn should_enable_tile_preview(
    is_focused: bool,
    is_fullscreen: bool,
    is_pip: bool,
    grid_video_preview: bool,
    explicitly_enabled: bool,
) -> bool {
    is_focused || is_fullscreen || is_pip || grid_video_preview || explicitly_enabled
}

pub(crate) fn find_free_screen_slot(
    active_slots: &[&str],
    max_screens: usize,
) -> Option<&'static str> {
    if active_slots.len() >= max_screens {
        return None;
    }
    SCREEN_CHANNELS
        .iter()
        .copied()
        .find(|cand| !active_slots.contains(cand))
}

pub(crate) fn is_peer_video_channel_active(peer: &PeerInfo, channel: &str) -> bool {
    if SCREEN_CHANNELS.contains(&channel) {
        peer.screen.unwrap_or(peer.sharing)
    } else if channel == "camera" {
        peer.camera.unwrap_or(false)
    } else {
        peer.files
            .iter()
            .any(|f| f.channel == channel && f.mode != "music")
    }
}

pub(crate) fn resolve_media_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with("data:") || trimmed.starts_with("javascript:") {
        return None;
    }
    if crate::net::protocol::is_safe_http_url(trimmed) {
        Some(trimmed.to_string())
    } else if trimmed.starts_with("//") {
        Some(format!("https:{}", trimmed))
    } else if trimmed.starts_with('/') {
        Some(format!("https://golive.nemtudo.me{}", trimmed))
    } else if !trimmed.contains(':') {
        Some(format!(
            "https://golive.nemtudo.me/{}",
            trimmed.trim_start_matches('/')
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{find_free_screen_slot, should_enable_tile_preview};

    #[test]
    fn test_individual_slot_allocation_and_freeing() {
        assert_eq!(find_free_screen_slot(&[], 2), Some("screen"));
        assert_eq!(find_free_screen_slot(&["screen"], 2), Some("screen2"));
        assert_eq!(find_free_screen_slot(&["screen", "screen2"], 2), None);
        // Parar a primeira tela ("screen") mantem "screen2" viva e libera "screen"
        assert_eq!(find_free_screen_slot(&["screen2"], 2), Some("screen"));
    }

    #[test]
    fn test_should_enable_tile_preview_for_fullscreen_and_pip() {
        assert!(!should_enable_tile_preview(false, false, false, false, false));
        assert!(should_enable_tile_preview(false, true, false, false, false));
        assert!(should_enable_tile_preview(false, false, true, false, false));
        assert!(should_enable_tile_preview(true, false, false, false, false));
        assert!(should_enable_tile_preview(false, false, false, true, false));
        assert!(should_enable_tile_preview(false, false, false, false, true));
    }

    #[test]
    fn test_is_peer_video_channel_active_and_resolve_media_url() {
        let mut peer = crate::net::protocol::PeerInfo::default();
        peer.id = "p1".to_string();
        peer.screen = Some(true);
        peer.sharing = true;
        assert!(super::is_peer_video_channel_active(&peer, "screen"));
        assert!(super::is_peer_video_channel_active(&peer, "screen2"));
        assert!(!super::is_peer_video_channel_active(&peer, "camera"));

        // Quando o transmissor para a tela, canais screen/screen2 devem ser podados
        peer.screen = Some(false);
        peer.sharing = false;
        assert!(!super::is_peer_video_channel_active(&peer, "screen"));
        assert!(!super::is_peer_video_channel_active(&peer, "screen2"));

        assert_eq!(
            super::resolve_media_url("/uploads/banner.png").as_deref(),
            Some("https://golive.nemtudo.me/uploads/banner.png")
        );
        assert_eq!(
            super::resolve_media_url("banners/custom.webp").as_deref(),
            Some("https://golive.nemtudo.me/banners/custom.webp")
        );
        assert_eq!(
            super::resolve_media_url("//cdn.example.com/a.png").as_deref(),
            Some("https://cdn.example.com/a.png")
        );
        assert!(super::resolve_media_url("javascript:alert(1)").is_none());
    }
}

/// `playWarningSound` do site: o som de aviso do sistema (assincrono, sem decodificar audio
/// nem segurar buffer — o Windows toca e volta na hora).
fn play_warning_sound() {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBeep(u_type: u32) -> i32;
        }
        const MB_ICONWARNING: u32 = 0x30;
        // SAFETY: MessageBeep so recebe um inteiro e nao toca em memoria nossa.
        unsafe {
            MessageBeep(MB_ICONWARNING);
        }
    }
}

fn spawn_meta_feed_poller(
    ws_url: String,
    tx: mpsc::UnboundedSender<MetaFeedEvent>,
    update_tx: mpsc::UnboundedSender<UpdateEvent>,
    wake_rx: std::sync::mpsc::Receiver<MetaPollCmd>,
    current_partner_id: Arc<std::sync::Mutex<Option<String>>>,
    account_token: Arc<std::sync::Mutex<Option<String>>>,
    guest_token: Arc<std::sync::Mutex<Option<String>>>,
    ctx: egui::Context,
) {
    let http_base = crate::net::protocol::signaling_http_base(&ws_url);
    let update_url = crate::net::updater::update_manifest_url();
    std::thread::Builder::new()
        .name("golive-meta-feed".into())
        .spawn(move || {
            let Ok(agent) = crate::net::auth::build_http_agent(Duration::from_secs(6)) else {
                return;
            };
            let startup = std::time::Instant::now();
            let mut next_status = startup;
            let mut next_stats = startup;
            let mut next_partner = startup;
            let mut next_points = startup + Duration::from_secs(3);
            let mut next_update = startup + Duration::from_secs(5);
            let mut update_etag: Option<String> = None;

            loop {
                let now = std::time::Instant::now();
                let mut updated = false;

                if now >= next_status {
                    let st = crate::net::auth::fetch_site_status(&agent);
                    let _ = tx.send(MetaFeedEvent::SiteStatus(st));
                    // Mesmo intervalo do site (useSiteStatus: 60s).
                    next_status = now + Duration::from_secs(60);
                    updated = true;
                }

                if now >= next_stats {
                    let online = crate::net::auth::fetch_people_online(&agent, &http_base);
                    if online.is_some() {
                        let _ = tx.send(MetaFeedEvent::PeopleOnline(online));
                        updated = true;
                    }
                    next_stats = now + Duration::from_secs(45);
                }

                if now >= next_partner {
                    let cur = current_partner_id.lock().ok().and_then(|g| g.clone());
                    if let Some(maybe_partner) =
                        crate::net::auth::fetch_partner_ad(&agent, &http_base, cur.as_deref())
                    {
                        let _ = tx.send(MetaFeedEvent::Partner(maybe_partner));
                        updated = true;
                    }
                    next_partner = now + Duration::from_secs(180);
                }

                if now >= next_points {
                    let acc_tok = account_token.lock().ok().and_then(|g| g.clone());
                    if let Some(tok) = acc_tok {
                        if let Ok(acc) = crate::net::auth::fetch_me(&ws_url, &tok) {
                            let _ = tx.send(MetaFeedEvent::AccountPointsOk(acc));
                            updated = true;
                        }
                    } else if let Some(gt) = guest_token.lock().ok().and_then(|g| g.clone()) {
                        if let Some(pts) =
                            crate::net::auth::fetch_guest_points(&agent, &http_base, &gt)
                        {
                            let _ = tx.send(MetaFeedEvent::GuestPoints(pts));
                            updated = true;
                        }
                    }
                    next_points = now + Duration::from_secs(180);
                }

                let mut force_update_manual = false;
                let due_periodic_update = now >= next_update;
                if due_periodic_update {
                    run_update_check(
                        &agent,
                        &update_url,
                        &mut update_etag,
                        false,
                        &update_tx,
                    );
                    next_update = now + Duration::from_secs(6 * 3600);
                    updated = true;
                }

                if updated {
                    ctx.request_repaint();
                }

                let next_due = [next_status, next_stats, next_partner, next_points, next_update]
                    .into_iter()
                    .min()
                    .unwrap_or(now + Duration::from_secs(45));
                let sleep_dur = next_due
                    .saturating_duration_since(std::time::Instant::now())
                    .clamp(Duration::from_secs(1), Duration::from_secs(60));

                match wake_rx.recv_timeout(sleep_dur) {
                    Ok(MetaPollCmd::RefreshNow) => {
                        let now = std::time::Instant::now();
                        next_status = now;
                        next_stats = now;
                        next_partner = now;
                        next_update = now;
                    }
                    Ok(MetaPollCmd::CheckUpdateNow) => {
                        force_update_manual = true;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }

                if force_update_manual {
                    run_update_check(
                        &agent,
                        &update_url,
                        &mut update_etag,
                        true,
                        &update_tx,
                    );
                    next_update = std::time::Instant::now() + Duration::from_secs(6 * 3600);
                    ctx.request_repaint();
                }
            }
        })
        .ok();
}

fn run_update_check(
    agent: &ureq::Agent,
    update_url: &str,
    etag: &mut Option<String>,
    manual: bool,
    update_tx: &mpsc::UnboundedSender<UpdateEvent>,
) {
    let req_etag = if manual { None } else { etag.as_deref() };
    match crate::net::updater::fetch_update_manifest(agent, update_url, req_etag) {
        Ok(Some((manifest, new_etag))) => {
            if new_etag.is_some() {
                *etag = new_etag;
            }
            let available =
                crate::net::updater::evaluate_update(&manifest, crate::net::updater::APP_VERSION);
            let _ = update_tx.send(UpdateEvent::CheckFinished { available, manual });
        }
        Ok(None) => {
            if manual {
                let _ = update_tx.send(UpdateEvent::CheckFinished {
                    available: None,
                    manual: true,
                });
            }
        }
        Err(err) => {
            let _ = update_tx.send(UpdateEvent::CheckFailed { error: err, manual });
        }
    }
}
