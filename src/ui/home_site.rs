//! A tela inicial, copiada da home do site (`app/page.tsx` + `components/SiteHeader.tsx`,
//! `RecentRooms.tsx`, `HomeFriendsPanel.tsx`, `SocialLinks.tsx`), no tema escuro classico do
//! site (a home nao usa a paleta da sala nova): pagina `#101014`, cartao zinc-950 `#1a1a1f`,
//! campos zinc-900 `#232329`, bordas zinc-700/800 — cores medidas num print da home real.
//!
//! O que aparece depende das modificacoes instaladas: o painel de Amigos ao lado do cartao so
//! existe com "Amigos" (e uma conta), como o `HomeFriendsPanel` do site, que so aparece logado.

use egui::{pos2, vec2, Align, Align2, Color32, CursorIcon, FontFamily, FontId, Layout, Rect, RichText, Rounding, Sense, Stroke};

use crate::app::{AppSystemTab, GoLiveLiteApp, UserSettingsTab};
use crate::net::protocol::{
    is_private_room_handle, is_valid_room_handle, room_handle_from_input, split_private_room_handle,
    verified_badge_tone, MAX_PRIVATE_ROOM_NAME_LENGTH,
};
use crate::ui::icons;
use crate::ui::svg_icon;
use crate::ui::theme::{SemiBoldExt, SEMIBOLD};

// --- Paleta da home do site (tema escuro classico) ------------------------------------------
const PAGE: Color32 = Color32::from_rgb(16, 16, 20); // #101014
const Z950: Color32 = Color32::from_rgb(26, 26, 31); // #1a1a1f (cartao, pilulas)
const Z900: Color32 = Color32::from_rgb(35, 35, 41); // #232329 (campos, hover)
const Z800: Color32 = Color32::from_rgb(49, 49, 57); // #313139
const Z700: Color32 = Color32::from_rgb(67, 67, 77); // #43434d
const Z600: Color32 = Color32::from_rgb(82, 82, 91);
const Z500: Color32 = Color32::from_rgb(113, 113, 122);
const Z400: Color32 = Color32::from_rgb(161, 161, 170);
const Z300: Color32 = Color32::from_rgb(212, 212, 216);
const Z100: Color32 = Color32::from_rgb(244, 244, 245);
const Z50: Color32 = Color32::from_rgb(250, 250, 250);
const BORDER_WHITE_10: Color32 = Color32::from_rgb(49, 49, 54); // white/10 sobre o cartao
const HEADER_LINE: Color32 = Color32::from_rgb(28, 28, 32); // white/5
const SKY_700: Color32 = Color32::from_rgb(3, 105, 161);
const SKY_600: Color32 = Color32::from_rgb(2, 132, 199);
const SKY_900: Color32 = Color32::from_rgb(12, 74, 110);
const SKY_300: Color32 = Color32::from_rgb(125, 211, 252);
const EMERALD_500: Color32 = Color32::from_rgb(16, 185, 129);
const EMERALD_400: Color32 = Color32::from_rgb(52, 211, 153);
const RED_500: Color32 = Color32::from_rgb(239, 68, 68);

const CARD_W: f32 = 448.0; // max-w-md
const CARD_PAD: f32 = 32.0; // p-8
const FRIENDS_W: f32 = 352.0; // lg:w-88

fn semibold(px: f32) -> FontId {
    FontId::new(px, FontFamily::Name(SEMIBOLD.into()))
}

