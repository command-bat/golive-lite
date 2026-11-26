use crate::ui::theme::SemiBoldExt;
use egui::{pos2, Align, Color32, CursorIcon, Layout, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::{GoLiveLiteApp, SharePickerTab, ShareTarget};
use crate::net::protocol::{DegradationMode, ShareBitrate, ShareFps, ShareResolution};
use crate::ui::theme::{
    centered_movable_window, BG_ELEVATED, BG_MAIN, BG_SURFACE, BG_TILE, BLUE, BORDER_STRONG,
    BORDER_SUBTLE, EMERALD, EMERALD_BRIGHT, RED, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

fn is_already_shared(app: &GoLiveLiteApp, cand: &ShareTarget) -> bool {
    app.active_screen_slots
        .iter()
        .any(|w| w.target.same_source(cand))
}

/// Modal dedicado APENAS para selecionar qual Tela ou Janela compartilhar (com grid visual de previews).
pub fn render_share_picker_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_share_settings_modal {
        return;
    }

    let tier = app.current_plan_tier();
    let is_pro = tier.is_pro();
    let max_screens = tier.max_screens();
    let active_count = app.active_screen_slots.len();
    let at_limit = active_count >= max_screens;
    let mut open = true;
    let mut start_now = false;
    let mut close_now = false;

    centered_movable_window(
        "Compartilhar Tela ou Janela",
        "golive_share_picker_modal",
        ctx,
        Vec2::new(540.0, 460.0),
    )
    .open(&mut open)
    .default_width(540.0)
    .frame(
        egui::Frame::window(&ctx.style())
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(12.0)),
    )
    .show(ctx, |ui| {
        ui.set_width(516.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Plano: {}", tier.label()))
                    .size(12.5)
                    .semibold()
                    .color(if is_pro { EMERALD_BRIGHT } else { TEXT_SECONDARY }),
            );
            ui.label(
                RichText::new(format!(
                    "• Compartilhando: {}/{}",
                    active_count, max_screens
                ))
                .size(12.0)
                .color(TEXT_MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button("Atualizar lista")
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .clicked()
                {
                    app.refresh_share_sources();
                    app.spawn_share_thumbnails_capture();
                }
            });
        });
        ui.separator();

        if at_limit {
            egui::Frame::none()
                .fill(RED.linear_multiply(0.18))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Limite de {}/{} telas simultaneas atingido. Pare uma transmissao para adicionar outra.",
                            active_count, max_screens
                        ))
                        .size(12.0)
                        .color(Color32::from_rgb(252, 165, 165)),
                    );
                });
            ui.add_space(4.0);
        }

        // Abas Telas / Janelas
        ui.horizontal(|ui| {
            let mon_lbl = format!("Telas ({})", app.available_monitors.len());
            let win_lbl = format!("Janelas ({})", app.available_windows.len());
            if ui
                .selectable_label(app.share_picker_tab == SharePickerTab::Monitors, mon_lbl)
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                app.share_picker_tab = SharePickerTab::Monitors;
            }
            if ui
                .selectable_label(app.share_picker_tab == SharePickerTab::Windows, win_lbl)
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                app.share_picker_tab = SharePickerTab::Windows;
            }
        });
        ui.add_space(4.0);

        // Grade 2 colunas com miniatura ao vivo das telas / janelas
        egui::Frame::none()
            .fill(BG_ELEVATED)
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| match app.share_picker_tab {
                        SharePickerTab::Monitors => {
                            if app.available_monitors.is_empty() {
                                ui.label(RichText::new("Nenhum monitor detectado.").color(RED));
                            } else {
                                let monitors = app.available_monitors.clone();
                                for pair in monitors.chunks(2) {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        for mon in pair {
                                            let cand = ShareTarget::Monitor(mon.clone());
                                            let thumb_key = format!("mon:{}", mon.index);
                                            let thumb_tex = app.share_thumb_textures.get(&thumb_key);
                                            let is_selected = app
                                                .picker_selected_target
                                                .as_ref()
                                                .is_some_and(|s| s.same_source(&cand));
                                            let active_already = is_already_shared(app, &cand);
                                            let subtitle = if mon.refresh_hz > 0 {
                                                format!("{}x{} @ {}Hz", mon.width, mon.height, mon.refresh_hz)
                                            } else {
                                                format!("{}x{}", mon.width, mon.height)
                                            };
                                            let title = if mon.is_primary {
                                                format!("Tela {} (Principal)", mon.index + 1)
                                            } else {
                                                format!("Tela {}", mon.index + 1)
                                            };
                                            let resp = render_preview_source_card(
                                                ui,
                                                &title,
                                                &subtitle,
                                                thumb_tex.map(|t| (t.id(), t.size_vec2())),
                                                is_selected,
                                                active_already,
                                                !at_limit && !active_already,
                                            );
                                            if resp.clicked() && !at_limit && !active_already {
                                                app.picker_selected_target = Some(cand.clone());
                                            }
                                            if resp.double_clicked() && !at_limit && !active_already {
                                                app.picker_selected_target = Some(cand);
                                                start_now = true;
                                            }
                                        }
                                    });
                                    ui.add_space(6.0);
                                }
                            }
                        }
                        SharePickerTab::Windows => {
                            if app.available_windows.is_empty() {
                                ui.label(
                                    RichText::new(
                                        "Nenhuma janela aberta detectada (clique em Atualizar lista).",
                                    )
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                                );
                            } else {
                                let windows = app.available_windows.clone();
                                for pair in windows.chunks(2) {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        for win in pair {
                                            let cand = ShareTarget::Window(win.clone());
                                            let thumb_key = format!("win:{}", win.hwnd);
                                            let thumb_tex = app.share_thumb_textures.get(&thumb_key);
                                            let is_selected = app
                                                .picker_selected_target
                                                .as_ref()
                                                .is_some_and(|s| s.same_source(&cand));
                                            let active_already = is_already_shared(app, &cand);
                                            let title = if win.process_name.is_empty() {
                                                win.title.clone()
                                            } else {
                                                format!("{} — {}", win.process_name, win.title)
                                            };
                                            let subtitle = format!("{}x{}", win.width, win.height);
                                            let resp = render_preview_source_card(
                                                ui,
                                                &title,
                                                &subtitle,
                                                thumb_tex.map(|t| (t.id(), t.size_vec2())),
                                                is_selected,
                                                active_already,
                                                !at_limit && !active_already,
                                            );
                                            if resp.clicked() && !at_limit && !active_already {
                                                app.picker_selected_target = Some(cand.clone());
                                            }
                                            if resp.double_clicked() && !at_limit && !active_already {
                                                app.picker_selected_target = Some(cand);
                                                start_now = true;
                                            }
                                        }
                                    });
                                    ui.add_space(6.0);
                                }
                            }
                        }
                    });
            });

        // Somente som da transmissao (qualidade agora fica no menu proprio "Qualidade da Transmissao")
        ui.add_space(8.0);
        if ui
            .checkbox(
                &mut app.config.share_audio,
                RichText::new("Compartilhar com som (audio da tela/janela)")
                    .size(12.5)
                    .semibold()
                    .color(TEXT_PRIMARY),
            )
            .changed()
        {
            app.config.save();
        }
        let is_window_selected = matches!(
            app.picker_selected_target,
            Some(ShareTarget::Window(_))
        );
        let audio_hint = if !app.config.share_audio {
            "Som desativado: sera transmitido apenas o video."
        } else if is_window_selected {
            "Janela: envia apenas o som do aplicativo selecionado (sem eco da chamada)."
        } else {
            "Tela inteira: envia o som do PC excluindo as vozes do Go Live - Lite."
        };
        ui.label(RichText::new(audio_hint).size(11.0).color(TEXT_SECONDARY));

        if app.picker_selected_target.is_none() && !at_limit {
            ui.label(
                RichText::new("Clique em uma previa acima (ou duplo clique para iniciar rapido).")
                    .size(11.0)
                    .color(TEXT_MUTED),
            );
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let can_start = !at_limit && app.picker_selected_target.is_some();
            let start_btn = ui.add_enabled(
                can_start,
                egui::Button::new(RichText::new("Compartilhar").semibold().color(Color32::WHITE))
                    .min_size(Vec2::new(150.0, 34.0))
                    .fill(if can_start { EMERALD } else { BG_ELEVATED }),
            );
            if start_btn.clicked() {
                start_now = true;
            }

            if ui
                .add_sized(
                    Vec2::new(170.0, 34.0),
                    egui::Button::new(
                        RichText::new(format!(
                            "Qualidade: {} {}fps",
                            app.config.share_resolution.label().split(' ').next().unwrap_or("1080p"),
                            app.current_effective_fps()
                        ))
                        .size(11.5)
                        .color(TEXT_PRIMARY),
                    )
                    .fill(BG_ELEVATED),
                )
                .on_hover_text("Abrir menu de Qualidade da Transmissao")
                .clicked()
            {
                app.show_stream_quality_modal = true;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_sized(
                        Vec2::new(92.0, 34.0),
                        egui::Button::new(RichText::new("Cancelar").color(Color32::WHITE)).fill(BLUE),
                    )
                    .clicked()
                {
                    close_now = true;
                }
            });
        });
    });

    if start_now {
        if let Some(target) = app.picker_selected_target.take() {
            app.start_screen_share(target);
            app.show_share_settings_modal = false;
            app.clear_share_thumbnails();
        }
    } else if !open || close_now {
        app.show_share_settings_modal = false;
        app.clear_share_thumbnails();
    }
}

