//! Modificacao "Amigos": copia da pagina `/friends` do site (`app/(shell)/friends/FriendsPanel.tsx`)
//! e do dialogo "Adicionar amigo" (`components/AddFriendDialog.tsx`), mais o botao de amizade
//! do cartao de perfil (`components/SocialActions.tsx`).
//!
//! Custo zero sem a modificacao: tudo aqui so compila com `mod_amigos` e o estado so existe
//! (`app.amigos`) quando ela esta instalada e ha conta logada. Rede so por evento: ao logar,
//! quando a API avisa `social-update` e depois de cada acao — nunca por polling. Presenca so
//! e pedida (`presence-watch`) enquanto a pagina esta aberta.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Rounding, Sense, Stroke};

use crate::config::signaling_ws_url;
use crate::net::protocol::{resolve_name_color_rgb, verified_badge_tone};
use crate::net::social::{self, Relationship, SearchHit, SocialAction, SocialGraph, SocialUser};
use crate::ui::icons;
use crate::ui::theme::{SemiBoldExt, BG_ELEVATED, BG_MAIN, BG_SURFACE, BORDER_STRONG, BORDER_SUBTLE, TEXT_MUTED, TEXT_SECONDARY};

// Paleta usada pela pagina (classes do site traduzidas).
const ZINC_50: Color32 = Color32::from_rgb(250, 250, 250);
const ZINC_100: Color32 = Color32::from_rgb(244, 244, 245);
const ZINC_300: Color32 = Color32::from_rgb(212, 212, 216);
const EMERALD_600: Color32 = Color32::from_rgb(5, 150, 105);
const EMERALD_700: Color32 = Color32::from_rgb(4, 120, 87);
const PRESENCE_ONLINE: Color32 = Color32::from_rgb(16, 185, 129); // emerald-500
const PRESENCE_AWAY: Color32 = Color32::from_rgb(14, 165, 233); // sky-500
const PRESENCE_BACKGROUND: Color32 = Color32::from_rgb(251, 191, 36); // amber-400
const RED_BG: Color32 = Color32::from_rgb(69, 10, 10);
const RED_TEXT: Color32 = Color32::from_rgb(252, 165, 165);

/// Mesmo atraso do site entre a tecla e a busca.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

enum Msg {
    Graph(Option<SocialGraph>),
    Search(String, Vec<SearchHit>),
    Acted { user_id: String, result: Result<(), String>, from_search: bool, action: SocialAction },
}