/// Botao desenhado a mao (para ficar com as medidas exatas do site).
struct Btn<'a> {
    label: &'a str,
    icon: Option<(&'static str, &'static [&'static str])>,
    fill: Color32,
    fill_hover: Color32,
    border: Option<Color32>,
    text: Color32,
    font: FontId,
    height: f32,
    enabled: bool,
}

fn button(ui: &mut egui::Ui, width: f32, b: Btn<'_>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(width, b.height), if b.enabled { Sense::click() } else { Sense::hover() });
    let hov = resp.hovered() && b.enabled;
    let alpha = if b.enabled { 1.0 } else { 0.5 };
    let p = ui.painter();
    p.rect(
        r,
        Rounding::same(8.0),
        (if hov { b.fill_hover } else { b.fill }).gamma_multiply(alpha),
        b.border.map_or(Stroke::NONE, |c| Stroke::new(1.0_f32, c)),
    );
    let galley = ui.fonts(|f| f.layout_no_wrap(b.label.to_string(), b.font.clone(), b.text));
    let icon_w = if b.icon.is_some() { 16.0 + 6.0 } else { 0.0 };
    let total = icon_w + galley.size().x;
    let x0 = r.center().x - total / 2.0;
    if let Some((key, paths)) = b.icon {
        svg_icon::paint(ui.painter(), key, paths, pos2(x0 + 8.0, r.center().y), 16.0, b.text.gamma_multiply(alpha));
    }
    ui.painter().galley(pos2(x0 + icon_w, r.center().y - galley.size().y / 2.0), galley, b.text.gamma_multiply(alpha));
    if b.enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn primary(ui: &mut egui::Ui, w: f32, label: &str, enabled: bool) -> egui::Response {
    // `rounded-lg bg-zinc-50 px-4 py-2.5 font-medium text-zinc-950 hover:bg-zinc-200`
    button(ui, w, Btn { label, icon: None, fill: Z50, fill_hover: Color32::from_rgb(228, 228, 231), border: None, text: Color32::from_rgb(9, 9, 11), font: semibold(16.0), height: 44.0, enabled })
}

fn secondary(ui: &mut egui::Ui, w: f32, label: &str) -> egui::Response {
    // `rounded-lg border border-zinc-700 px-4 py-2.5 font-medium text-zinc-300 hover:bg-zinc-900`
    button(ui, w, Btn { label, icon: None, fill: Color32::TRANSPARENT, fill_hover: Z900, border: Some(Z700), text: Z300, font: semibold(16.0), height: 44.0, enabled: true })
}

/// Campo `rounded-lg border border-zinc-700 bg-zinc-900 px-4 py-2.5 text-zinc-50`.
fn input(ui: &mut egui::Ui, w: f32, text: &mut String, hint: &str, id: &str, max: usize) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
    let id = egui::Id::new(id);
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(rect, Rounding::same(8.0), Z900, Stroke::new(1.0_f32, if focused { Z500 } else { Z700 }));
    // Num filho: o TextEdit nao empurra o layout (o botao ao lado fica no lugar certo).
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
        pos2(rect.left() + 16.0, rect.top() + 2.0),
        pos2(rect.right() - 12.0, rect.bottom() - 2.0),
    )));
    child.add_sized(
        vec2(rect.width() - 28.0, rect.height() - 4.0),
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(false)
            .char_limit(max)
            .font(FontId::proportional(16.0))
            .text_color(Z50)
            .hint_text(RichText::new(hint).color(Z500).size(16.0))
            .vertical_align(Align::Center),
    )
}

fn label(ui: &mut egui::Ui, text: &str) {
    // `text-sm font-medium text-zinc-300`
    ui.label(RichText::new(text).size(14.0).semibold().color(Z300));
}

/// Se a pessoa ja escolheu um nome (o site so mostra o formulario da sala depois disso).
fn has_chosen_name(app: &GoLiveLiteApp) -> bool {
    app.logged_account.is_some() || {
        let n = app.config.display_name.trim();
        !n.is_empty() && n != "UsuarioLite"
    }
}

pub fn render(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    render_header(app, ctx);

    // Modificacao "Amigos": a pagina /friends ocupa o lugar da home.
    #[cfg(feature = "mod_amigos")]
    {
        crate::ui::friends_view::render_add_friend_dialog(app, ctx);
        if app.amigos.as_ref().is_some_and(|s| s.page_open) {
            crate::ui::friends_view::render_friends_page(app, ctx);
            return;
        }
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(PAGE))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_size();
                // Centralizado na vertical como no site (`lg:justify-center`): usa a altura do
                // conteudo do frame anterior.
                let h_id = egui::Id::new("golive_home_content_h");
                let last_h: f32 = ui.ctx().data(|d| d.get_temp(h_id)).unwrap_or(0.0);
                let top = if last_h > 0.0 { ((avail.y - last_h) / 2.0).max(16.0) } else { 16.0 };
                ui.add_space(top);
                let start = ui.cursor().top();
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 12.0); // gap-3
                    render_pills(app, ui);
                    render_row(app, ui);
                    ui.add_space(12.0);
                    render_footer(ui);
                });
                let used = ui.cursor().top() - start;
                ui.ctx().data_mut(|d| d.insert_temp(h_id, used));
                ui.add_space(16.0);
            });
        });
}

// --- Cabecalho (SiteHeader) ------------------------------------------------------------------

