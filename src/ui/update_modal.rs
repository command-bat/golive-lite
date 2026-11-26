//! UI leve para atualizacoes opcionais (banner discreto) e obrigatorias/forcadas (overlay bloqueante).

use crate::ui::theme::SemiBoldExt;
use egui::{Align, Color32, CursorIcon, Layout, Order, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::GoLiveLiteApp;
use crate::net::protocol::is_safe_http_url;
use crate::net::updater::UpdateDownloadState;
use crate::ui::theme::{
    centered_movable_window, BG_ELEVATED, BG_SURFACE, BLUE, BORDER_STRONG, BORDER_SUBTLE, EMERALD,
    RED, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

pub fn render_optional_update_banner(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    let Some(update) = app.available_update.clone() else {
        return;
    };
    if update.is_mandatory || app.update_dismissed_session {
        return;
    }
    if app
        .config
        .skipped_update_version
        .as_deref()
        .is_some_and(|v| v == update.version)
    {
        return;
    }

    egui::TopBottomPanel::top("golive_optional_update_banner")
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(20, 48, 86))
                .stroke(Stroke::new(1.0_f32, BLUE))
                .inner_margin(egui::Margin::symmetric(12.0, 5.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Nova versao do Go Live - Lite disponivel: v{} (atual: v{})",
                        update.version, update.current_version
                    ))
                    .size(12.5)
                    .semibold()
                    .color(Color32::WHITE),
                );

                match &app.update_state {
                    UpdateDownloadState::Downloading { downloaded, total } => {
                        let txt = format_progress(*downloaded, *total);
                        ui.label(
                            RichText::new(format!("Baixando... {}", txt))
                                .size(12.0)
                                .color(Color32::from_rgb(191, 219, 254)),
                        );
                    }
                    UpdateDownloadState::Error(err) => {
                        ui.label(
                            RichText::new(err)
                                .size(11.5)
                                .color(Color32::from_rgb(254, 202, 202)),
                        );
                    }
                    _ => {}
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Ignorar versao")
                                    .size(11.0)
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 55))
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        app.skip_optional_update(&update.version);
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Depois").size(11.5).color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 22))
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        app.update_dismissed_session = true;
                    }

                    let busy = matches!(app.update_state, UpdateDownloadState::Downloading { .. });
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(
                                RichText::new(if busy {
                                    "Baixando..."
                                } else {
                                    "Atualizar agora"
                                })
                                .size(11.5)
                                .semibold()
                                .color(Color32::WHITE),
                            )
                            .fill(EMERALD)
                            .rounding(Rounding::same(6.0)),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        app.start_update_install(&update);
                    }
                });
            });
        });
}

pub fn render_mandatory_update_overlay(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    let Some(update) = app.available_update.clone() else {
        return;
    };
    if !update.is_mandatory {
        return;
    }

    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("mandatory_update_backdrop"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(205));
        });

    centered_movable_window(
        "Atualizacao obrigatoria",
        "mandatory_update_modal",
        ctx,
        Vec2::new(430.0, 260.0),
    )
    .order(Order::Foreground)
    .title_bar(false)
    .fixed_size(Vec2::new(430.0, 260.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.5_f32, BORDER_STRONG))
                .rounding(Rounding::same(16.0))
                .inner_margin(egui::Margin::same(22.0)),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                egui::Frame::none()
                    .fill(RED.linear_multiply(0.22))
                    .stroke(Stroke::new(1.0_f32, RED))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("ATUALIZACAO OBRIGATORIA")
                                .size(11.0)
                                .semibold()
                                .color(Color32::from_rgb(254, 202, 202)),
                        );
                    });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!(
                        "Go Live - Lite v{} necessario para continuar",
                        update.version
                    ))
                    .size(17.0)
                    .semibold()
                    .color(TEXT_PRIMARY),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Sua versao atual (v{}) precisa ser atualizada para manter compatibilidade com os servidores.",
                        update.current_version
                    ))
                    .size(12.5)
                    .color(TEXT_SECONDARY),
                );
            });

            if let Some(ref notes) = update.notes {
                ui.add_space(10.0);
                egui::Frame::none()
                    .fill(BG_ELEVATED)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(notes).size(12.0).color(TEXT_PRIMARY));
                    });
            }

            ui.add_space(14.0);
            match &app.update_state {
                UpdateDownloadState::Downloading { downloaded, total } => {
                    let frac = total
                        .filter(|t| *t > 0)
                        .map(|t| (*downloaded as f32 / t as f32).clamp(0.0, 1.0))
                        .unwrap_or(0.3);
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .text(format!("Baixando atualizacao... {}", format_progress(*downloaded, *total))),
                    );
                }
                UpdateDownloadState::ReadyToRestart => {
                    ui.label(
                        RichText::new("Aplicando atualizacao e reiniciando...")
                            .size(12.5)
                            .semibold()
                            .color(EMERALD),
                    );
                }
                UpdateDownloadState::Error(err) => {
                    ui.label(
                        RichText::new(err)
                            .size(12.0)
                            .color(Color32::from_rgb(252, 165, 165)),
                    );
                    ui.add_space(8.0);
                }
                _ => {}
            }

            if !matches!(
                app.update_state,
                UpdateDownloadState::Downloading { .. } | UpdateDownloadState::ReadyToRestart
            ) {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Atualizar agora")
                                .size(14.0)
                                .semibold()
                                .color(Color32::WHITE),
                        )
                        .fill(EMERALD)
                        .rounding(Rounding::same(10.0))
                        .min_size(Vec2::new(ui.available_width(), 38.0)),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .clicked()
                {
                    app.start_update_install(&update);
                }

                let fallback_url = update
                    .release_page_url
                    .as_deref()
                    .filter(|u| is_safe_http_url(u))
                    .or_else(|| {
                        if is_safe_http_url(&update.download_url) {
                            Some(update.download_url.as_str())
                        } else {
                            None
                        }
                    });
                if let Some(url) = fallback_url {
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        if ui
                            .small_button(
                                RichText::new("Abrir pagina de download manual")
                                    .size(11.5)
                                    .color(TEXT_MUTED),
                            )
                            .clicked()
                        {
                            crate::ui::external_link::request(url);
                        }
                    });
                }
            }
        });
}

fn format_progress(downloaded: u64, total: Option<u64>) -> String {
    let dl_mb = downloaded as f64 / (1024.0 * 1024.0);
    match total {
        Some(t) if t > 0 => {
            let tot_mb = t as f64 / (1024.0 * 1024.0);
            format!("{:.1} / {:.1} MB", dl_mb, tot_mb)
        }
        _ => format!("{:.1} MB", dl_mb),
    }
}