pub struct AmigosState {
    /// Pagina "Amigos" aberta (no lugar da tela inicial).
    pub page_open: bool,
    pub graph: Option<SocialGraph>,
    graph_inflight: bool,
    /// Outro `social-update` chegou com uma leitura em andamento: rele quando ela voltar.
    graph_stale: bool,
    busy_id: Option<String>,
    error: Option<String>,
    /// id da conta -> (estado, aparelho). So enquanto a pagina esta aberta.
    presence: HashMap<String, (String, Option<String>)>,
    watched: Vec<String>,
    pub add_open: bool,
    add_query: String,
    add_focus: bool,
    add_error: Option<String>,
    search: Option<(String, Vec<SearchHit>)>,
    search_due: Option<Instant>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl AmigosState {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            page_open: false,
            graph: None,
            graph_inflight: false,
            graph_stale: false,
            busy_id: None,
            error: None,
            presence: HashMap::new(),
            watched: Vec::new(),
            add_open: false,
            add_query: String::new(),
            add_focus: false,
            add_error: None,
            search: None,
            search_due: None,
            tx,
            rx,
        }
    }

    /// Pedidos esperando resposta (o numero do selo no menu).
    pub fn incoming_count(&self) -> usize {
        self.graph.as_ref().map_or(0, |g| g.incoming.len())
    }

    pub fn relationship_with(&self, user_id: &str) -> FriendState {
        let Some(g) = self.graph.as_ref() else { return FriendState::Unknown };
        if g.blocked.iter().any(|u| u.id == user_id) {
            FriendState::Blocked
        } else if g.friends.iter().any(|u| u.id == user_id) {
            FriendState::Friends
        } else if g.incoming.iter().any(|u| u.id == user_id) {
            FriendState::Incoming
        } else if g.outgoing.iter().any(|u| u.id == user_id) {
            FriendState::Outgoing
        } else {
            FriendState::None
        }
    }

    /// Rele `/social` (ao logar, num `social-update`, depois de uma acao).
    pub fn refresh(&mut self, token: &str, ctx: &egui::Context) {
        if self.graph_inflight {
            self.graph_stale = true;
            return;
        }
        self.graph_inflight = true;
        let (tx, ws, tok, ctx) = (self.tx.clone(), signaling_ws_url(), token.to_string(), ctx.clone());
        let _ = std::thread::Builder::new().name("golive-social".into()).spawn(move || {
            let _ = tx.send(Msg::Graph(social::fetch_graph(&ws, &tok)));
            ctx.request_repaint();
        });
    }

    pub fn act(&mut self, token: &str, ctx: &egui::Context, user_id: &str, action: SocialAction, from_search: bool) {
        if self.busy_id.is_some() {
            return;
        }
        self.busy_id = Some(user_id.to_string());
        if from_search {
            self.add_error = None;
        } else {
            self.error = None;
        }
        let (tx, ws, tok, uid, ctx) =
            (self.tx.clone(), signaling_ws_url(), token.to_string(), user_id.to_string(), ctx.clone());
        let _ = std::thread::Builder::new().name("golive-social-act".into()).spawn(move || {
            let result = social::act(&ws, &tok, &uid, action);
            let _ = tx.send(Msg::Acted { user_id: uid, result, from_search, action });
            ctx.request_repaint();
        });
    }

    pub fn apply_presence(&mut self, entries: Vec<(String, String, Option<String>)>) {
        if !self.page_open {
            return;
        }
        for (id, state, device) in entries {
            self.presence.insert(id, (state, device));
        }
    }

    /// Recebe resultados das threads. Devolve `true` quando o grafo precisa ser relido.
    fn drain(&mut self) -> bool {
        let mut reread = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Graph(g) => {
                    self.graph_inflight = false;
                    // Falha de rede mantem o que ja havia (como o site).
                    if let Some(g) = g {
                        self.graph = Some(g);
                    } else if self.graph.is_none() {
                        self.graph = Some(SocialGraph::default());
                    }
                    if std::mem::take(&mut self.graph_stale) {
                        reread = true;
                    }
                }
                Msg::Search(q, hits) => {
                    if self.add_query.trim().trim_start_matches('@') == q {
                        self.search = Some((q, hits));
                    }
                }
                Msg::Acted { user_id, result, from_search, action } => {
                    self.busy_id = None;
                    match result {
                        Err(e) => {
                            if from_search {
                                self.add_error = Some(e);
                            } else {
                                self.error = Some(e);
                            }
                        }
                        Ok(()) if from_search => {
                            // Mesma troca otimista do site: "aceitar" vira amigo, "adicionar" vira enviado.
                            if let Some((_, hits)) = self.search.as_mut() {
                                for h in hits.iter_mut().filter(|h| h.user.id == user_id) {
                                    h.relationship = if action == SocialAction::Accept || h.relationship == Relationship::Incoming {
                                        Relationship::Friends
                                    } else {
                                        Relationship::Outgoing
                                    };
                                }
                            }
                        }
                        Ok(()) => {}
                    }
                    // Rele sempre: uma falha muitas vezes e falha *porque* o grafo mudou.
                    reread = true;
                }
            }
        }
        reread
    }

    /// Ids que a pagina precisa observar (presenca).
    fn wanted_presence(&self) -> Vec<String> {
        if !self.page_open {
            return Vec::new();
        }
        let Some(g) = self.graph.as_ref() else { return Vec::new() };
        let mut ids: Vec<String> = g.friends.iter().chain(g.incoming.iter()).chain(g.outgoing.iter()).map(|u| u.id.clone()).collect();
        ids.sort();
        ids.dedup();
        ids.truncate(200);
        ids
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendState {
    Unknown,
    None,
    Friends,
    Incoming,
    Outgoing,
    Blocked,
}

/// Chamado a cada frame pelo app (barato: um `try_recv` vazio). Cuida das respostas, da busca
/// com atraso e do `presence-watch`.
pub fn tick(app: &mut crate::app::GoLiveLiteApp, ctx: &egui::Context) {
    let Some(token) = app.config.account_token.clone() else { return };
    let Some(st) = app.amigos.as_mut() else { return };
    let reread = st.drain();
    if reread {
        st.refresh(&token, ctx);
    }
    // Busca do "Adicionar amigo" (300 ms depois da ultima tecla, como o site).
    if let Some(due) = st.search_due {
        if Instant::now() >= due {
            st.search_due = None;
            let q = st.add_query.trim().trim_start_matches('@').to_string();
            if q.chars().count() >= 2 {
                let (tx, ws, tok, ctx2) = (st.tx.clone(), signaling_ws_url(), token.clone(), ctx.clone());
                let _ = std::thread::Builder::new().name("golive-social-search".into()).spawn(move || {
                    let hits = social::search(&ws, &tok, &q);
                    let _ = tx.send(Msg::Search(q, hits));
                    ctx2.request_repaint();
                });
            }
        } else {
            ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
        }
    }
    // Presenca: so pede o que a pagina mostra, e solta tudo quando ela fecha.
    let wanted = st.wanted_presence();
    if wanted != st.watched {
        if wanted.is_empty() {
            st.presence.clear();
            st.presence.shrink_to_fit();
        }
        st.watched = wanted.clone();
        app.send_tool_json(serde_json::json!({ "type": "presence-watch", "ids": wanted }));
    }
}

// --- Desenho -----------------------------------------------------------------------------

fn paint_check(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    p.add(egui::Shape::line(
        vec![pos2(c.x - s * 0.85, c.y), pos2(c.x - s * 0.25, c.y + s * 0.6), pos2(c.x + s * 0.9, c.y - s * 0.6)],
        Stroke::new(1.7_f32, col),
    ));
}

fn paint_search(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let r = sz * 0.3;
    let st = Stroke::new(1.6_f32, col);
    p.circle_stroke(pos2(c.x - r * 0.3, c.y - r * 0.3), r, st);
    p.line_segment([pos2(c.x + r * 0.45, c.y + r * 0.45), pos2(c.x + r * 1.25, c.y + r * 1.25)], st);
}

fn paint_person_remove(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.4_f32, col);
    p.circle_stroke(pos2(c.x - s * 0.25, c.y - s * 0.35), s * 0.32, st);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.82, c.y + s * 0.72),
            pos2(c.x - s * 0.62, c.y + s * 0.22),
            pos2(c.x + s * 0.12, c.y + s * 0.22),
            pos2(c.x + s * 0.28, c.y + s * 0.72),
        ],
        st,
    ));
    p.line_segment([pos2(c.x + s * 0.35, c.y - s * 0.15), pos2(c.x + s * 0.85, c.y - s * 0.15)], st);
}

