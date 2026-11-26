//! Aviso antes de sair do app: todo link que abre um site externo (Loja/Pro, termos, Discord,
//! parceiros, avisos do topo, links de perfil...) passa por aqui e so abre no navegador depois
//! que a pessoa confirma.
//!
//! Custo: uma `String` enquanto o aviso esta aberto e nada depois — sem thread, sem timer.

use crate::ui::theme::SemiBoldExt;
use std::sync::{Mutex, OnceLock};

use egui::{Align, Color32, CursorIcon, Layout, RichText, Rounding, Stroke, Vec2};

use crate::net::protocol::{is_safe_http_url, open_external_url};
use crate::ui::theme::{
    BG_ELEVATED, BG_SURFACE, BLUE, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

static PENDING: Mutex<Option<String>> = Mutex::new(None);
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// Pede para abrir `url` no navegador; o aviso aparece no proximo quadro.
/// URLs que nao sao http(s) sao ignoradas (mesma regra de `open_external_url`).
pub fn request(url: &str) {
    let url = url.trim();
    if !is_safe_http_url(url) {
        return;
    }
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(url.to_string());
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// "golive.nemtudo.me" de "https://golive.nemtudo.me/pro?x=1".
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    host.split(':').next().unwrap_or(host)
}

/// Desenha o aviso (chamado uma vez por quadro, depois do resto da interface).
pub fn render(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
    let Some(url) = PENDING.lock().ok().and_then(|p| p.clone()) else {
        return;
    };

    let mut decision: Option<bool> = None;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        decision = Some(false);
    }

    // Fundo escurecido que tambem bloqueia cliques na interface atras.
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("external_link_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(150));
            if resp.clicked() {
                decision = Some(false);
            }
        });

    egui::Window::new("Você está saindo do Go Live")
        .id(egui::Id::new("external_link_confirm"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .fixed_size(Vec2::new(360.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::same(18.0)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(
                RichText::new("Você será redirecionado para um site externo")
                    .size(15.0)
                    .semibold()
                    .color(TEXT_PRIMARY),
            );
            ui.label(
                RichText::new("O link vai abrir no seu navegador, fora do Go Live - Lite.")
                    .size(12.0)
                    .color(TEXT_SECONDARY),
            );
            egui::Frame::none()
                .fill(BG_ELEVATED)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(host_of(&url)).size(13.0).semibold().color(TEXT_PRIMARY));
                    ui.add(
                        egui::Label::new(RichText::new(&url).size(11.0).color(TEXT_MUTED))
                            .truncate(),
                    )
                    .on_hover_text(&url);
                });
            ui.add_space(4.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let go = ui
                    .add(
                        egui::Button::new(
                            RichText::new("Continuar").size(12.5).semibold().color(Color32::WHITE),
                        )
                        .fill(BLUE)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(104.0, 30.0)),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand);
                if go.clicked() {
                    decision = Some(true);
                }
                let cancel = ui
                    .add(
                        egui::Button::new(RichText::new("Cancelar").size(12.5).color(TEXT_PRIMARY))
                            .fill(BG_ELEVATED)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(90.0, 30.0)),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand);
                if cancel.clicked() {
                    decision = Some(false);
                }
            });
        });

    if let Some(go) = decision {
        if let Ok(mut p) = PENDING.lock() {
            *p = None;
        }
        if go {
            let _ = open_external_url(&url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::host_of;

    #[test]
    fn test_host_of_strips_scheme_path_port_and_userinfo() {
        assert_eq!(host_of("https://golive.nemtudo.me/pro"), "golive.nemtudo.me");
        assert_eq!(host_of("http://user@ex.com:8080/a?b#c"), "ex.com");
        assert_eq!(host_of("https://discord.gg"), "discord.gg");
    }
}