fn render_header(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    let mut account_rect: Option<Rect> = None;
    let mut toggled = false;
    egui::TopBottomPanel::top("golive_home_header")
        .exact_height(56.0)
        .frame(egui::Frame::none().fill(PAGE).stroke(Stroke::NONE))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0_f32, HEADER_LINE));
            // `mx-auto max-w-5xl px-4`
            let w = full.width().min(1024.0) - 32.0;
            let inner = Rect::from_center_size(full.center(), vec2(w, 56.0));
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
            ui.spacing_mut().item_spacing.x = 4.0;

            // Logo: icone 20 px + "Go Live - Lite" (`text-base font-semibold`)
            if let Some(tex) = app.brand_icon_tex.as_ref().map(|t| t.id()) {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                ui.painter().image(tex, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Go Live - Lite").size(16.0).semibold().color(Z50));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // Exclusivo do Lite: opcoes do programa (bandeja, atualizacoes, sobre).
                let (r, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, Rounding::same(8.0), Z900);
                }
                icons::paint_dots_vertical(ui.painter(), r.center(), 16.0, if resp.hovered() { Z50 } else { Z400 });
                if resp.on_hover_text("Opções do programa").on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    app.app_system_tab = AppSystemTab::System;
                    app.show_app_system_modal = !app.show_app_system_modal;
                }

                // Conta (AccountMenu): avatar 24 + nome + seta, `rounded-lg border border-zinc-700`
                let name = app
                    .logged_account
                    .as_ref()
                    .map(|a| a.display_name.clone())
                    .or_else(|| has_chosen_name(app).then(|| app.config.display_name.clone()));
                if let Some(name) = name {
                    let shown: String = name.chars().take(12).collect();
                    let tw = ui.fonts(|f| f.layout_no_wrap(shown.clone(), semibold(14.0), Z300).size().x);
                    let (r, resp) = ui.allocate_exact_size(vec2(4.0 + 24.0 + 6.0 + tw + 6.0 + 16.0 + 6.0, 34.0), Sense::click());
                    account_rect = Some(r);
                    let hov = resp.hovered() || app.home_user_menu_open;
                    let p = ui.painter();
                    p.rect(r, Rounding::same(8.0), if hov { Z900 } else { Color32::TRANSPARENT }, Stroke::new(1.0_f32, if hov { Z600 } else { Z700 }));
                    let av = Rect::from_min_size(pos2(r.left() + 4.0, r.center().y - 12.0), vec2(24.0, 24.0));
                    let tex = app.logged_account.as_ref().and_then(|a| a.avatar_url.clone());
                    if app.logged_account.is_some() {
                        if let Some(t) = app.resolve_and_load_avatar(tex.as_deref()) {
                            icons::paint_circular_avatar(ui.painter(), av, t, Stroke::NONE);
                        }
                    } else {
                        // Convidado: inicial num quadrado cinza (`from-zinc-400 to-zinc-500`).
                        ui.painter().rect_filled(av, Rounding::same(6.0), Color32::from_rgb(137, 137, 146));
                        ui.painter().text(av.center(), Align2::CENTER_CENTER, name.chars().next().unwrap_or('?').to_uppercase().to_string(), semibold(11.0), Color32::WHITE);
                    }
                    let p = ui.painter();
                    p.text(pos2(av.right() + 6.0, r.center().y), Align2::LEFT_CENTER, &shown, semibold(14.0), Z300);
                    svg_icon::paint(p, "md_expand_more", svg_icon::MD_EXPAND_MORE, pos2(r.right() - 14.0, r.center().y), 16.0, Z400);
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        if app.logged_account.is_some() {
                            app.home_user_menu_open = !app.home_user_menu_open;
                            toggled = true;
                        } else {
                            app.user_settings_tab = UserSettingsTab::Account;
                            app.show_user_settings_modal = !app.show_user_settings_modal;
                        }
                    }
                }

                crate::ui::top_banners::alerts_bell_button(app, ui, 32.0);

                // Navegacao secundaria (`px-2.5 py-1.5 text-sm text-zinc-400 hover:bg-zinc-900`)
                let nav: [(&str, &str, NavIcon); 4] = [
                    ("Bot para Discord", "https://golive.nemtudo.me/discord-bot", NavIcon::Discord),
                    ("Temas", "https://golive.nemtudo.me/workshop", NavIcon::Palette),
                    ("Pro", "https://golive.nemtudo.me/pro", NavIcon::Pro),
                    ("Square Cloud", "https://go.nemtudo.me/square-link", NavIcon::Square),
                ];
                for (label, url, icon) in nav {
                    let tw = ui.fonts(|f| f.layout_no_wrap(label.into(), FontId::proportional(14.0), Z400).size().x);
                    let (r, resp) = ui.allocate_exact_size(vec2(10.0 + 16.0 + 6.0 + tw + 10.0, 32.0), Sense::click());
                    let hov = resp.hovered();
                    let col = if hov { Z50 } else { Z400 };
                    if hov {
                        ui.painter().rect_filled(r, Rounding::same(8.0), Z900);
                    }
                    let ic = pos2(r.left() + 10.0 + 8.0, r.center().y);
                    match icon {
                        NavIcon::Square => icons::paint_square_cloud(ui.painter(), ic, 15.0, col),
                        NavIcon::Pro => icons::paint_verified_seal(ui.painter(), ic, 6.5, crate::net::protocol::VerifiedTone::Blue),
                        NavIcon::Palette => svg_icon::paint(ui.painter(), "md_palette", svg_icon::MD_PALETTE, ic, 16.0, col),
                        NavIcon::Discord => icons::paint_discord(ui.painter(), ic, 16.0, col),
                    }
                    ui.painter().text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), col);
                    if resp.on_hover_text(url).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        crate::ui::external_link::request(url);
                    }
                }
            });
        });
    if app.home_user_menu_open {
        if let Some(anchor) = account_rect {
            crate::ui::home_view::render_home_user_dropdown_pub(app, ctx, anchor, toggled);
        }
    }
}