fn paint_block(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let r = sz * 0.36;
    let st = Stroke::new(1.5_f32, col);
    p.circle_stroke(c, r, st);
    let d = r * 0.7;
    p.line_segment([pos2(c.x - d, c.y + d), pos2(c.x + d, c.y - d)], st);
}

type IconFn = fn(&Painter, Pos2, f32, Color32);

#[derive(Clone, Copy)]
enum Tone {
    /// `bg-emerald-600 text-white`
    Primary,
    /// `bg-zinc-50 text-zinc-950` (o "Adicionar" no escuro)
    Light,
    /// `border-zinc-700 text-zinc-400`
    Outline,
}

/// O `ACTION` do site: `px-2.5 py-1.5 text-xs rounded-lg`, icone 14 px.
fn action_button(ui: &mut egui::Ui, label: &str, icon: IconFn, tone: Tone, disabled: bool) -> bool {
    let font = FontId::new(12.0, egui::FontFamily::Name(crate::ui::theme::SEMIBOLD.into()));
    let text_w = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x);
    let size = vec2(10.0 + 14.0 + 4.0 + text_w + 10.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, if disabled { Sense::hover() } else { Sense::click() });
    let hovered = resp.hovered() && !disabled;
    let (fill, stroke, fg) = match tone {
        Tone::Primary => (if hovered { EMERALD_700 } else { EMERALD_600 }, Stroke::NONE, Color32::WHITE),
        Tone::Light => (if hovered { Color32::from_rgb(228, 228, 231) } else { ZINC_50 }, Stroke::NONE, Color32::from_rgb(9, 9, 11)),
        Tone::Outline => (
            if hovered { BG_ELEVATED } else { Color32::TRANSPARENT },
            Stroke::new(1.0_f32, BORDER_STRONG),
            TEXT_SECONDARY,
        ),
    };
    let alpha = if disabled { 0.5 } else { 1.0 };
    let p = ui.painter();
    p.rect(rect, Rounding::same(8.0), fill.gamma_multiply(alpha), Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha)));
    icon(p, pos2(rect.left() + 10.0 + 7.0, rect.center().y), 14.0, fg.gamma_multiply(alpha));
    p.text(pos2(rect.left() + 10.0 + 14.0 + 4.0, rect.center().y), Align2::LEFT_CENTER, label, font, fg.gamma_multiply(alpha));
    if !disabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    } else {
        false
    }
}

