//! Top Warns UI (`StatusBanner` outage warning + live WS `AnnouncementBanner`).

use crate::ui::theme::SemiBoldExt;
use egui::{Align, Color32, CursorIcon, Layout, RichText, Rounding, Stroke, Vec2};

use crate::app::GoLiveLiteApp;
use crate::net::protocol::is_safe_http_url;

/// Renders Top Warns at the top of the window (`StatusBanner` first, then `AnnouncementBanner`).
pub fn render_top_warns(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    // 1. Site instavel (StatusBanner.tsx): vermelho, acima de tudo, sem botao de fechar.
    if let Some(status) = app.site_status.as_ref().filter(|s| s.should_display()) {
        let msg: String = status.message.trim().chars().take(300).collect();
        let btn = status.button.clone().filter(|b| {
            b.enabled && !b.label.trim().is_empty() && is_safe_http_url(b.href.trim())
        });

        egui::TopBottomPanel::top("golive_status_banner")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(220, 38, 38)) // bg-red-600
                    .inner_margin(egui::Margin::symmetric(16.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(ref b) = btn {
                        let label: String = b.label.trim().chars().take(40).collect();
                        let button = egui::Button::new(
                            RichText::new(label).size(11.5).semibold().color(Color32::WHITE),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(
                            1.0_f32,
                            Color32::from_rgba_unmultiplied(255, 255, 255, 180),
                        ))
                        .rounding(Rounding::same(6.0));
                        if ui
                            .add(button)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            crate::ui::external_link::request(b.href.trim());
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let (badge_rect, _) =
                            ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                        paint_warning_circle(ui.painter(), badge_rect, Color32::WHITE);
                        ui.add(
                            egui::Label::new(
                                RichText::new(msg).size(12.5).semibold().color(Color32::WHITE),
                            )
                            .wrap(),
                        );
                    });
                }));
            });
    }

    // 2. Aviso ao vivo do admin (AnnouncementBar.tsx).
    if let Some(ann) = app.active_announcement() {
        let (bg_arr, fg_arr) = ann.color_preset_rgb();
        let bg = Color32::from_rgb(bg_arr[0], bg_arr[1], bg_arr[2]);
        let fg = Color32::from_rgb(fg_arr[0], fg_arr[1], fg_arr[2]);

        egui::TopBottomPanel::top("golive_announcement_banner")
            .frame(
                egui::Frame::none()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(16.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if ann.dismissible {
                        let close_btn = egui::Button::new(
                            RichText::new("×").size(17.0).color(fg.gamma_multiply(0.85)),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new(20.0, 20.0));
                        if ui
                            .add(close_btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text("Fechar aviso")
                            .clicked()
                        {
                            app.dismiss_announcement(&ann.id);
                        }
                    }

                    if ann.has_button && !ann.button_label.trim().is_empty() {
                        let can_act = ann.button_action.trim() == "reload"
                            || is_safe_http_url(ann.button_url_str());
                        if can_act {
                            let cta = egui::Button::new(
                                RichText::new(ann.button_label.trim())
                                    .size(11.5)
                                    .semibold()
                                    .color(fg),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0_f32, fg))
                            .rounding(Rounding::same(6.0));
                            if ui
                                .add(cta)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                app.click_announcement_button(&ann);
                            }
                        }
                    }

                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(ann.text.trim()).size(12.5).semibold().color(fg),
                            )
                            .wrap(),
                        );
                    });
                }));
            });
    }
}

pub fn paint_warning_circle(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let r = (rect.width().min(rect.height()) * 0.44).max(5.0);
    let c = rect.center();
    painter.circle_stroke(c, r, Stroke::new(1.5_f32, color));
    painter.line_segment(
        [egui::pos2(c.x, c.y - r * 0.46), egui::pos2(c.x, c.y + r * 0.14)],
        Stroke::new(1.6_f32, color),
    );
    painter.circle_filled(egui::pos2(c.x, c.y + r * 0.48), 1.1, color);
}

/// Ha algo para ver na central de avisos? (bolinha vermelha no sino)
pub fn has_pending_alerts(app: &GoLiveLiteApp) -> bool {
    app.active_announcement().is_some()
        || app.site_status.as_ref().is_some_and(|s| s.should_display())
        || app.available_update.is_some()
}

/// Sino da central de avisos (mesmo botao na tela inicial e na sala).
pub fn alerts_bell_button(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, size: f32) {
    let has_alert = has_pending_alerts(app);
    let resp = crate::ui::icons::icon_button(
        ui,
        Vec2::splat(size),
        if app.show_alerts_popover {
            crate::ui::theme::BG_ELEVATED
        } else {
            Color32::TRANSPARENT
        },
        Stroke::NONE,
        Rounding::same(8.0),
        "Central de avisos",
        |p, c, col| {
            crate::ui::icons::paint_bell(p, c, 14.5, col, false);
            if has_alert {
                p.circle_filled(egui::pos2(c.x + 5.0, c.y - 5.0), 3.0, crate::ui::theme::RED);
            }
        },
        if has_alert {
            crate::ui::theme::TEXT_PRIMARY
        } else {
            crate::ui::theme::TEXT_SECONDARY
        },
    );
    if resp.clicked() {
        app.show_alerts_popover = !app.show_alerts_popover;
    }
}