#[derive(Clone, Copy)]
enum NavIcon {
    Square,
    Pro,
    Palette,
    Discord,
}

// --- Pilulas acima do cartao -----------------------------------------------------------------

fn render_pills(app: &GoLiveLiteApp, ui: &mut egui::Ui) {
    // `rounded-full border border-zinc-800 bg-zinc-950 px-3.5 py-1.5 text-xs font-medium text-zinc-400`
    let Some(online) = app.people_online else {
        ui.add_space(38.0);
        return;
    };
    let text = if online == 1 { "1 pessoa em salas agora".to_string() } else { format!("{} pessoas em salas agora", online) };
    let g = ui.fonts(|f| f.layout_no_wrap(text, semibold(12.0), Z400));
    let size = vec2(14.0 + 8.0 + 8.0 + g.size().x + 14.0, 30.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - size.x) / 2.0).max(0.0));
        let (r, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter();
        p.rect(r, Rounding::same(15.0), Z950, Stroke::new(1.0_f32, Z800));
        p.circle_filled(pos2(r.left() + 18.0, r.center().y), 4.0, EMERALD_500);
        p.galley(pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, Z400);
    });
}

// --- Linha: [cartao] [amigos] ----------------------------------------------------------------

fn render_row(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    let show_friends = friends_panel_visible(app);
    let avail = ui.available_width();
    let side_by_side = show_friends && avail >= CARD_W + 16.0 + FRIENDS_W + 32.0;
    let card_w = CARD_W.min(avail - 32.0);
    if side_by_side {
        // `lg:flex-row lg:items-stretch gap-4`, o par centralizado
        let total = CARD_W + 16.0 + FRIENDS_W;
        ui.horizontal_top(|ui| {
            ui.add_space(((avail - total) / 2.0).max(0.0));
            ui.spacing_mut().item_spacing.x = 16.0;
            let card = ui.allocate_ui_with_layout(vec2(CARD_W, 0.0), Layout::top_down(Align::Min), |ui| render_card(app, ui, CARD_W)).response.rect;
            ui.allocate_ui_with_layout(vec2(FRIENDS_W, card.height()), Layout::top_down(Align::Min), |ui| render_friends_panel(app, ui, FRIENDS_W, Some(card.height())));
        });
    } else {
        center_block(ui, card_w, |ui| render_card(app, ui, card_w));
        if show_friends {
            center_block(ui, card_w, |ui| render_friends_panel(app, ui, card_w, None));
        }
    }
}

fn center_block(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
        ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), add);
    });
}

fn card_frame() -> egui::Frame {
    // `rounded-2xl border border-white/10 bg-zinc-950 p-8 shadow-sm`
    egui::Frame::none()
        .fill(Z950)
        .stroke(Stroke::new(1.0_f32, BORDER_WHITE_10))
        .rounding(Rounding::same(16.0))
        .inner_margin(egui::Margin::same(CARD_PAD))
}

fn render_card(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, w: f32) {
    card_frame().show(ui, |ui| {
        let inner = w - CARD_PAD * 2.0;
        ui.set_width(inner);
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            // Titulo `text-2xl font-semibold` + `mt-1 text-sm text-zinc-400`
            ui.add(egui::Label::new(RichText::new("Transmitir tela online em grupo, grátis").size(24.0).semibold().color(Z50)).wrap());
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new("Compartilhe sua tela com quem estiver na mesma sala, sem cadastro.").size(14.0).color(Z400)).wrap());
            ui.add_space(12.0);

            // `mt-3`: Ver salas publicas / Ver mapa de salas (borda azul)
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for (label, url, globe) in [
                    ("Ver salas públicas", "https://golive.nemtudo.me/rooms", true),
                    ("Ver mapa de salas", "https://golive.nemtudo.me/worldmap", false),
                ] {
                    let tw = ui.fonts(|f| f.layout_no_wrap(label.into(), semibold(14.0), SKY_300).size().x);
                    let (r, resp) = ui.allocate_exact_size(vec2(14.0 + 16.0 + 6.0 + tw + 14.0, 38.0), Sense::click());
                    let hov = resp.hovered();
                    let p = ui.painter();
                    p.rect(r, Rounding::same(8.0), if hov { SKY_900 } else { Color32::TRANSPARENT }, Stroke::new(1.0_f32, if hov { SKY_600 } else { SKY_700 }));
                    let ic = pos2(r.left() + 14.0 + 8.0, r.center().y);
                    if globe {
                        icons::paint_globe(p, ic, 15.0, SKY_300);
                    } else {
                        svg_icon::paint(p, "md_outline_map", svg_icon::MD_OUTLINE_MAP, ic, 16.0, SKY_300);
                    }
                    p.text(pos2(r.left() + 36.0, r.center().y), Align2::LEFT_CENTER, label, semibold(14.0), SKY_300);
                    if resp.on_hover_text(url).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        crate::ui::external_link::request(url);
                    }
                }
            });
            ui.add_space(32.0); // mt-8

            if has_chosen_name(app) {
                render_room_form(app, ui, inner);
            } else {
                render_landing(app, ui, inner);
            }
        });
    });
}