/// `DisplayUserName`: nome (cor da loja), selo verificado e a etiqueta BOT.
fn display_name(ui: &mut egui::Ui, user: &SocialUser, size: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let color = resolve_name_color_rgb(user.name_color.as_deref())
            .map(|[r, g, b]| Color32::from_rgb(r, g, b))
            .unwrap_or(ZINC_100);
        ui.add(egui::Label::new(RichText::new(&user.display_name).size(size).color(color).semibold()).truncate());
        if let Some(tone) = verified_badge_tone(&user.flags) {
            let (r, _) = ui.allocate_exact_size(vec2(13.0, 14.0), Sense::hover());
            icons::paint_verified_seal(ui.painter(), r.center(), 5.6, tone);
        }
        if user.bot {
            let galley = ui.painter().layout_no_wrap("BOT".into(), FontId::proportional(9.5), Color32::WHITE);
            let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + 8.0, 15.0), Sense::hover());
            ui.painter().rect_filled(r, Rounding::same(4.0), Color32::from_rgb(88, 101, 242));
            ui.painter().galley(pos2(r.left() + 4.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
        }
    });
}

fn presence_color(state: &str) -> Option<Color32> {
    match state {
        "online" => Some(PRESENCE_ONLINE),
        "away" => Some(PRESENCE_AWAY),
        "background" => Some(PRESENCE_BACKGROUND),
        _ => None,
    }
}

fn presence_label(state: &str) -> &'static str {
    match state {
        "online" => "Online",
        "away" => "Ausente",
        "background" => "Com o app em segundo plano",
        _ => "Offline",
    }
}

/// `rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2`, nome + @usuario, acoes a direita.
fn user_row(
    ui: &mut egui::Ui,
    user: &SocialUser,
    presence: Option<&(String, Option<String>)>,
    avatar: Option<egui::TextureId>,
    actions: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::none()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if let Some(tex) = avatar {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                    icons::paint_circular_avatar(ui.painter(), r, tex, Stroke::new(1.0_f32, BORDER_SUBTLE));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if let Some((state, _)) = presence {
                            if let Some(col) = presence_color(state) {
                                let (r, resp) = ui.allocate_exact_size(vec2(8.0, 16.0), Sense::hover());
                                ui.painter().circle_filled(r.center(), 4.0, col);
                                resp.on_hover_text(presence_label(state));
                            }
                        }
                        display_name(ui, user, 14.0);
                    });
                    ui.label(RichText::new(format!("@{}", user.username)).size(12.0).color(TEXT_MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    actions(ui);
                });
            });
        });
}