fn render_preview_source_card(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    thumb: Option<(egui::TextureId, Vec2)>,
    is_selected: bool,
    active_already: bool,
    enabled: bool,
) -> egui::Response {
    let card_size = Vec2::new(242.0, 154.0);
    let (rect, resp) = ui.allocate_exact_size(
        card_size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let bg = if is_selected {
        Color32::from_rgb(16, 48, 38)
    } else if resp.hovered() && enabled {
        Color32::from_rgb(42, 49, 60)
    } else {
        BG_TILE
    };
    let border = if is_selected {
        Stroke::new(2.0_f32, EMERALD_BRIGHT)
    } else if resp.hovered() && enabled {
        Stroke::new(1.0_f32, BORDER_STRONG)
    } else {
        Stroke::new(1.0_f32, BORDER_SUBTLE)
    };
    ui.painter().rect(rect, Rounding::same(9.0), bg, border);

    let preview_rect = Rect::from_min_size(
        pos2(rect.left() + 6.0, rect.top() + 6.0),
        Vec2::new(card_size.x - 12.0, 108.0),
    );
    ui.painter()
        .rect_filled(preview_rect, Rounding::same(6.0), BG_MAIN);

    if let Some((tid, tsz)) = thumb {
        let scale = (preview_rect.width() / tsz.x.max(1.0))
            .min(preview_rect.height() / tsz.y.max(1.0));
        let fit = Vec2::new(tsz.x * scale, tsz.y * scale);
        let img_rect = Rect::from_center_size(preview_rect.center(), fit);
        ui.painter().image(
            tid,
            img_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        ui.painter().text(
            preview_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Carregando previa...",
            egui::FontId::proportional(11.0),
            TEXT_MUTED,
        );
    }

    if active_already {
        let pill = Rect::from_min_size(
            pos2(preview_rect.right() - 88.0, preview_rect.top() + 6.0),
            Vec2::new(82.0, 18.0),
        );
        ui.painter().rect_filled(pill, Rounding::same(9.0), EMERALD);
        ui.painter().text(
            pill.center(),
            egui::Align2::CENTER_CENTER,
            "Transmitindo",
            egui::FontId::proportional(10.0),
            Color32::WHITE,
        );
    }

    let text_area = Rect::from_min_max(
        pos2(rect.left() + 8.0, preview_rect.bottom() + 4.0),
        pos2(rect.right() - 8.0, rect.bottom() - 4.0),
    );
    let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(text_area));
    tui.spacing_mut().item_spacing.y = 1.0;
    tui.add(
        egui::Label::new(
            RichText::new(title)
                .size(11.5)
                .semibold()
                .color(if active_already { TEXT_MUTED } else { TEXT_PRIMARY }),
        )
        .truncate(),
    );
    tui.label(RichText::new(subtitle).size(10.0).color(TEXT_MUTED));

    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Modal separado e dedicado APENAS a Qualidade da Transmissao (Resolucao, FPS, Bitrate, Perfil e Qualidade inteligente).
pub fn render_stream_quality_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_stream_quality_modal {
        return;
    }

    let tier = app.current_plan_tier();
    let is_pro = tier.is_pro();
    let mut open = true;
    let mut close_now = false;

    centered_movable_window(
        "Qualidade da Transmissao",
        "golive_stream_quality_modal",
        ctx,
        Vec2::new(420.0, 315.0),
    )
    .open(&mut open)
    .default_width(420.0)
    .frame(
        egui::Frame::window(&ctx.style())
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(12.0)),
    )
    .show(ctx, |ui| {
        ui.set_width(396.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Plano atual: {}", tier.label()))
                    .size(12.5)
                    .semibold()
                    .color(if is_pro { EMERALD_BRIGHT } else { TEXT_SECONDARY }),
            );
        });
        if !is_pro {
            ui.label(
                RichText::new(
                    "Free: ate 1080p 60fps (8 Mbps) e 2 telas. Pro libera 2K/4K, 120/240fps e 16-32 Mbps.",
                )
                .size(11.0)
                .color(TEXT_MUTED),
            );
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(6.0);

        egui::Grid::new("stream_quality_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Resolucao:").size(12.5).color(TEXT_SECONDARY));
                egui::ComboBox::from_id_salt("qual_only_res")
                    .selected_text(app.config.share_resolution.label())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for res in ShareResolution::ALL {
                            let unlocked = app.can_use_resolution(res);
                            ui.add_enabled_ui(unlocked, |ui| {
                                if ui
                                    .selectable_value(&mut app.config.share_resolution, res, res.label())
                                    .changed()
                                {
                                    app.config.save();
                                }
                            });
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Taxa de quadros (FPS):").size(12.5).color(TEXT_SECONDARY));
                egui::ComboBox::from_id_salt("qual_only_fps")
                    .selected_text(app.config.share_fps.label())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for fps in ShareFps::ALL {
                            let unlocked = app.can_use_fps(fps);
                            ui.add_enabled_ui(unlocked, |ui| {
                                if ui
                                    .selectable_value(&mut app.config.share_fps, fps, fps.label())
                                    .changed()
                                {
                                    app.config.save();
                                }
                            });
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Bitrate maximo:").size(12.5).color(TEXT_SECONDARY));
                egui::ComboBox::from_id_salt("qual_only_bitrate")
                    .selected_text(app.config.share_bitrate.label())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for br in ShareBitrate::ALL {
                            let unlocked = app.can_use_bitrate(br);
                            ui.add_enabled_ui(unlocked, |ui| {
                                if ui
                                    .selectable_value(&mut app.config.share_bitrate, br, br.label())
                                    .changed()
                                {
                                    app.config.save();
                                }
                            });
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Perfil de video:").size(12.5).color(TEXT_SECONDARY));
                ui.horizontal(|ui| {
                    for mode in [
                        DegradationMode::Text,
                        DegradationMode::Balanced,
                        DegradationMode::Motion,
                    ] {
                        if ui
                            .selectable_label(app.config.share_profile == mode, mode.label())
                            .clicked()
                        {
                            app.config.share_profile = mode;
                            app.config.save();
                        }
                    }
                });
                ui.end_row();
            });

        ui.add_space(8.0);
        if ui
            .checkbox(
                &mut app.config.smart_quality,
                "Qualidade inteligente (adapta ao tamanho dos tiles)",
            )
            .changed()
        {
            app.config.save();
        }
        if ui
            .checkbox(
                &mut app.config.share_audio,
                "Compartilhar som do sistema por padrao",
            )
            .changed()
        {
            app.config.save();
        }

        if !app.active_screen_slots.is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Alteracoes de resolucao/FPS aplicam ao iniciar uma nova transmissao.")
                    .size(11.0)
                    .color(TEXT_MUTED),
            );
        }

        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_sized(
                    Vec2::new(96.0, 32.0),
                    egui::Button::new(RichText::new("Concluir").semibold().color(Color32::WHITE))
                        .fill(EMERALD),
                )
                .clicked()
            {
                close_now = true;
            }
        });
    });

    if !open || close_now {
        app.show_stream_quality_modal = false;
    }
}
