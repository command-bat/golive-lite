use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[cfg(feature = "mod_permissoes_chamada")]
use crate::net::protocol::RoomBan;
use crate::net::protocol::{
    ws_url_with_locale, Announcement, ChatMessage, ChatReplyTo, MusicSource, PartnerCardData,
    PeerInfo, RegisteredAccount, RoomAdmin, RoomPermissions, SignalData, VideoSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingStatus {
    Idle,
    Connecting,
    Open,
    Closed,
    Superseded,
    Banned,
}

impl SignalingStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Desconectado",
            Self::Connecting => "Conectando...",
            Self::Open => "Conectado",
            Self::Closed => "Reconectando...",
            Self::Superseded => "Sessao assumida por outro dispositivo",
            Self::Banned => "Conexao bloqueada",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SignalingCommand {
    Connect {
        ws_url: String,
        name: String,
        client_id: String,
        token: Option<String>,
        install_id: String,
        device_id: String,
    },
    JoinRoom {
        room: String,
        confirm_device: bool,
    },
    ConfirmDeviceJoin,
    LeaveRoom,
    SendChat {
        text: String,
        reply_to: Option<ChatReplyTo>,
    },
    SendTyping(bool),
    SetMic(bool),
    SetMicsMuted(bool),
    SetSharing {
        screen: bool,
        camera: bool,
        screens: usize,
        cameras: usize,
    },
    SendSignal {
        to: String,
        data: SignalData,
    },
    ReportAnnouncementView {
        id: String,
    },
    ReportAnnouncementButtonClick {
        id: String,
    },
    ReportAnnouncementDismiss {
        id: String,
    },
    ReportPartnerView {
        id: String,
    },
    ReportPartnerSessionView {
        id: String,
    },
    ReportPartnerClick {
        id: String,
        source: String,
    },
    SetRoomInfo {
        description: Option<String>,
        category: Option<Option<String>>,
    },
    RoomSilence {
        user_id: String,
        silenced: bool,
    },
    RoomKick {
        user_id: String,
    },
    RoomBan {
        user_id: String,
    },
    RoomUnban {
        user_id: String,
    },
    RequestRoomBans,
    SetRoomPermission {
        key: String,
        allowed: bool,
    },
    AddRoomAdmin {
        user_id: String,
    },
    RemoveRoomAdmin {
        user_id: String,
    },
    SetRoomMemberLimit {
        limit: Option<u32>,
    },
    SetRoomCaptureProtection {
        enabled: Option<bool>,
        watermark: Option<bool>,
    },
    SendRawJson(Value),
    Disconnect,
}

#[derive(Debug, Clone)]
pub enum SignalingEvent {
    StatusChanged(SignalingStatus),
    Registered {
        self_id: String,
        name: String,
        account: Option<RegisteredAccount>,
        guest_token: Option<String>,
    },
    RegisterError {
        message: String,
        retryable: bool,
    },
    Banned {
        reason: Option<String>,
    },
    RoomState {
        room: String,
        self_id: String,
        self_user_id: Option<String>,
        self_device: Option<u32>,
        peers: Vec<PeerInfo>,
        messages: Vec<ChatMessage>,
        video_sources: Vec<VideoSource>,
        music: Option<MusicSource>,
        owner_id: Option<String>,
        admins: Vec<RoomAdmin>,
        silenced: Vec<String>,
        description: String,
        category: Option<String>,
        member_limit: Option<u32>,
        capture_protected: bool,
        capture_watermark: bool,
        permissions: RoomPermissions,
        reclaimed: bool,
    },
    JoinError {
        reason: String,
        message: String,
    },
    DeviceConflict {
        devices: u32,
        max_devices: u32,
    },
    CaptchaRequired {
        message: String,
    },
    PeerJoined(PeerInfo),
    PeerLeft(String),
    PeerRenamed {
        id: String,
        name: String,
        flags: Option<Vec<String>>,
        name_color: Option<String>,
        avatar_url: Option<String>,
    },
    PeerSharing {
        id: String,
        sharing: bool,
        screen: Option<bool>,
        camera: Option<bool>,
        files: Vec<crate::net::protocol::SharedFile>,
        ad_paused: bool,
    },
    PeerMic {
        id: String,
        mic: bool,
    },
    PeerMicsMuted {
        id: String,
        mics_muted: bool,
    },
    PeerTyping {
        id: String,
        typing: bool,
    },
    ChatMessage(ChatMessage),
    ChatDeleted(String),
    ChatBlocked(String),
    VideoSourceAdded(VideoSource),
    VideoSourceRemoved(String),
    VideoSourceState {
        id: String,
        playing: bool,
        position_seconds: f64,
        playback_rate: f64,
        updated_at: u64,
    },
    MusicChanged(Option<MusicSource>),
    MusicState {
        id: String,
        playing: bool,
        position_seconds: f64,
        playback_rate: f64,
        updated_at: u64,
    },
    RoomSettings {
        owner_id: Option<String>,
        admins: Vec<RoomAdmin>,
        silenced: Vec<String>,
        description: String,
        category: Option<String>,
        member_limit: Option<u32>,
        capture_protected: bool,
        capture_watermark: bool,
        permissions: RoomPermissions,
    },
    #[cfg(feature = "mod_permissoes_chamada")]
    RoomBans(Vec<RoomBan>),
    #[cfg(feature = "mod_ferramentas")]
    RoomToolMessage(Value),
    RoomPermissionDenied(String),
    RoomRemoved {
        banned: bool,
    },
    TimeSync {
        offset_ms: i64,
    },
    AnnouncementUpdated {
        announcement: Option<Announcement>,
        live: bool,
    },
    PartnerUpdated(Option<PartnerCardData>),
    /// O admin publicou uma versao nova do Go Live - Lite: checar agora, sem esperar 6h.
    LiteUpdateCheck,
    /// Mod Amigos: o grafo social mudou (pedido recebido, aceito, removido...). O app relê `/social`.
    SocialUpdate,
    /// `{type:"error", message}` do servidor (ex.: "Your plan does not include this tool.").
    ServerError(String),
    /// Mod Amigos: presença das contas observadas (`presence-watch`): id -> (estado, aparelho).
    PresenceState(Vec<(String, String, Option<String>)>),
    Signal {
        from: String,
        data: SignalData,
    },
}

struct SessionState {
    ws_url: String,
    name: String,
    client_id: String,
    token: Option<String>,
    install_id: String,
    device_id: String,
    desired_room: Option<String>,
    confirmed_device: bool,
    captcha_pass: Option<String>,
    registered_once: bool,
    should_run: bool,
    reconnect_attempts: u32,
    mic_on: bool,
    mics_muted: bool,
    sharing_screen: bool,
    sharing_camera: bool,
    screens_count: usize,
    cameras_count: usize,
    enable_tools_mod: bool,
    enable_perms_mod: bool,
}

impl SessionState {
    fn new(enable_tools_mod: bool, enable_perms_mod: bool) -> Self {
        Self {
            ws_url: String::new(),
            name: String::new(),
            client_id: String::new(),
            token: None,
            install_id: String::new(),
            device_id: String::new(),
            desired_room: None,
            confirmed_device: false,
            captcha_pass: None,
            registered_once: false,
            should_run: false,
            reconnect_attempts: 0,
            mic_on: false,
            mics_muted: false,
            sharing_screen: false,
            sharing_camera: false,
            screens_count: 0,
            cameras_count: 0,
            enable_tools_mod,
            enable_perms_mod,
        }
    }
}

pub fn spawn_signaling_actor(
    rt: &tokio::runtime::Runtime,
    egui_ctx: egui::Context,
    enable_tools_mod: bool,
    enable_perms_mod: bool,
) -> (
    mpsc::UnboundedSender<SignalingCommand>,
    mpsc::UnboundedReceiver<SignalingEvent>,
) {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SignalingCommand>();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel::<SignalingEvent>();

    rt.spawn(async move {
        let mut session = SessionState::new(enable_tools_mod, enable_perms_mod);
        let emit = |ev: SignalingEvent| {
            let _ = evt_tx.send(ev);
            egui_ctx.request_repaint();
        };

        loop {
            while !session.should_run {
                let Some(cmd) = cmd_rx.recv().await else {
                    return;
                };
                handle_pre_connect_cmd(cmd, &mut session);
            }

            emit(SignalingEvent::StatusChanged(SignalingStatus::Connecting));
            let url = ws_url_with_locale(&session.ws_url);
            let conn = connect_async(&url).await;

            let ws_stream = match conn {
                Ok((stream, _)) => {
                    crate::diag_info!("signaling", "WebSocket conectado em {}", url);
                    stream
                }
                Err(e) => {
                    crate::diag_warn!("signaling", "falha ao conectar WebSocket ({}): {}", url, e);
                    emit(SignalingEvent::StatusChanged(SignalingStatus::Closed));
                    if wait_reconnect_or_cmd(&mut cmd_rx, &mut session).await {
                        continue;
                    } else {
                        emit(SignalingEvent::StatusChanged(SignalingStatus::Idle));
                        continue;
                    }
                }
            };

            session.reconnect_attempts = 0;
            session.registered_once = false;
            emit(SignalingEvent::StatusChanged(SignalingStatus::Open));

            let (mut ws_write, mut ws_read) = ws_stream.split();

            // Envia time-sync + register inicial
            let _ = ws_write
                .send(Message::Text(
                    json!({ "type": "time-sync", "t0": now_epoch_ms() }).to_string(),
                ))
                .await;
            let reg_payload = build_register_payload(&session);
            let _ = ws_write.send(Message::Text(reg_payload.to_string())).await;

            // Keepalive de 15s + select `biased` priorizando leitura para responder Ping do servidor imediatamente
            let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
            ping_interval.tick().await;
            // Amostra de uso (RAM/CPU/midias) para as metricas do Lite no admin: 3 min depois de
            // conectar e a cada 10 min (40 ticks de 15 s). Custo: uma leitura de contadores do SO.
            let mut ping_ticks: u32 = 0;

            let mut close_code: Option<u16> = None;

            loop {
                tokio::select! {
                    biased;
                    maybe_msg = ws_read.next() => {
                        match maybe_msg {
                            Some(Ok(Message::Ping(data))) => {
                                let _ = ws_write.send(Message::Pong(data)).await;
                            }
                            Some(Ok(Message::Text(txt))) => {
                                if let Ok(val) = serde_json::from_str::<Value>(&txt) {
                                    if let Some(reply) = handle_server_message(&val, &mut session, &emit) {
                                        let _ = ws_write.send(Message::Text(reply.to_string())).await;
                                    }
                                }
                            }
                            Some(Ok(Message::Close(frame))) => {
                                close_code = frame.as_ref().map(|f| f.code.into());
                                crate::diag_warn!(
                                    "signaling",
                                    "WebSocket fechado pelo servidor code={:?} reason='{}'",
                                    close_code,
                                    frame.as_ref().map(|f| f.reason.as_ref()).unwrap_or("")
                                );
                                break;
                            }
                            Some(Err(e)) => {
                                crate::diag_warn!("signaling", "erro de leitura WebSocket: {}", e);
                                break;
                            }
                            None => {
                                crate::diag_warn!("signaling", "stream WebSocket encerrada (EOF)");
                                break;
                            }
                            _ => {}
                        }
                    }
                    maybe_cmd = cmd_rx.recv() => {
                        let Some(cmd) = maybe_cmd else {
                            return;
                        };
                        match cmd {
                            SignalingCommand::Disconnect => {
                                session.should_run = false;
                                session.desired_room = None;
                                let _ = ws_write.send(Message::Text(json!({ "type": "leave" }).to_string())).await;
                                let _ = ws_write.close().await;
                                emit(SignalingEvent::StatusChanged(SignalingStatus::Idle));
                                break;
                            }
                            SignalingCommand::Connect { ws_url, name, client_id, token, install_id, device_id } => {
                                let url_changed = ws_url != session.ws_url;
                                session.ws_url = ws_url;
                                session.name = name;
                                session.client_id = client_id;
                                session.token = token;
                                session.install_id = install_id;
                                session.device_id = device_id;
                                session.should_run = true;
                                if url_changed {
                                    let _ = ws_write.close().await;
                                    break;
                                } else {
                                    let reg = build_register_payload(&session);
                                    let _ = ws_write.send(Message::Text(reg.to_string())).await;
                                }
                            }
                            SignalingCommand::JoinRoom { room, confirm_device } => {
                                session.desired_room = Some(room.clone());
                                session.confirmed_device = confirm_device;
                                if session.registered_once {
                                    let msg = build_join_payload(&room, confirm_device, session.captcha_pass.as_deref());
                                    let _ = ws_write.send(Message::Text(msg.to_string())).await;
                                }
                            }
                            SignalingCommand::ConfirmDeviceJoin => {
                                session.confirmed_device = true;
                                if let Some(room) = session.desired_room.clone() {
                                    let msg = build_join_payload(&room, true, session.captcha_pass.as_deref());
                                    let _ = ws_write.send(Message::Text(msg.to_string())).await;
                                }
                            }
                            SignalingCommand::LeaveRoom => {
                                session.desired_room = None;
                                session.confirmed_device = false;
                                session.sharing_screen = false;
                                session.sharing_camera = false;
                                session.screens_count = 0;
                                session.cameras_count = 0;
                                let _ = ws_write.send(Message::Text(json!({ "type": "leave" }).to_string())).await;
                            }
                            SignalingCommand::SendChat { text, reply_to } => {
                                let trimmed = text.trim();
                                if !trimmed.is_empty() {
                                    let mut payload = json!({ "type": "chat", "text": trimmed });
                                    if let Some(rt_obj) = reply_to {
                                        if let Ok(v) = serde_json::to_value(rt_obj) {
                                            payload["replyTo"] = v;
                                        }
                                    }
                                    let _ = ws_write.send(Message::Text(payload.to_string())).await;
                                }
                            }
                            SignalingCommand::SendTyping(typing) => {
                                let _ = ws_write.send(Message::Text(json!({ "type": "typing", "typing": typing }).to_string())).await;
                            }
                            SignalingCommand::SetMic(mic) => {
                                session.mic_on = mic;
                                let _ = ws_write.send(Message::Text(json!({ "type": "mic", "mic": mic }).to_string())).await;
                            }
                            SignalingCommand::SetMicsMuted(muted) => {
                                session.mics_muted = muted;
                                let _ = ws_write.send(Message::Text(json!({ "type": "mics-muted", "muted": muted }).to_string())).await;
                            }
                            SignalingCommand::SetSharing { screen, camera, screens, cameras } => {
                                session.sharing_screen = screen;
                                session.sharing_camera = camera;
                                session.screens_count = screens;
                                session.cameras_count = cameras;
                                let payload = build_sharing_payload(screen, camera, screens, cameras);
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::SendSignal { to, data } => {
                                let payload = json!({
                                    "type": "signal",
                                    "to": to,
                                    "data": data,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::SetRoomInfo { description, category } => {
                                let mut payload = json!({ "type": "room-info-set" });
                                if let Some(desc) = description {
                                    payload["description"] = Value::String(desc);
                                }
                                if let Some(cat) = category {
                                    payload["category"] = match cat {
                                        Some(c) => Value::String(c),
                                        None => Value::Null,
                                    };
                                }
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::RoomSilence { user_id, silenced } => {
                                let payload = json!({
                                    "type": "room-silence",
                                    "userId": user_id,
                                    "silenced": silenced,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::RoomKick { user_id } => {
                                let payload = json!({
                                    "type": "room-kick",
                                    "userId": user_id,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::RoomBan { user_id } => {
                                let payload = json!({
                                    "type": "room-ban",
                                    "userId": user_id,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::RoomUnban { user_id } => {
                                let payload = json!({
                                    "type": "room-unban",
                                    "userId": user_id,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::RequestRoomBans => {
                                let payload = json!({ "type": "room-bans" });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::SetRoomPermission { key, allowed } => {
                                let mut perms = serde_json::Map::new();
                                perms.insert(key, Value::Bool(allowed));
                                let payload = json!({
                                    "type": "room-permissions-set",
                                    "permissions": Value::Object(perms),
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::AddRoomAdmin { user_id } => {
                                let payload = json!({
                                    "type": "room-admin-add",
                                    "userId": user_id,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::RemoveRoomAdmin { user_id } => {
                                let payload = json!({
                                    "type": "room-admin-remove",
                                    "userId": user_id,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::SetRoomMemberLimit { limit } => {
                                let payload = json!({
                                    "type": "room-member-limit",
                                    "limit": limit,
                                });
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            SignalingCommand::SetRoomCaptureProtection { enabled, watermark } => {
                                let mut payload = serde_json::Map::new();
                                payload.insert(
                                    "type".to_string(),
                                    Value::String("room-capture-protection".to_string()),
                                );
                                if let Some(en) = enabled {
                                    payload.insert("enabled".to_string(), Value::Bool(en));
                                }
                                if let Some(wm) = watermark {
                                    payload.insert("watermark".to_string(), Value::Bool(wm));
                                }
                                let _ = ws_write
                                    .send(Message::Text(Value::Object(payload).to_string()))
                                    .await;
                            }
                            SignalingCommand::SendRawJson(payload) => {
                                let _ = ws_write.send(Message::Text(payload.to_string())).await;
                            }
                            other_report => {
                                if let Some(payload) = telemetry_payload_for_cmd(&other_report) {
                                    let _ = ws_write.send(Message::Text(payload.to_string())).await;
                                }
                            }
                        }
                    }
                    _ = ping_interval.tick() => {
                        ping_ticks = ping_ticks.wrapping_add(1);
                        if ping_ticks % 40 == 12 {
                            if let Some(sample) = crate::win_tray::lite_usage_sample() {
                                let _ = ws_write.send(Message::Text(sample.to_string())).await;
                            }
                        }
                        if let Err(e) = ws_write
                            .send(Message::Text(
                                json!({ "type": "time-sync", "t0": now_epoch_ms() }).to_string(),
                            ))
                            .await
                        {
                            crate::diag_warn!("signaling", "falha ao enviar time-sync keepalive: {}", e);
                            break;
                        }
                    }
                }
            }

            if !session.should_run {
                continue;
            }
            if close_code == Some(4000) {
                session.should_run = false;
                emit(SignalingEvent::StatusChanged(SignalingStatus::Superseded));
                continue;
            }
            if close_code == Some(4001) {
                session.should_run = false;
                emit(SignalingEvent::StatusChanged(SignalingStatus::Banned));
                continue;
            }

            emit(SignalingEvent::StatusChanged(SignalingStatus::Closed));
            if !wait_reconnect_or_cmd(&mut cmd_rx, &mut session).await {
                emit(SignalingEvent::StatusChanged(SignalingStatus::Idle));
            }
        }
    });

    (cmd_tx, evt_rx)
}

fn handle_pre_connect_cmd(cmd: SignalingCommand, session: &mut SessionState) {
    match cmd {
        SignalingCommand::Connect {
            ws_url,
            name,
            client_id,
            token,
            install_id,
            device_id,
        } => {
            session.ws_url = ws_url;
            session.name = name;
            session.client_id = client_id;
            session.token = token;
            session.install_id = install_id;
            session.device_id = device_id;
            session.should_run = true;
            session.reconnect_attempts = 0;
        }
        SignalingCommand::JoinRoom {
            room,
            confirm_device,
        } => {
            session.desired_room = Some(room);
            session.confirmed_device = confirm_device;
        }
        SignalingCommand::LeaveRoom => {
            session.desired_room = None;
        }
        SignalingCommand::Disconnect => {
            session.desired_room = None;
            session.should_run = false;
        }
        _ => {}
    }
}

async fn wait_reconnect_or_cmd(
    cmd_rx: &mut mpsc::UnboundedReceiver<SignalingCommand>,
    session: &mut SessionState,
) -> bool {
    let exp = session.reconnect_attempts.min(4);
    let ceiling_ms = (1000u64 * (1u64 << exp)).min(10_000);
    let jitter = crate::config::simple_random_u64() % (ceiling_ms / 2 + 1);
    let delay = Duration::from_millis(ceiling_ms / 2 + jitter);
    session.reconnect_attempts = session.reconnect_attempts.saturating_add(1);

    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep => {
                return session.should_run;
            }
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else {
                    session.should_run = false;
                    return false;
                };
                match cmd {
                    SignalingCommand::Disconnect => {
                        session.should_run = false;
                        session.desired_room = None;
                        return false;
                    }
                    SignalingCommand::Connect { ws_url, name, client_id, token, install_id, device_id } => {
                        session.ws_url = ws_url;
                        session.name = name;
                        session.client_id = client_id;
                        session.token = token;
                        session.install_id = install_id;
                        session.device_id = device_id;
                        session.should_run = true;
                        return true;
                    }
                    SignalingCommand::JoinRoom { room, confirm_device } => {
                        session.desired_room = Some(room);
                        session.confirmed_device = confirm_device;
                    }
                    SignalingCommand::LeaveRoom => {
                        session.desired_room = None;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn build_register_payload(session: &SessionState) -> Value {
    json!({
        "type": "register",
        "name": session.name,
        "clientId": session.client_id,
        "token": session.token,
        "fingerprint": session.device_id,
        // Proprio do Lite: a API marca a conexao como Go Live - Lite (icone proprio na lista
        // de participantes) e o admin pode mirar avisos so nele.
        "device": "desktop-lite",
        "desktopAppOS": "windows",
        "installId": session.install_id,
        "deviceId": session.device_id,
        "background": false,
        "idleMs": 0,
        "path": session.desired_room.as_ref().map(|r| format!("/watch/{}", r)),
        "version": "1.1.13-lite.rust",
        // Metricas do Lite no admin (versao em uso, atualizacoes, modificacoes instaladas).
        "liteVersion": env!("CARGO_PKG_VERSION"),
        "liteMods": installed_modifications(),
        "announcesLeave": true,
    })
}

fn build_join_payload(room: &str, confirm_device: bool, captcha_pass: Option<&str>) -> Value {
    let mut payload = json!({
        "type": "join",
        "room": room,
        "turnstileToken": null,
        "confirmDevice": confirm_device,
        "streamerMode": false,
    });
    if let Some(pass) = captcha_pass {
        payload["captchaPass"] = Value::String(pass.to_string());
    }
    payload
}

fn build_sharing_payload(screen: bool, camera: bool, screens: usize, cameras: usize) -> Value {
    json!({
        "type": "sharing",
        "sharing": screen || camera,
        "screen": screen,
        "camera": camera,
        "files": [],
        "cameraEffects": false,
        "phoneCamera": false,
        "screens": if screen { screens.max(1) } else { 0 },
        "cameras": if camera { cameras.max(1) } else { 0 },
    })
}

fn handle_server_message(
    msg: &Value,
    session: &mut SessionState,
    emit: &impl Fn(SignalingEvent),
) -> Option<Value> {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match msg_type {
        "welcome" => {
            // Aguarda register/registered
            None
        }
        "registered" => {
            let is_re_register = session.registered_once;
            session.registered_once = true;
            let self_id = msg
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = msg
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&session.name)
                .to_string();
            let account = msg
                .get("account")
                .and_then(|v| serde_json::from_value::<RegisteredAccount>(v.clone()).ok());
            let guest_token = msg
                .get("guestToken")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if let Some(pass) = msg.get("captchaPass").and_then(|v| v.as_str()) {
                session.captcha_pass = Some(pass.to_string());
            }

            let mut follow_up: Option<Value> = None;
            if account.is_none() {
                if let Some(ref gt) = guest_token {
                    session.token = Some(gt.clone());
                    session.client_id = self_id.clone();
                    // Re-echo register uma vez com o guestToken mintado (como faz signalingClient.ts)
                    if !is_re_register {
                        follow_up = Some(build_register_payload(session));
                    }
                }
            }

            emit(SignalingEvent::Registered {
                self_id,
                name,
                account,
                guest_token,
            });

            if !is_re_register {
                if let Some(ref room) = session.desired_room {
                    // Se havia follow_up, o servidor aceita ambos em sequencia
                    return Some(if let Some(reg) = follow_up {
                        // Nota: na pratica envia join logo apos ou retorna join aqui;
                        // Para garantir que join siga mesmo quando guestToken e ecoado, retornamos join se nao houver follow_up,
                        // mas se houver follow_up enviamos join apenas depois? Nao, WebSocket e ordenado e registered sera respondido de novo.
                        // Mas como is_re_register sera true na segunda resposta, enviamos o join agora ou na segunda!
                        // Melhor: enviamos o join agora e deixamos follow_up opcional, pois no servidor o guestToken ja esta registrado na sessao!
                        let _ = reg;
                        build_join_payload(
                            room,
                            session.confirmed_device,
                            session.captcha_pass.as_deref(),
                        )
                    } else {
                        build_join_payload(
                            room,
                            session.confirmed_device,
                            session.captcha_pass.as_deref(),
                        )
                    });
                }
            }
            follow_up
        }
        "register-error" => {
            let message = msg
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Erro ao registrar nome.")
                .to_string();
            let retryable = msg
                .get("retryable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            emit(SignalingEvent::RegisterError { message, retryable });
            None
        }
        "banned" => {
            let reason = msg
                .get("reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            emit(SignalingEvent::Banned { reason });
            None
        }
        "join-error" => {
            let reason = msg
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("generic")
                .to_string();
            if reason == "other-device" {
                let devices = msg.get("devices").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let max_devices = msg
                    .get("maxDevices")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as u32;
                emit(SignalingEvent::DeviceConflict {
                    devices,
                    max_devices,
                });
            } else {
                let message = msg
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Nao foi possivel entrar nesta sala.")
                    .to_string();
                session.desired_room = None;
                emit(SignalingEvent::JoinError { reason, message });
            }
            None
        }
        "captcha-required" => {
            let message = msg
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Verificacao anti-bot (Turnstile) exigida pelo servidor.")
                .to_string();
            session.desired_room = None;
            emit(SignalingEvent::CaptchaRequired { message });
            None
        }
        "room-state" => {
            if let Some(pass) = msg.get("captchaPass").and_then(|v| v.as_str()) {
                session.captcha_pass = Some(pass.to_string());
            }
            let room = msg
                .get("room")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let self_id = msg
                .get("selfId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let self_user_id = msg
                .get("selfUserId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let self_device = msg
                .get("selfDevice")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let peers = msg
                .get("peers")
                .and_then(|v| serde_json::from_value::<Vec<PeerInfo>>(v.clone()).ok())
                .unwrap_or_default();
            let messages = msg
                .get("messages")
                .and_then(|v| serde_json::from_value::<Vec<ChatMessage>>(v.clone()).ok())
                .unwrap_or_default();
            let video_sources = msg
                .get("videoSources")
                .and_then(|v| serde_json::from_value::<Vec<VideoSource>>(v.clone()).ok())
                .unwrap_or_default();
            let music = msg
                .get("music")
                .and_then(|v| serde_json::from_value::<MusicSource>(v.clone()).ok());
            let owner_id = msg
                .get("ownerId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let admins = extract_room_admins(msg.get("admins"));
            let silenced = extract_string_array(msg.get("silenced"));
            let description = msg
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let category = msg
                .get("category")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let member_limit = msg
                .get("memberLimit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let capture_protected = msg
                .get("captureProtected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let capture_watermark = msg
                .get("captureWatermark")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let permissions = RoomPermissions::from_json(msg.get("permissions"));
            let reclaimed = msg
                .get("reclaimed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            emit(SignalingEvent::RoomState {
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
                reclaimed,
            });
            None
        }
        "peer-joined" => {
            if let Ok(peer) = serde_json::from_value::<PeerInfo>(msg.clone()) {
                emit(SignalingEvent::PeerJoined(peer));
            }
            None
        }
        "peer-left" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                emit(SignalingEvent::PeerLeft(id.to_string()));
            }
            None
        }
        "peer-renamed" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                let name = msg
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let flags = msg
                    .get("flags")
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());
                let name_color = msg
                    .get("nameColor")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let avatar_url = msg
                    .get("avatarUrl")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                emit(SignalingEvent::PeerRenamed {
                    id: id.to_string(),
                    name,
                    flags,
                    name_color,
                    avatar_url,
                });
            }
            None
        }
        "peer-sharing" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                let sharing = msg
                    .get("sharing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let screen = msg.get("screen").and_then(|v| v.as_bool());
                let camera = msg.get("camera").and_then(|v| v.as_bool());
                let files = msg
                    .get("files")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let ad_paused = msg
                    .get("adPaused")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                emit(SignalingEvent::PeerSharing {
                    id: id.to_string(),
                    sharing,
                    screen,
                    camera,
                    files,
                    ad_paused,
                });
            }
            None
        }
        "peer-mic" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                let mic = msg.get("mic").and_then(|v| v.as_bool()).unwrap_or(false);
                emit(SignalingEvent::PeerMic {
                    id: id.to_string(),
                    mic,
                });
            }
            None
        }
        "peer-mics-muted" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                let mics_muted = msg
                    .get("micsMuted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                emit(SignalingEvent::PeerMicsMuted {
                    id: id.to_string(),
                    mics_muted,
                });
            }
            None
        }
        "peer-typing" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                let typing = msg
                    .get("typing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                emit(SignalingEvent::PeerTyping {
                    id: id.to_string(),
                    typing,
                });
            }
            None
        }
        "chat-message" => {
            if let Ok(chat_msg) = serde_json::from_value::<ChatMessage>(msg.clone()) {
                emit(SignalingEvent::ChatMessage(chat_msg));
            }
            None
        }
        "chat-deleted" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                emit(SignalingEvent::ChatDeleted(id.to_string()));
            }
            None
        }
        "chat-blocked" => {
            let text = msg
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Mensagem bloqueada pela moderacao.")
                .to_string();
            emit(SignalingEvent::ChatBlocked(text));
            None
        }
        "video-source-added" => {
            if let Some(src) = msg
                .get("source")
                .and_then(|v| serde_json::from_value::<VideoSource>(v.clone()).ok())
            {
                emit(SignalingEvent::VideoSourceAdded(src));
            }
            None
        }
        "video-source-removed" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                emit(SignalingEvent::VideoSourceRemoved(id.to_string()));
            }
            None
        }
        "video-source-state" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                emit(SignalingEvent::VideoSourceState {
                    id: id.to_string(),
                    playing: msg.get("playing").and_then(|v| v.as_bool()).unwrap_or(false),
                    position_seconds: msg
                        .get("positionSeconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    playback_rate: msg
                        .get("playbackRate")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0),
                    updated_at: msg
                        .get("updatedAt")
                        .and_then(|v| v.as_u64())
                        .unwrap_or_else(now_epoch_ms),
                });
            }
            None
        }
        "music" => {
            let music = msg
                .get("music")
                .and_then(|v| serde_json::from_value::<MusicSource>(v.clone()).ok());
            emit(SignalingEvent::MusicChanged(music));
            None
        }
        "music-state" => {
            if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                emit(SignalingEvent::MusicState {
                    id: id.to_string(),
                    playing: msg.get("playing").and_then(|v| v.as_bool()).unwrap_or(false),
                    position_seconds: msg
                        .get("positionSeconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    playback_rate: msg
                        .get("playbackRate")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0),
                    updated_at: msg
                        .get("updatedAt")
                        .and_then(|v| v.as_u64())
                        .unwrap_or_else(now_epoch_ms),
                });
            }
            None
        }
        "room-settings" => {
            let owner_id = msg
                .get("ownerId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let admins = extract_room_admins(msg.get("admins"));
            let silenced = extract_string_array(msg.get("silenced"));
            let description = msg
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let category = msg
                .get("category")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let member_limit = msg
                .get("memberLimit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let capture_protected = msg
                .get("captureProtected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let capture_watermark = msg
                .get("captureWatermark")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let permissions = RoomPermissions::from_json(msg.get("permissions"));
            emit(SignalingEvent::RoomSettings {
                owner_id,
                admins,
                silenced,
                description,
                category,
                member_limit,
                capture_protected,
                capture_watermark,
                permissions,
            });
            None
        }
        "room-bans" => {
            if !session.enable_perms_mod {
                return None;
            }
            #[cfg(feature = "mod_permissoes_chamada")]
            {
                let bans = msg
                    .get("bans")
                    .and_then(|v| serde_json::from_value::<Vec<RoomBan>>(v.clone()).ok())
                    .unwrap_or_default();
                emit(SignalingEvent::RoomBans(bans));
            }
            None
        }
        "room-tools"
        | "tool-upsert"
        | "tool-removed"
        | "tool-meta"
        | "tool-stroke"
        | "tool-stroke-live"
        | "tool-stroke-moved"
        | "tool-strokes-removed"
        | "tool-text-op"
        | "tool-sheet"
        | "tool-calc-expr"
        | "tool-reaction"
        | "room-hands" => {
            if !session.enable_tools_mod {
                return None;
            }
            #[cfg(feature = "mod_ferramentas")]
            emit(SignalingEvent::RoomToolMessage(msg.clone()));
            None
        }
        "room-permission-denied" => {
            let message = msg
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Acao nao permitida nesta sala.")
                .to_string();
            emit(SignalingEvent::RoomPermissionDenied(message));
            None
        }
        "room-removed" => {
            let banned = msg
                .get("banned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            session.desired_room = None;
            emit(SignalingEvent::RoomRemoved { banned });
            None
        }
        "time-sync" => {
            let t0 = msg.get("t0").and_then(|v| v.as_u64()).unwrap_or(0);
            let server_time = msg.get("serverTime").and_then(|v| v.as_u64()).unwrap_or(0);
            if t0 > 0 && server_time > 0 {
                let now = now_epoch_ms();
                let rtt = now.saturating_sub(t0);
                if rtt <= 5000 {
                    let offset = (server_time as i64) + (rtt as i64 / 2) - (now as i64);
                    emit(SignalingEvent::TimeSync { offset_ms: offset });
                }
            }
            None
        }
        "announcement" => {
            let announcement = match msg.get("announcement").filter(|v| !v.is_null()) {
                Some(v) => match serde_json::from_value::<Announcement>(v.clone()) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        crate::diag_warn!("signaling", "falha ao parsear announcement: {}", e);
                        None
                    }
                },
                None => None,
            };
            let live = msg.get("live").and_then(|v| v.as_bool()).unwrap_or(false);
            emit(SignalingEvent::AnnouncementUpdated { announcement, live });
            None
        }
        "lite-update-check" => {
            emit(SignalingEvent::LiteUpdateCheck);
            None
        }
        "error" => {
            if let Some(m) = msg.get("message").and_then(|v| v.as_str()) {
                emit(SignalingEvent::ServerError(m.chars().take(300).collect()));
            }
            None
        }
        "social-update" => {
            emit(SignalingEvent::SocialUpdate);
            None
        }
        "presence-state" => {
            let mut out = Vec::new();
            if let Some(map) = msg.get("presence").and_then(|v| v.as_object()) {
                for (id, entry) in map.iter().take(512) {
                    let state = entry.get("state").and_then(|s| s.as_str()).unwrap_or("offline");
                    let device = entry.get("device").and_then(|s| s.as_str()).map(str::to_string);
                    out.push((id.clone(), state.to_string(), device));
                }
            }
            if !out.is_empty() {
                emit(SignalingEvent::PresenceState(out));
            }
            None
        }
        "partner" => {
            let partner = msg
                .get("partner")
                .filter(|v| !v.is_null())
                .and_then(|v| serde_json::from_value::<PartnerCardData>(v.clone()).ok());
            emit(SignalingEvent::PartnerUpdated(partner));
            None
        }
        "signal" => {
            if let Some(from) = msg.get("from").and_then(|v| v.as_str()) {
                if let Some(data_val) = msg.get("data") {
                    if let Ok(data) = serde_json::from_value::<SignalData>(data_val.clone()) {
                        emit(SignalingEvent::Signal {
                            from: from.to_string(),
                            data,
                        });
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn telemetry_payload_for_cmd(cmd: &SignalingCommand) -> Option<Value> {
    match cmd {
        SignalingCommand::ReportAnnouncementView { id } => {
            Some(json!({ "type": "announcement-view", "id": id }))
        }
        SignalingCommand::ReportAnnouncementButtonClick { id } => {
            Some(json!({ "type": "announcement-button-click", "id": id }))
        }
        SignalingCommand::ReportAnnouncementDismiss { id } => {
            Some(json!({ "type": "announcement-x-click", "id": id }))
        }
        SignalingCommand::ReportPartnerView { id } => {
            Some(json!({ "type": "partner-view", "id": id }))
        }
        SignalingCommand::ReportPartnerSessionView { id } => {
            Some(json!({ "type": "partner-session-view", "id": id }))
        }
        SignalingCommand::ReportPartnerClick { id, source } => {
            Some(json!({ "type": "partner-click", "id": id, "source": source }))
        }
        _ => None,
    }
}

fn extract_room_admins(val: Option<&Value>) -> Vec<RoomAdmin> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        Some(RoomAdmin {
                            id: s.to_string(),
                            name: String::new(),
                        })
                    } else {
                        let id = item.get("id")?.as_str()?.to_string();
                        let name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some(RoomAdmin { id, name })
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_string_array(val: Option<&Value>) -> Vec<String> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Modificacoes instaladas (o `modifications.json` que o setup grava ao lado do executavel).
/// Lido uma vez por execucao: so muda quando o setup roda de novo, e ai o app reinicia.
fn installed_modifications() -> &'static [String] {
    static MODS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    MODS.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("modifications.json")))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|raw| crate::config::AppConfig::parse_modifications_manifest(&raw))
            .unwrap_or_default()
    })
}