fn section(ui: &mut egui::Ui, title: &str, count: usize, empty: &str, rows: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(24.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(title).size(14.0).semibold().color(ZINC_300));
        if count > 0 {
            ui.label(RichText::new(count.to_string()).size(14.0).color(TEXT_MUTED));
        }
    });
    ui.add_space(8.0);
    if count == 0 {
        ui.label(RichText::new(empty).size(14.0).color(TEXT_MUTED));
    } else {
        ui.spacing_mut().item_spacing.y = 8.0;
        rows(ui);
    }
}

#[derive(Clone)]
enum Click {
    Act(String, SocialAction),
    Message(String),
}

/// A pagina "Amigos" no lugar da tela inicial. Devolve `false` quando a pessoa volta.
pub fn render_friends_page(app: &mut crate::app::GoLiveLiteApp, ctx: &egui::Context) {
    let Some(token) = app.config.account_token.clone().filter(|_| app.logged_account.is_some()) else {
        render_logged_out_page(app, ctx);
        return;
    };
    let Some(graph) = app.amigos.as_ref().map(|s| s.graph.clone()) else { return };
    // Avatares so para quem tem foto (a pagina do site nao mostra foto nos amigos; o
    // dialogo de busca mostra, e e la que eles sao carregados).
    let mut clicks: Vec<Click> = Vec::new();
    let mut back = false;
    let mut open_add = false;

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG_MAIN))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // `mx-auto max-w-2xl px-4 py-10`
                let w = ui.available_width().min(672.0);
                let pad = ((ui.available_width() - w) / 2.0).max(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(w - 32.0);
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, Rounding::same(8.0), BG_ELEVATED);
                            }
                            icons::paint_arrow_back(ui.painter(), r.center(), 18.0, ZINC_300);
                            if resp.on_hover_text("Voltar ao início").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                back = true;
                            }
                            ui.label(RichText::new("Amigos").size(24.0).semibold().color(ZINC_50));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if action_button(ui, "Adicionar", icons::paint_user_plus, Tone::Light, false) {
                                    open_add = true;
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Adicione alguém pelo perfil, ou pela lista de participantes de uma sala.")
                                .size(14.0)
                                .color(TEXT_MUTED),
                        );

                        let (busy, error, presence) = app
                            .amigos
                            .as_ref()
                            .map(|s| (s.busy_id.clone(), s.error.clone(), s.presence.clone()))
                            .unwrap_or_default();
                        if let Some(err) = error {
                            ui.add_space(12.0);
                            egui::Frame::none()
                                .fill(RED_BG)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(err).size(14.0).color(RED_TEXT));
                                });
                        }

                        let Some(g) = graph.as_ref() else {
                            ui.add_space(24.0);
                            ui.label(RichText::new("Carregando…").size(14.0).color(TEXT_MUTED));
                            return;
                        };
                        let is_busy = |id: &str| busy.as_deref() == Some(id);
                        let messages = crate::ui::friends_view::can_message(app);

                        section(ui, "Pedidos recebidos", g.incoming.len(), "Nenhum pedido esperando por você.", |ui| {
                            for u in &g.incoming {
                                user_row(ui, u, presence.get(&u.id), None, |ui| {
                                    if action_button(ui, "Recusar", icons::paint_close_x, Tone::Outline, is_busy(&u.id)) {
                                        clicks.push(Click::Act(u.id.clone(), SocialAction::Remove));
                                    }
                                    if action_button(ui, "Aceitar", paint_check, Tone::Primary, is_busy(&u.id)) {
                                        clicks.push(Click::Act(u.id.clone(), SocialAction::Accept));
                                    }
                                });
                            }
                        });
                        section(ui, "Seus amigos", g.friends.len(), "Você ainda não tem amigos aqui.", |ui| {
                            for u in &g.friends {
                                user_row(ui, u, presence.get(&u.id), None, |ui| {
                                    if action_button(ui, "Remover", paint_person_remove, Tone::Outline, is_busy(&u.id)) {
                                        clicks.push(Click::Act(u.id.clone(), SocialAction::Remove));
                                    }
                                    if messages && action_button(ui, "Mensagem", icons::paint_chat_bubble, Tone::Outline, false) {
                                        clicks.push(Click::Message(u.id.clone()));
                                    }
                                });
                            }
                        });
                        section(ui, "Pedidos enviados", g.outgoing.len(), "Nenhum pedido enviado.", |ui| {
                            for u in &g.outgoing {
                                user_row(ui, u, presence.get(&u.id), None, |ui| {
                                    if action_button(ui, "Cancelar", icons::paint_close_x, Tone::Outline, is_busy(&u.id)) {
                                        clicks.push(Click::Act(u.id.clone(), SocialAction::Remove));
                                    }
                                });
                            }
                        });
                        section(ui, "Bloqueados", g.blocked.len(), "Ninguém bloqueado.", |ui| {
                            for u in &g.blocked {
                                user_row(ui, u, None, None, |ui| {
                                    if action_button(ui, "Desbloquear", paint_block, Tone::Outline, is_busy(&u.id)) {
                                        clicks.push(Click::Act(u.id.clone(), SocialAction::Unblock));
                                    }
                                });
                            }
                        });
                        ui.add_space(40.0);
                    });
                });
            });
        });

    for click in clicks {
        match click {
            Click::Act(id, action) => {
                if let Some(st) = app.amigos.as_mut() {
                    st.act(&token, ctx, &id, action, false);
                }
            }
            Click::Message(id) => open_direct_message(app, &id),
        }
    }
    if let Some(st) = app.amigos.as_mut() {
        if open_add {
            st.add_open = true;
            st.add_focus = true;
        }
        if back {
            st.page_open = false;
            st.error = None;
        }
    }
}