/// "Seu nome" + Continuar, "ou entre com uma conta", Entrar / Criar conta.
fn render_landing(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, w: f32) {
    let draft_id = egui::Id::new("golive_home_name_draft");
    let mut draft: String = ui.ctx().data(|d| d.get_temp(draft_id)).unwrap_or_default();
    label(ui, "Seu nome");
    ui.add_space(12.0);
    let mut submit = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let btn_w = 100.0;
        let resp = input(ui, w - btn_w - 8.0, &mut draft, "Ex: Maria", "golive_home_name", 24);
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }
        if primary(ui, btn_w, "Continuar", !draft.trim().is_empty()).clicked() {
            submit = true;
        }
    });
    if submit && !draft.trim().is_empty() {
        app.config.display_name = draft.trim().to_string();
        app.config.save();
        draft.clear();
    }
    ui.ctx().data_mut(|d| d.insert_temp(draft_id, draft));

    // AuthDivider: linha, texto `text-xs uppercase tracking-wide text-zinc-500`, linha
    ui.add_space(16.0);
    let (r, _) = ui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
    let g = ui.fonts(|f| f.layout_no_wrap("OU ENTRE COM UMA CONTA".into(), FontId::proportional(12.0), Z500));
    let gw = g.size().x;
    let p = ui.painter();
    p.hline(r.left()..=r.center().x - gw / 2.0 - 12.0, r.center().y, Stroke::new(1.0_f32, Z800));
    p.hline(r.center().x + gw / 2.0 + 12.0..=r.right(), r.center().y, Stroke::new(1.0_f32, Z800));
    p.galley(pos2(r.center().x - gw / 2.0, r.center().y - g.size().y / 2.0), g, Z500);
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let half = (w - 8.0) / 2.0;
        if secondary(ui, half, "Entrar").clicked() {
            app.show_login_box = true;
        }
        if secondary(ui, half, "Criar conta").clicked() {
            crate::ui::external_link::request("https://golive.nemtudo.me/login");
        }
    });
}

