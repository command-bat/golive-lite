//! Modificacao "Amigos": cliente das rotas `/social` da API (as mesmas do `lib/socialApi.ts`
//! do site). Nada aqui decide quem pode o que: so pergunta, e depois de qualquer acao o app
//! rele o grafo inteiro (como o site), em vez de remendar uma copia local.
//!
//! So existe com a feature `mod_amigos`; sem a modificacao instalada nada disto roda.

use std::time::Duration;

use serde::Deserialize;

use crate::net::auth::{build_http_agent, encode_query_component};
use crate::net::protocol::signaling_http_base;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SocialUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub bot: bool,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub name_color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SocialGraph {
    #[serde(default)]
    pub friends: Vec<SocialUser>,
    /// Pedidos esperando esta conta responder.
    #[serde(default)]
    pub incoming: Vec<SocialUser>,
    /// Pedidos que esta conta mandou e ninguem respondeu.
    #[serde(default)]
    pub outgoing: Vec<SocialUser>,
    #[serde(default)]
    pub blocked: Vec<SocialUser>,
}

/// Onde as duas contas estao, para a busca oferecer o unico verbo que se aplica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Relationship {
    None,
    Friends,
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub user: SocialUser,
    pub relationship: Relationship,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchHit>,
}

/// O grafo inteiro; `None` sem login ou com a API fora.
pub fn fetch_graph(ws_url: &str, token: &str) -> Option<SocialGraph> {
    let agent = build_http_agent(Duration::from_secs(8)).ok()?;
    agent
        .get(&format!("{}/social", signaling_http_base(ws_url)))
        .set("Authorization", &format!("Bearer {}", token))
        .call()
        .ok()?
        .into_json::<SocialGraph>()
        .ok()
}

/// Pessoas pelo nome de usuario (nunca voce, nunca quem bloqueou). Vazio em erro: a busca
/// roda enquanto a pessoa digita e a proxima tecla pergunta de novo.
pub fn search(ws_url: &str, token: &str, query: &str) -> Vec<SearchHit> {
    let Ok(agent) = build_http_agent(Duration::from_secs(8)) else {
        return Vec::new();
    };
    let url = format!(
        "{}/social/search?q={}",
        signaling_http_base(ws_url),
        encode_query_component(query)
    );
    agent
        .get(&url)
        .set("Authorization", &format!("Bearer {}", token))
        .call()
        .ok()
        .and_then(|r| r.into_json::<SearchResponse>().ok())
        .map(|r| r.results)
        .unwrap_or_default()
}

/// Um verbo do grafo. Mesmo texto de erro do site quando a API nao diz o motivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocialAction {
    /// Manda um pedido, ou aceita o que ja vinha.
    Add,
    Accept,
    /// Recusar, cancelar ou desfazer amizade: uma rota so na API.
    Remove,
    Block,
    Unblock,
}

pub fn act(ws_url: &str, token: &str, user_id: &str, action: SocialAction) -> Result<(), String> {
    let base = signaling_http_base(ws_url);
    let id = encode_query_component(user_id);
    let (method, path) = match action {
        SocialAction::Add => ("POST", format!("/social/friends/{}", id)),
        SocialAction::Accept => ("POST", format!("/social/friends/{}/accept", id)),
        SocialAction::Remove => ("DELETE", format!("/social/friends/{}", id)),
        SocialAction::Block => ("POST", format!("/social/blocks/{}", id)),
        SocialAction::Unblock => ("DELETE", format!("/social/blocks/{}", id)),
    };
    let agent = build_http_agent(Duration::from_secs(8)).map_err(|_| "Sem conexão com o servidor.".to_string())?;
    match agent
        .request(method, &format!("{}{}", base, path))
        .set("Authorization", &format!("Bearer {}", token))
        .call()
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(_, r)) => Err(r
            .into_json::<serde_json::Value>()
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| "Não foi possível concluir.".to_string())),
        Err(ureq::Error::Transport(_)) => Err("Sem conexão com o servidor.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_and_search_parse_the_site_shapes() {
        let g: SocialGraph = serde_json::from_str(
            r#"{"friends":[{"id":"a","username":"ana","displayName":"Ana","flags":["PRO"],"avatarUrl":null}],
                "incoming":[],"outgoing":[{"id":"b","username":"bia","displayName":"Bia","flags":[]}],"blocked":[]}"#,
        )
        .unwrap();
        assert_eq!(g.friends[0].display_name, "Ana");
        assert_eq!(g.outgoing[0].username, "bia");
        let s: SearchResponse = serde_json::from_str(
            r#"{"results":[{"id":"c","username":"caio","displayName":"Caio","flags":[],"relationship":"incoming"}]}"#,
        )
        .unwrap();
        assert_eq!(s.results[0].relationship, Relationship::Incoming);
        assert_eq!(s.results[0].user.username, "caio");
    }
}