/// Sem conta: o mesmo aviso do site ("É preciso ter uma conta para adicionar amigos.").
fn render_logged_out_page(app: &mut crate::app::GoLiveLiteApp, ctx: &egui::Context) {
    let mut back = false;
    let mut login = false;
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG_MAIN))
        .show(ctx, |ui| {
            let w = ui.available_width().min(672.0);
            let pad = ((ui.available_width() - w) / 2.0).max(16.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(w - 32.0);
                    ui.add_space(24.0);
                    ui.horizontal(|ui| {
                        let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, Rounding::same(8.0), BG_ELEVATED);
                        }
                        icons::paint_arrow_back(ui.painter(), r.center(), 18.0, ZINC_300);
                        if resp.on_hover_text("Voltar ao início").clicked() {
                            back = true;
                        }
                        ui.label(RichText::new("Amigos").size(24.0).semibold().color(ZINC_50));
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(RichText::new("É preciso ter uma conta para adicionar amigos.").size(14.0).color(TEXT_MUTED));
                        let link = ui.add(
                            egui::Label::new(RichText::new("Entrar com conta").size(14.0).semibold().underline().color(ZINC_100))
                                .sense(Sense::click()),
                        );
                        if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            login = true;
                        }
                    });
                });
            });
        });
    if let Some(st) = app.amigos.as_mut() {
        if back || login {
            st.page_open = false;
        }
    }
    if login {
        app.show_login_box = true;
    }
}

/// "Mensagem" so aparece com a modificacao de mensagens diretas instalada.
pub fn can_message(_app: &crate::app::GoLiveLiteApp) -> bool {
    false
}

fn open_direct_message(_app: &mut crate::app::GoLiveLiteApp, _user_id: &str) {}