/// RecentRooms + "Que tipo de sala?" + Nome da sala + botao (o formulario do site).
fn render_room_form(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, w: f32) {
    // Salas recentes (as 3 ultimas; no site, `flex flex-col gap-2 pt-1.5 pl-1.5`)
    let recent: Vec<String> = app.config.recent_rooms.iter().take(3).cloned().collect();
    if !recent.is_empty() {
        label(ui, "Salas recentes");
        ui.add_space(8.0);
        let mut join: Option<String> = None;
        let mut forget: Option<String> = None;
        for handle in &recent {
            let (name, code) = if is_private_room_handle(handle) {
                split_private_room_handle(handle).map_or((handle.clone(), None), |(n, c)| (n.to_string(), Some(c.to_string())))
            } else {
                (handle.clone(), None)
            };
            let (r, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
            let hov = resp.hovered();
            let p = ui.painter();
            // `rounded-lg border border-zinc-700 px-3.5 py-2 text-sm font-medium text-zinc-300`
            p.rect(r, Rounding::same(8.0), if hov { Z900 } else { Color32::TRANSPARENT }, Stroke::new(1.0_f32, if hov { Z600 } else { Z700 }));
            let ic = pos2(r.left() + 14.0 + 8.0, r.center().y);
            if code.is_some() {
                svg_icon::paint(p, "md_lock", svg_icon::MD_LOCK, ic, 16.0, Z300);
            } else {
                icons::paint_globe(p, ic, 15.0, Z300);
            }
            let right = r.right() - 14.0 - 16.0 - 8.0;
            let code_w = code.as_ref().map_or(0.0, |c| {
                let g = ui.fonts(|f| f.layout_no_wrap(c.clone(), FontId::proportional(14.0), Z500));
                let x = right - g.size().x;
                ui.painter().galley(pos2(x, r.center().y - g.size().y / 2.0), g.clone(), Z500);
                g.size().x + 8.0
            });
            let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), semibold(14.0), Z300);
            job.wrap = egui::text::TextWrapping::truncate_at_width(right - code_w - (r.left() + 38.0));
            let g = ui.fonts(|f| f.layout_job(job));
            ui.painter().galley(pos2(r.left() + 38.0, r.center().y - g.size().y / 2.0), g, Z300);
            svg_icon::paint(ui.painter(), "md_chevron_right", svg_icon::MD_CHEVRON_RIGHT, pos2(r.right() - 14.0 - 8.0, r.center().y), 16.0, Z500);
            // "x" para esquecer (aparece no hover, no canto como no site)
            if hov || ui.rect_contains_pointer(Rect::from_center_size(r.right_top(), vec2(18.0, 18.0))) {
                let xr = Rect::from_center_size(pos2(r.right() - 2.0, r.top() + 2.0), vec2(16.0, 16.0));
                let xresp = ui.interact(xr, ui.id().with(("forget_recent", handle)), Sense::click());
                ui.painter().circle(xr.center(), 8.0, Z950, Stroke::new(1.0_f32, Z600));
                icons::paint_close_x(ui.painter(), xr.center(), 9.0, if xresp.hovered() { Z100 } else { Z400 });
                if xresp.on_hover_text("Remover das salas recentes").clicked() {
                    forget = Some(handle.clone());
                }
            }
            if forget.is_none() && resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                join = Some(handle.clone());
            }
            ui.add_space(8.0);
        }
        if let Some(h) = forget {
            app.config.remove_recent_room(&h);
            app.config.save();
        } else if let Some(h) = join {
            app.join_room_handle(h);
        }
        ui.add_space(4.0);
    }

    // "Que tipo de sala?" + abas Publica / Privada
    label(ui, "Que tipo de sala?");
    ui.add_space(12.0);
    let half = (w - 8.0) / 2.0;
    let tab = |ui: &mut egui::Ui, text: &str, icon: Option<bool>, selected: bool| -> bool {
        // `rounded-lg border px-3 py-2 text-sm font-medium`; selecionada `bg-zinc-50 text-zinc-950`
        let (r, resp) = ui.allocate_exact_size(vec2(half, 38.0), Sense::click());
        let hov = resp.hovered() && !selected;
        let (fill, border, fg) = if selected {
            (Z50, Z50, Color32::from_rgb(9, 9, 11))
        } else {
            (if hov { Z900 } else { Color32::TRANSPARENT }, Z700, Z300)
        };
        let p = ui.painter();
        p.rect(r, Rounding::same(8.0), fill, Stroke::new(1.0_f32, border));
        let g = ui.fonts(|f| f.layout_no_wrap(text.to_string(), semibold(14.0), fg));
        let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
        let x0 = r.center().x - (icon_w + g.size().x) / 2.0;
        match icon {
            Some(true) => icons::paint_globe(ui.painter(), pos2(x0 + 8.0, r.center().y), 15.0, fg),
            Some(false) => svg_icon::paint(ui.painter(), "md_lock", svg_icon::MD_LOCK, pos2(x0 + 8.0, r.center().y), 16.0, fg),
            None => {}
        }
        ui.painter().galley(pos2(x0 + icon_w, r.center().y - g.size().y / 2.0), g, fg);
        resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if tab(ui, "Pública", Some(true), !app.home_room_private) {
            app.home_room_private = false;
            app.home_error = None;
        }
        if tab(ui, "Privada", Some(false), app.home_room_private) {
            app.home_room_private = true;
            app.home_private_create = false;
            app.home_error = None;
        }
    });
    if app.home_room_private {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if tab(ui, "Entrar em sala", None, !app.home_private_create) {
                app.home_private_create = false;
                app.home_error = None;
            }
            if tab(ui, "Criar sala", None, app.home_private_create) {
                app.home_private_create = true;
                app.home_error = None;
            }
        });
    }
    ui.add_space(12.0);

    label(ui, "Nome da sala");
    ui.add_space(12.0);
    let (hint, help, cta) = if !app.home_room_private {
        ("Ex: reuniao-time (ou link)", "Aparece na lista de salas públicas.".to_string(), "Criar sala")
    } else if !app.home_private_create {
        ("Ex: familia-123456 (ou link)", "Cole o nome com o código no fim, como no link que te mandaram.".to_string(), "Entrar na sala")
    } else {
        (
            "Ex: familia",
            "Geramos um código de 6 dígitos e ele vira parte do link. Quem tiver o link entra; a sala não aparece na lista pública.".to_string(),
            "Criar sala privada",
        )
    };
    let max = if app.home_room_private && app.home_private_create { MAX_PRIVATE_ROOM_NAME_LENGTH } else { 200 };
    let resp = input(ui, w, &mut app.room_input, hint, "golive_home_room", max);
    if resp.changed() {
        app.home_error = None;
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(12.0);
    // `text-xs text-zinc-400`
    ui.add(egui::Label::new(RichText::new(help).size(12.0).color(Z400)).wrap());
    if let Some(err) = app.home_error.clone() {
        ui.add_space(12.0);
        ui.add(egui::Label::new(RichText::new(err).size(14.0).color(RED_500)).wrap());
    }
    ui.add_space(20.0); // gap-3 + mt-2

    let can = if !app.home_room_private {
        is_valid_room_handle(&room_handle_from_input(&app.room_input))
    } else if !app.home_private_create {
        let norm = crate::net::protocol::normalize_private_join_handle(&app.room_input);
        !app.room_input.trim().is_empty() && is_valid_room_handle(&norm)
    } else {
        let parsed = room_handle_from_input(&app.room_input);
        let base = parsed.strip_prefix("priv-").unwrap_or(&parsed).to_string();
        !base.is_empty()
            && base.len() <= MAX_PRIVATE_ROOM_NAME_LENGTH
            && base.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if (primary(ui, w, cta, can).clicked() || (enter && can)) && can {
        app.join_room_by_input();
    }
}

// --- Painel de Amigos (HomeFriendsPanel), so com a modificacao ------------------------------

fn friends_panel_visible(app: &GoLiveLiteApp) -> bool {
    #[cfg(feature = "mod_amigos")]
    {
        app.logged_account.is_some() && app.has_modification(crate::config::MOD_AMIGOS)
    }
    #[cfg(not(feature = "mod_amigos"))]
    {
        let _ = app;
        false
    }
}

#[cfg(not(feature = "mod_amigos"))]
fn render_friends_panel(_app: &mut GoLiveLiteApp, _ui: &mut egui::Ui, _w: f32, _h: Option<f32>) {}

#[cfg(feature = "mod_amigos")]
fn render_friends_panel(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, w: f32, min_h: Option<f32>) {
    if app.amigos.is_none() {
        app.start_amigos();
    }
    let (friends, incoming) = app
        .amigos
        .as_ref()
        .and_then(|s| s.graph.as_ref())
        .map(|g| (Some(g.friends.clone()), g.incoming.len()))
        .unwrap_or((None, 0));
    let mut open_page = false;
    let mut open_add = false;
    let mut open_profile: Option<String> = None;
    // `rounded-2xl border border-white/10 bg-zinc-950 p-5 shadow-sm`
    egui::Frame::none()
        .fill(Z950)
        .stroke(Stroke::new(1.0_f32, BORDER_WHITE_10))
        .rounding(Rounding::same(16.0))
        .inner_margin(egui::Margin::same(20.0))
        .show(ui, |ui| {
            ui.set_width(w - 40.0);
            if let Some(h) = min_h {
                ui.set_min_height(h - 40.0);
            }
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Amigos").size(14.0).semibold().color(Z100));
                if let Some(f) = friends.as_ref().filter(|f| !f.is_empty()) {
                    ui.label(RichText::new(f.len().to_string()).size(14.0).color(Z400));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let all = ui.add(egui::Label::new(RichText::new("Ver todos").size(12.0).semibold().underline().color(Z400)).sense(Sense::click()));
                    if all.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        open_page = true;
                    }
                    // `h-7 w-7 rounded-lg border border-zinc-700`
                    let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    ui.painter().rect(r, Rounding::same(8.0), if resp.hovered() { Z900 } else { Color32::TRANSPARENT }, Stroke::new(1.0_f32, Z700));
                    svg_icon::paint(ui.painter(), "md_person_add", svg_icon::MD_PERSON_ADD, r.center(), 16.0, Z400);
                    if resp.on_hover_text("Adicionar amigo").on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        open_add = true;
                    }
                });
            });
            if incoming > 0 {
                ui.add_space(6.0);
                let text = if incoming == 1 { "1 pedido de amizade esperando".to_string() } else { format!("{} pedidos de amizade esperando", incoming) };
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                let p = ui.painter();
                p.rect(r, Rounding::same(8.0), Color32::from_rgba_unmultiplied(6, 78, 59, if resp.hovered() { 150 } else { 90 }), Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(16, 185, 129, 77)));
                p.text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, text, semibold(12.0), EMERALD_400);
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    open_page = true;
                }
            }
            ui.add_space(6.0);
            match friends {
                None => {
                    ui.label(RichText::new("Carregando…").size(12.0).color(Z400));
                }
                Some(list) if list.is_empty() => {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.label(RichText::new("Você ainda não tem amigos aqui.").size(12.0).color(Z400));
                        let link = ui.add(egui::Label::new(RichText::new("Adicionar alguém").size(12.0).semibold().underline().color(Z400)).sense(Sense::click()));
                        if link.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            open_add = true;
                        }
                        ui.label(RichText::new(", ou adicione pelo perfil e pela lista de participantes de uma sala.").size(12.0).color(Z400));
                    });
                }
                Some(list) => {
                    egui::ScrollArea::vertical().max_height(26.0 * 16.0).show(ui, |ui| {
                        for u in &list {
                            // `rounded-lg border border-zinc-800 bg-zinc-950 px-2.5 py-2`, avatar 32
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
                            ui.painter().rect(r, Rounding::same(8.0), if resp.hovered() { Z900 } else { Z950 }, Stroke::new(1.0_f32, Z800));
                            let av = Rect::from_min_size(pos2(r.left() + 10.0, r.center().y - 16.0), vec2(32.0, 32.0));
                            if let Some(t) = app.resolve_and_load_avatar(u.avatar_url.as_deref()) {
                                icons::paint_circular_avatar(ui.painter(), av, t, Stroke::NONE);
                            }
                            let color = crate::net::protocol::resolve_name_color_rgb(u.name_color.as_deref())
                                .map_or(Z100, |[r, g, b]| Color32::from_rgb(r, g, b));
                            let name_g = ui.fonts(|f| f.layout_no_wrap(u.display_name.clone(), semibold(14.0), color));
                            let nx = av.right() + 10.0;
                            let name_w = name_g.size().x;
                            ui.painter().galley(pos2(nx, r.top() + 8.0), name_g, color);
                            if let Some(tone) = verified_badge_tone(&u.flags) {
                                icons::paint_verified_seal(ui.painter(), pos2(nx + name_w + 9.0, r.top() + 17.0), 5.4, tone);
                            }
                            ui.painter().text(pos2(nx, r.top() + 28.0), Align2::LEFT_TOP, format!("@{}", u.username), FontId::proportional(12.0), Z400);
                            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                open_profile = Some(u.id.clone());
                            }
                            ui.add_space(6.0);
                        }
                    });
                }
            }
        });
    if let Some(st) = app.amigos.as_mut() {
        if open_page {
            st.page_open = true;
        }
        if open_add {
            st.add_open = true;
        }
    }
    #[cfg(feature = "mod_perfil")]
    if let Some(id) = open_profile {
        app.open_profile_account(id);
    }
    #[cfg(not(feature = "mod_perfil"))]
    let _ = open_profile;
}