enum AlertAction {
    OpenUrl(String),
    AnnouncementButton(crate::net::protocol::Announcement),
    Dismiss(String),
    Restore,
    InstallUpdate(crate::net::updater::AvailableUpdate),
}

/// Central de avisos: os mesmos avisos do topo, com os botoes deles, mais o ultimo aviso
/// dispensado (para reabrir) e a atualizacao do app.
pub fn render_alerts_center(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    use crate::ui::theme::{
        centered_movable_window, BG_ELEVATED, BG_SURFACE, BORDER_SUBTLE, GOLD, TEXT_MUTED,
        TEXT_PRIMARY, TEXT_SECONDARY,
    };
    if !app.show_alerts_popover {
        return;
    }

    fn card(
        ui: &mut egui::Ui,
        accent: Color32,
        title: &str,
        body: &str,
        buttons: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Frame::none()
            .fill(BG_ELEVATED)
            .stroke(Stroke::new(1.0_f32, accent.gamma_multiply(0.55)))
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).size(11.0).semibold().color(accent));
                ui.add(egui::Label::new(RichText::new(body).size(12.5).color(TEXT_PRIMARY)).wrap());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    buttons(ui);
                });
            });
        ui.add_space(6.0);
    }
    fn small(ui: &mut egui::Ui, label: &str, primary: bool) -> bool {
        ui.add(
            egui::Button::new(RichText::new(label).size(11.5).color(if primary {
                Color32::WHITE
            } else {
                TEXT_SECONDARY
            }))
            .fill(if primary {
                crate::ui::theme::BLUE
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(6.0)),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked()
    }

    let status = app.site_status.clone().filter(|s| s.should_display());
    let active = app.active_announcement();
    let dismissed = if active.is_none() {
        app.dismissed_announcement.clone()
    } else {
        None
    };
    let update = app.available_update.clone();
    let mut action: Option<AlertAction> = None;
    let mut open = true;

    centered_movable_window("Central de avisos", "golive_alerts_popover_modal", ctx, Vec2::new(360.0, 240.0))
        .open(&mut open)
        .default_width(360.0)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(14.0)),
        )
        .show(ctx, |ui| {
            let mut any = false;
            if let Some(ref st) = status {
                any = true;
                card(ui, Color32::from_rgb(248, 113, 113), "INSTABILIDADE NO GO LIVE", st.message.trim(), |ui| {
                    if let Some(b) = st.button.as_ref().filter(|b| {
                        b.enabled && !b.label.trim().is_empty() && is_safe_http_url(b.href.trim())
                    }) {
                        if small(ui, b.label.trim(), true) {
                            action = Some(AlertAction::OpenUrl(b.href.trim().to_string()));
                        }
                    }
                });
            }
            if let Some(ref ann) = active {
                any = true;
                card(ui, Color32::from_rgb(52, 211, 153), "AVISO NO TOPO", ann.text.trim(), |ui| {
                    if ann.has_button && !ann.button_label.trim().is_empty() && small(ui, ann.button_label.trim(), true) {
                        action = Some(AlertAction::AnnouncementButton(ann.clone()));
                    }
                    if ann.dismissible && small(ui, "Fechar aviso", false) {
                        action = Some(AlertAction::Dismiss(ann.id.clone()));
                    }
                });
            } else if let Some(ref ann) = dismissed {
                any = true;
                card(ui, TEXT_MUTED, "AVISO FECHADO", ann.text.trim(), |ui| {
                    if small(ui, "Mostrar no topo", false) {
                        action = Some(AlertAction::Restore);
                    }
                    if ann.has_button && !ann.button_label.trim().is_empty() && small(ui, ann.button_label.trim(), true) {
                        action = Some(AlertAction::AnnouncementButton(ann.clone()));
                    }
                });
            }
            if let Some(ref upd) = update {
                any = true;
                let body = format!("Go Live - Lite v{} está disponível.", upd.version);
                card(ui, GOLD, "ATUALIZAÇÃO", &body, |ui| {
                    if small(ui, "Atualizar agora", true) {
                        action = Some(AlertAction::InstallUpdate(upd.clone()));
                    }
                });
            }
            if !any {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Nenhum aviso no momento.").size(12.5).color(TEXT_MUTED));
                });
                ui.add_space(6.0);
            }
        });

    match action {
        Some(AlertAction::OpenUrl(url)) => crate::ui::external_link::request(&url),
        Some(AlertAction::AnnouncementButton(ann)) => app.click_announcement_button(&ann),
        Some(AlertAction::Dismiss(id)) => app.dismiss_announcement(&id),
        Some(AlertAction::Restore) => app.restore_dismissed_announcement(),
        Some(AlertAction::InstallUpdate(upd)) => {
            app.show_alerts_popover = false;
            app.start_update_install(&upd);
        }
        None => {}
    }
    if !open {
        app.show_alerts_popover = false;
    }
}
