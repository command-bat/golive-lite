use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

use crate::net::protocol::{
    is_safe_http_url, signaling_http_base, IceServerConfig, PartnerCardData, SiteStatus,
};

pub const SITE_STATUS_URL: &str = "https://bin.nemtudo.me/raw/golive-status";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub points: u64,
    #[serde(default)]
    pub equipped_name_color: Option<String>,
    #[serde(default)]
    pub equipped_profile_color: Option<String>,
    #[serde(default)]
    pub profile_theme: Option<serde_json::Value>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub banner_url: Option<String>,
    #[serde(default)]
    pub call_seconds: u64,
    #[serde(default)]
    pub mic_seconds: u64,
    #[serde(default)]
    pub share_seconds: u64,
    #[serde(default)]
    pub profile_links: Vec<crate::net::protocol::ProfileLink>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub bot: bool,
}

#[cfg(feature = "mod_perfil")]
#[derive(Debug, Clone)]
pub struct UserProfileData {
    pub account: AccountInfo,
    pub live_room: Option<(String, u64)>,
    pub group_count: Option<u64>,
}

#[cfg(feature = "mod_perfil")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserProfileLiveRaw {
    room: String,
    people_count: u64,
}

#[cfg(feature = "mod_perfil")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserProfileResponse {
    account: AccountInfo,
    #[serde(default)]
    live: Option<UserProfileLiveRaw>,
    #[serde(default)]
    group_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    token: String,
    account: AccountInfo,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    account: AccountInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IceServersResponse {
    #[serde(default)]
    ice_servers: Vec<IceServerConfig>,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    own: Option<OwnIceWrapper>,
    #[serde(default)]
    audio: Option<AudioIceWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnIceWrapper {
    #[serde(default)]
    ice_servers: Vec<IceServerConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioIceWrapper {
    #[serde(default)]
    own: Option<OwnIceWrapper>,
}

/// Constroi agente HTTP ureq com backend TLS nativo do sistema operacional (Schannel no Windows).
pub fn build_http_agent(timeout: Duration) -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new()
        .map_err(|e| format!("Falha ao inicializar TLS nativo: {}", e))?;
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(timeout)
        .build())
}

pub fn login_account(
    ws_url: &str,
    username: &str,
    password: &str,
) -> Result<(String, AccountInfo), String> {
    let base = signaling_http_base(ws_url);
    let url = format!("{}/auth/login", base);
    let agent = build_http_agent(Duration::from_secs(10))?;
    let resp = agent
        .post(&url)
        .set("Accept-Language", "pt-BR")
        .send_json(ureq::json!({
            "username": username.trim(),
            "password": password,
            "turnstileToken": null
        }))
        .map_err(|e| match e {
            ureq::Error::Status(_, r) => {
                if let Ok(v) = r.into_json::<serde_json::Value>() {
                    v.get("error")
                        .and_then(|s| s.as_str())
                        .unwrap_or("Usuario ou senha invalidos.")
                        .to_string()
                } else {
                    "Usuario ou senha invalidos.".to_string()
                }
            }
            ureq::Error::Transport(t) => format!("Erro de rede: {}", t),
        })?;

    let data: AuthResponse = resp
        .into_json()
        .map_err(|e| format!("Resposta invalida: {}", e))?;
    Ok((data.token, data.account))
}

pub fn fetch_me(ws_url: &str, token: &str) -> Result<AccountInfo, String> {
    let base = signaling_http_base(ws_url);
    let url = format!("{}/auth/me", base);
    let agent = build_http_agent(Duration::from_secs(8))?;
    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {}", token))
        .call()
        .map_err(|e| format!("Sessao expirada ou indisponivel: {}", e))?;

    let data: MeResponse = resp
        .into_json()
        .map_err(|e| format!("Resposta invalida: {}", e))?;
    Ok(data.account)
}

#[cfg(feature = "mod_perfil")]
pub fn fetch_user_profile(
    ws_url: &str,
    user_id: &str,
    token: Option<&str>,
) -> Result<UserProfileData, String> {
    let base = signaling_http_base(ws_url);
    let url = format!("{}/users/{}", base, encode_query_component(user_id.trim()));
    let agent = build_http_agent(Duration::from_secs(8))?;
    let mut req = agent.get(&url);
    if let Some(tok) = token {
        if !tok.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", tok));
        }
    }
    let resp = req
        .call()
        .map_err(|e| format!("Perfil indisponivel: {}", e))?;
    let parsed: UserProfileResponse = resp
        .into_json()
        .map_err(|e| format!("Resposta de perfil invalida: {}", e))?;
    Ok(UserProfileData {
        account: parsed.account,
        live_room: parsed.live.map(|l| (l.room, l.people_count)),
        group_count: parsed.group_count,
    })
}