// --- Rodape (SocialLinks + links + Square Cloud) ---------------------------------------------

fn render_footer(ui: &mut egui::Ui) {
    let links: [(&str, &str, fn(&egui::Painter, egui::Pos2, f32, Color32)); 3] = [
        ("discord.gg/nemtudo", "https://discord.gg/nemtudo", icons::paint_discord),
        ("@NemTudo_", "https://x.com/NemTudo_", icons::paint_x_twitter),
        ("@onemtudo", "https://instagram.com/onemtudo", icons::paint_instagram),
    ];
    // `mt-6 flex justify-center gap-2`; pilulas `rounded-full border border-zinc-700 px-3.5 py-1.5 text-sm`
    let sizes: Vec<f32> = links
        .iter()
        .map(|(l, _, _)| 14.0 + 16.0 + 8.0 + ui.fonts(|f| f.layout_no_wrap((*l).into(), FontId::proportional(14.0), Z400).size().x) + 14.0)
        .collect();
    let total: f32 = sizes.iter().sum::<f32>() + 8.0 * (links.len() as f32 - 1.0);
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
        for ((label, url, icon), w) in links.iter().zip(sizes) {
            let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
            let hov = resp.hovered();
            let col = if hov { Z100 } else { Z400 };
            ui.painter().rect(r, Rounding::same(17.0), Color32::TRANSPARENT, Stroke::new(1.0_f32, if hov { Z600 } else { Z700 }));
            icon(ui.painter(), pos2(r.left() + 14.0 + 8.0, r.center().y), 16.0, col);
            ui.painter().text(pos2(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(14.0), col);
            if resp.on_hover_text(*url).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                crate::ui::external_link::request(url);
            }
        }
    });

    // `mt-4 flex gap-5 text-xs text-zinc-600 underline` + "Powered by Square Cloud"
    ui.add_space(8.0);
    let l1 = "Como transmitir a tela online";
    let l2 = "Termos de uso";
    let w1 = ui.fonts(|f| f.layout_no_wrap(l1.into(), FontId::proportional(12.0), Z600).size().x);
    let w2 = ui.fonts(|f| f.layout_no_wrap(l2.into(), FontId::proportional(12.0), Z600).size().x);
    let powered_w = 200.0;
    let total = w1 + 20.0 + w2 + 20.0 + powered_w;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
        for (text, url) in [(l1, "https://golive.nemtudo.me/transmitir-tela-online"), (l2, "https://golive.nemtudo.me/terms")] {
            let resp = ui.add(egui::Label::new(RichText::new(text).size(12.0).underline().color(Z600)).sense(Sense::click()));
            if resp.on_hover_text(url).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                crate::ui::external_link::request(url);
            }
        }
        // O selo "Powered by Square Cloud" (fundo preto, logo + texto)
        let (r, resp) = ui.allocate_exact_size(vec2(powered_w, 36.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(r, Rounding::same(6.0), Color32::from_rgb(9, 9, 11));
        icons::paint_square_cloud(p, pos2(r.left() + 18.0, r.center().y), 18.0, Color32::WHITE);
        let mut job = egui::text::LayoutJob::default();
        job.append("Powered by ", 0.0, egui::TextFormat::simple(FontId::proportional(12.0), Color32::WHITE));
        job.append("Square Cloud", 0.0, egui::TextFormat::simple(semibold(12.0), Color32::WHITE));
        let g = ui.fonts(|f| f.layout_job(job));
        p.galley(pos2(r.left() + 38.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
        if resp.on_hover_text("https://go.nemtudo.me/square-link").on_hover_cursor(CursorIcon::PointingHand).clicked() {
            crate::ui::external_link::request("https://go.nemtudo.me/square-link");
        }
    });
}