/// Dialogo "Adicionar amigo" (`AddFriendDialog`): busca por nome de usuario com atraso de 300 ms.
pub fn render_add_friend_dialog(app: &mut crate::app::GoLiveLiteApp, ctx: &egui::Context) {
    let Some(token) = app.config.account_token.clone() else { return };
    if !app.amigos.as_ref().is_some_and(|s| s.add_open) {
        return;
    }
    let screen = ctx.screen_rect();
    // `bg-black/50` atras; clique fora fecha.
    let backdrop = egui::Area::new(egui::Id::new("amigos_add_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, Rounding::ZERO, Color32::from_black_alpha(128));
            resp.clicked()
        })
        .inner;

    let w = (screen.width() - 32.0).min(448.0);
    let pos = pos2(screen.center().x - w / 2.0, screen.top() + screen.height() * 0.10);
    let mut close = backdrop || ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut clicks: Vec<(String, SocialAction)> = Vec::new();
    let mut query_changed = false;

    // Avatares dos resultados: carregados pelo cache de avatares do app (so os visiveis).
    let hits_avatars: Vec<(String, Option<String>)> = app
        .amigos
        .as_ref()
        .and_then(|s| s.search.as_ref())
        .map(|(_, hits)| hits.iter().map(|h| (h.user.id.clone(), h.user.avatar_url.clone())).collect())
        .unwrap_or_default();
    let mut avatars: HashMap<String, Option<egui::TextureId>> = HashMap::new();
    for (id, url) in hits_avatars {
        avatars.insert(id, app.resolve_and_load_avatar(url.as_deref()));
    }

    let Some(st) = app.amigos.as_mut() else { return };
    egui::Area::new(egui::Id::new("amigos_add_dialog"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            // `max-w-md rounded-2xl border-white/10 bg-zinc-950 p-6 shadow-xl`
            egui::Frame::none()
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(26)))
                .rounding(Rounding::same(16.0))
                .inner_margin(egui::Margin::same(24.0))
                .shadow(egui::epaint::Shadow { offset: vec2(0.0, 12.0), blur: 32.0, spread: 0.0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(w - 48.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new("Adicionar amigo").size(18.0).semibold().color(ZINC_50));
                            ui.label(RichText::new("Procure pelo nome de usuário.").size(12.0).color(TEXT_MUTED));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                            let hov = resp.hovered();
                            if hov {
                                ui.painter().rect_filled(r, Rounding::same(8.0), BG_ELEVATED);
                            }
                            icons::paint_close_x(ui.painter(), r.center(), 20.0, if hov { ZINC_300 } else { TEXT_MUTED });
                            if resp.on_hover_text("Fechar").clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(16.0);

                    // Campo com lupa: `rounded-lg border-zinc-700 bg-zinc-900 py-2.5 pl-9`
                    let field_rect = ui.allocate_space(vec2(ui.available_width(), 40.0)).1;
                    ui.painter().rect(field_rect, Rounding::same(8.0), BG_ELEVATED, Stroke::new(1.0_f32, BORDER_STRONG));
                    paint_search(ui.painter(), pos2(field_rect.left() + 18.0, field_rect.center().y), 16.0, TEXT_MUTED);
                    let edit_rect = Rect::from_min_max(pos2(field_rect.left() + 36.0, field_rect.top() + 4.0), pos2(field_rect.right() - 12.0, field_rect.bottom() - 4.0));
                    let resp = ui.put(
                        edit_rect,
                        egui::TextEdit::singleline(&mut st.add_query)
                            .id(egui::Id::new("amigos_add_query"))
                            .frame(false)
                            .char_limit(32)
                            .hint_text(RichText::new("Ex: maria").color(TEXT_MUTED))
                            .font(FontId::proportional(14.0))
                            .vertical_align(egui::Align::Center),
                    );
                    if std::mem::take(&mut st.add_focus) {
                        resp.request_focus();
                    }
                    if resp.changed() {
                        query_changed = true;
                    }

                    if let Some(err) = st.add_error.clone() {
                        ui.add_space(12.0);
                        egui::Frame::none()
                            .fill(RED_BG)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(err).size(12.0).color(RED_TEXT));
                            });
                    }
                    ui.add_space(12.0);

                    let trimmed = st.add_query.trim().trim_start_matches('@').to_string();
                    let searchable = trimmed.chars().count() >= 2;
                    let current = st.search.as_ref().filter(|(q, _)| *q == trimmed).map(|(_, h)| h.clone());
                    egui::ScrollArea::vertical().max_height(screen.height() * 0.5).min_scrolled_height(96.0).show(ui, |ui| {
                        if !searchable {
                            ui.label(RichText::new("Digite pelo menos duas letras.").size(12.0).color(TEXT_MUTED));
                        } else if let Some(hits) = current {
                            if hits.is_empty() {
                                ui.label(RichText::new("Ninguém encontrado com esse nome.").size(12.0).color(TEXT_MUTED));
                            }
                            ui.spacing_mut().item_spacing.y = 6.0;
                            for h in &hits {
                                let busy = st.busy_id.as_deref() == Some(h.user.id.as_str());
                                let av = avatars.get(&h.user.id).copied().flatten();
                                user_row(ui, &h.user, None, av, |ui| match h.relationship {
                                    Relationship::Friends => {
                                        ui.label(RichText::new("Já é amigo").size(12.0).semibold().color(TEXT_MUTED));
                                    }
                                    Relationship::Outgoing => {
                                        ui.label(RichText::new("Pedido enviado").size(12.0).semibold().color(TEXT_MUTED));
                                    }
                                    Relationship::Incoming => {
                                        if action_button(ui, "Aceitar", paint_check, Tone::Primary, busy) {
                                            clicks.push((h.user.id.clone(), SocialAction::Accept));
                                        }
                                    }
                                    Relationship::None => {
                                        if action_button(ui, "Adicionar", icons::paint_user_plus, Tone::Light, busy) {
                                            clicks.push((h.user.id.clone(), SocialAction::Add));
                                        }
                                    }
                                });
                            }
                        } else {
                            ui.label(RichText::new("Procurando…").size(12.0).color(TEXT_MUTED));
                        }
                    });
                });
        });

    if query_changed {
        st.search_due = Some(Instant::now() + SEARCH_DEBOUNCE);
        ctx.request_repaint_after(SEARCH_DEBOUNCE);
    }
    for (id, action) in clicks {
        st.act(&token, ctx, &id, action, true);
    }
    if close {
        st.add_open = false;
        st.add_query.clear();
        st.add_error = None;
        st.search = None;
        st.search_due = None;
    }
}