pub fn fetch_ice_servers(ws_url: &str, token: Option<&str>) -> Vec<IceServerConfig> {
    let mut servers = vec![
        IceServerConfig {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            username: None,
            credential: None,
        },
        IceServerConfig {
            urls: vec!["stun:stun.cloudflare.com:3478".to_string()],
            username: None,
            credential: None,
        },
    ];

    let base = signaling_http_base(ws_url);
    let url = format!("{}/ice-servers?audio=1", base);
    let Ok(agent) = build_http_agent(Duration::from_secs(5)) else {
        return servers;
    };
    let mut req = agent.get(&url);
    if let Some(tok) = token {
        req = req.set("Authorization", &format!("Bearer {}", tok));
    }
    if let Ok(resp) = req.call() {
        if let Ok(body) = resp.into_json::<IceServersResponse>() {
            if !body.locked {
                for s in body.ice_servers {
                    if !s.urls.is_empty() {
                        servers.push(s);
                    }
                }
            }
            if let Some(own) = body.own {
                for s in own.ice_servers {
                    if !s.urls.is_empty() {
                        servers.push(s);
                    }
                }
            }
            if let Some(audio) = body.audio {
                if let Some(own_audio) = audio.own {
                    for s in own_audio.ice_servers {
                        if !s.urls.is_empty() {
                            servers.push(s);
                        }
                    }
                }
            }
        }
    }
    servers
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatsResponse {
    #[serde(default)]
    people_online: Option<u64>,
}

/// Parses raw JSON returned by `SITE_STATUS_URL`. Returns `None` on malformed JSON.
pub fn parse_site_status_json(raw: &str) -> Option<SiteStatus> {
    serde_json::from_str::<SiteStatus>(raw).ok()
}

pub fn fetch_site_status(agent: &ureq::Agent) -> Option<SiteStatus> {
    let resp = agent.get(SITE_STATUS_URL).call().ok()?;
    let body = resp.into_string().ok()?;
    parse_site_status_json(&body)
}

pub fn fetch_people_online(agent: &ureq::Agent, http_base: &str) -> Option<u64> {
    let url = format!("{}/stats", http_base.trim_end_matches('/'));
    let resp = agent.get(&url).call().ok()?;
    let body = resp.into_json::<StatsResponse>().ok()?;
    body.people_online
}

pub fn fetch_guest_points(agent: &ureq::Agent, http_base: &str, guest_token: &str) -> Option<u64> {
    let tok = guest_token.trim();
    if tok.is_empty() {
        return None;
    }
    let url = format!("{}/guest/points", http_base.trim_end_matches('/'));
    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {}", tok))
        .call()
        .ok()?;
    let val = resp.into_json::<serde_json::Value>().ok()?;
    val.get("points").and_then(|v| v.as_u64())
}

pub fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

pub fn fetch_partner_ad(
    agent: &ureq::Agent,
    http_base: &str,
    current_id: Option<&str>,
) -> Option<Option<PartnerCardData>> {
    let base = http_base.trim_end_matches('/');
    let url = match current_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => format!("{}/partner?current={}", base, encode_query_component(id)),
        None => format!("{}/partner", base),
    };
    let resp = agent.get(&url).call().ok()?;
    let val = resp.into_json::<serde_json::Value>().ok()?;
    if val.is_null() {
        return Some(None);
    }
    if let Some(inner) = val.get("partner") {
        if inner.is_null() {
            return Some(None);
        }
        return Some(serde_json::from_value::<PartnerCardData>(inner.clone()).ok());
    }
    if let Ok(parsed) = serde_json::from_value::<PartnerCardData>(val) {
        if !parsed.id.trim().is_empty() || !parsed.title.trim().is_empty() {
            return Some(Some(parsed));
        }
    }
    Some(None)
}

pub fn fetch_image_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if !is_safe_http_url(url) {
        return Err("URL de imagem insegura".to_string());
    }
    let resp = agent
        .get(url.trim())
        .call()
        .map_err(|e| format!("falha HTTP ao baixar imagem: {e}"))?;
    let mut reader = resp.into_reader().take(10 * 1024 * 1024 + 1);
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| format!("falha ao ler bytes da imagem: {e}"))?;
    if buf.len() > 10 * 1024 * 1024 {
        return Err("imagem remota excede 10 MB".to_string());
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_http_agent_with_native_tls() {
        let agent = build_http_agent(Duration::from_secs(5));
        assert!(agent.is_ok(), "TLS connector must initialize cleanly");
    }

    #[test]
    fn test_parse_site_status_json_handles_outage_and_safe_urls() {
        let raw = r#"{
            "api_error": true,
            "message": "Servidores em manutencao programada.",
            "button": {
                "enabled": true,
                "label": "Ver status",
                "href": "https://status.nemtudo.me",
                "target": "_blank"
            }
        }"#;
        let st = parse_site_status_json(raw).expect("valid status JSON");
        assert!(st.should_display());
        assert_eq!(st.message, "Servidores em manutencao programada.");
        let btn = st.button.expect("button present");
        assert!(btn.enabled);
        assert!(is_safe_http_url(&btn.href));

        let cleared = parse_site_status_json(r#"{"api_error": false, "message": ""}"#).unwrap();
        assert!(!cleared.should_display());
    }

    #[test]
    fn test_account_info_deserializes_banner_color_and_profile_theme() {
        let raw = r##"{
            "id": "u1",
            "username": "alice",
            "displayName": "Alice",
            "flags": ["PRO_MAX"],
            "points": 1250,
            "equippedProfileColor": "#22c55e",
            "profileTheme": { "from": "#102030", "to": "#204060" }
        }"##;
        let acc: AccountInfo = serde_json::from_str(raw).expect("AccountInfo JSON");
        assert_eq!(acc.equipped_profile_color.as_deref(), Some("#22c55e"));
        assert_eq!(
            acc.profile_theme
                .as_ref()
                .and_then(|t| t.get("from"))
                .and_then(|v| v.as_str()),
            Some("#102030")
        );
    }
}