/// O botao de amizade do cartao de perfil (`SocialActions`): o verbo que se aplica agora.
pub fn render_profile_friend_action(app: &mut crate::app::GoLiveLiteApp, ui: &mut egui::Ui, user_id: &str) {
    let Some(token) = app.config.account_token.clone() else { return };
    if app.logged_account.as_ref().is_some_and(|a| a.id == user_id) {
        return;
    }
    let ctx = ui.ctx().clone();
    let Some(st) = app.amigos.as_mut() else { return };
    let busy = st.busy_id.as_deref() == Some(user_id);
    let mut click = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        match st.relationship_with(user_id) {
            FriendState::Unknown => {}
            FriendState::None => {
                if action_button(ui, "Adicionar", icons::paint_user_plus, Tone::Light, busy) {
                    click = Some(SocialAction::Add);
                }
            }
            FriendState::Incoming => {
                if action_button(ui, "Aceitar", paint_check, Tone::Primary, busy) {
                    click = Some(SocialAction::Accept);
                }
                if action_button(ui, "Recusar", icons::paint_close_x, Tone::Outline, busy) {
                    click = Some(SocialAction::Remove);
                }
            }
            FriendState::Outgoing => {
                if action_button(ui, "Cancelar pedido", icons::paint_close_x, Tone::Outline, busy) {
                    click = Some(SocialAction::Remove);
                }
            }
            FriendState::Friends => {
                if action_button(ui, "Desfazer amizade", paint_person_remove, Tone::Outline, busy) {
                    click = Some(SocialAction::Remove);
                }
            }
            FriendState::Blocked => {
                if action_button(ui, "Desbloquear", paint_block, Tone::Outline, busy) {
                    click = Some(SocialAction::Unblock);
                }
            }
        }
        // "Bloquear" fica ao lado do verbo principal (no site, dentro de "Opções").
        if !matches!(st.relationship_with(user_id), FriendState::Unknown | FriendState::Blocked)
            && action_button(ui, "Bloquear", paint_block, Tone::Outline, busy)
        {
            click = Some(SocialAction::Block);
        }
    });
    if let Some(action) = click {
        st.act(&token, &ctx, user_id, action, false);
    }
}

/// Uma pessoa pendente vira item do menu: "Amigos" com o numero de pedidos.
pub fn incoming_badge(app: &crate::app::GoLiveLiteApp) -> usize {
    app.amigos.as_ref().map_or(0, |s| s.incoming_count())
}

