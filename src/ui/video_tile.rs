use crate::ui::theme::SemiBoldExt;
use egui::{pos2, Align, Color32, Layout, Rect, RichText, Rounding, Stroke, TextureHandle, Vec2};

use crate::ui::icons;
use crate::ui::theme::{
    BG_ELEVATED, BG_TILE, BLUE, BORDER_SUBTLE, EMERALD, RED, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};

pub struct TileInteraction {
    pub toggle_focus: bool,
    pub toggle_fullscreen: bool,
    pub toggle_pip: bool,
    pub toggle_stop_watching: bool,
    pub toggle_eco_preview: bool,
    pub stop_self_share: bool,
    pub toggle_mute: bool,
    pub toggle_stats: bool,
    pub toggle_reaction_picker: bool,
    pub close_reaction_picker: bool,
    pub send_reaction_emoji: Option<String>,
    pub new_volume: Option<f32>,
    pub video_rect: egui::Rect,
}

pub struct VideoTileViewModel<'a> {
    pub tile_id: &'a str,
    pub title: &'a str,
    pub badge: &'a str,
    pub texture: Option<&'a TextureHandle>,
    pub is_self: bool,
    pub is_focused: bool,
    pub is_fullscreen: bool,
    pub is_pip: bool,
    pub is_stopped_by_us: bool,
    pub preview_paused_eco: bool,
    pub is_resuming: bool,
    pub is_ad_paused: bool,
    pub placeholder_note: Option<&'a str>,
    pub muted: bool,
    pub volume: f32,
    pub allow_audio_controls: bool,
    pub show_stats_button: bool,
    pub stats_open: bool,
    pub show_reaction_btn: bool,
    pub reaction_picker_open: bool,
    pub floating_reactions: &'a [(&'a str, &'a str, f32, f32)],
    pub resolution_label: Option<String>,
}

pub fn render_video_tile(
    ui: &mut egui::Ui,
    vm: VideoTileViewModel<'_>,
    available_size: Vec2,
) -> TileInteraction {
    let mut action = TileInteraction {
        toggle_focus: false,
        toggle_fullscreen: false,
        toggle_pip: false,
        toggle_stop_watching: false,
        toggle_eco_preview: false,
        stop_self_share: false,
        toggle_mute: false,
        toggle_stats: false,
        toggle_reaction_picker: false,
        close_reaction_picker: false,
        send_reaction_emoji: None,
        new_volume: None,
        video_rect: egui::Rect::NOTHING,
    };

    egui::Frame::none()
        .fill(BG_TILE)
        .stroke(Stroke::new(
            if vm.is_focused { 1.5_f32 } else { 1.0_f32 },
            if vm.is_focused { BLUE } else { BORDER_SUBTLE },
        ))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(6.0))
        .show(ui, |ui| {
            ui.set_min_size(available_size);
            ui.set_max_size(available_size);

            // Empilhamento vertical explicito mesmo dentro de linhas horizontais da grade/faixa.
            ui.vertical(|ui| {
                ui.set_min_size(available_size);
                ui.set_max_size(available_size);

                // Cabecalho limpo do Tile (badge + titulo + resolucao; controles ficam sobre a midia)
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(EMERALD.linear_multiply(0.25))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(vm.badge)
                                    .size(10.5)
                                    .semibold()
                                    .color(Color32::from_rgb(110, 231, 183)),
                            );
                        });

                    ui.add(
                        egui::Label::new(
                            RichText::new(vm.title).size(12.5).semibold().color(TEXT_PRIMARY),
                        )
                        .truncate(),
                    );

                    if let Some(ref res_txt) = vm.resolution_label {
                        ui.label(RichText::new(res_txt).size(10.5).color(TEXT_MUTED));
                    }
                });

                ui.add_space(4.0);

                // Area de Video / Placeholder 16:9
                let footer_h = if vm.allow_audio_controls && !vm.is_stopped_by_us {
                    26.0
                } else {
                    4.0
                };
                let img_area = Vec2::new(
                    (available_size.x - 14.0).max(64.0),
                    (available_size.y - 34.0 - footer_h).max(48.0),
                );

                let (rect, resp) = ui.allocate_exact_size(img_area, egui::Sense::click());
                action.video_rect = rect;
                ui.painter()
                    .rect_filled(rect, Rounding::same(8.0), Color32::from_rgb(7, 7, 9));

                if vm.is_stopped_by_us {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space((rect.height() - 56.0).max(0.0) / 2.0);
                            ui.label(
                                RichText::new("Transmissao pausada para voce")
                                    .size(13.0)
                                    .color(TEXT_SECONDARY),
                            );
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Assistir").color(Color32::WHITE),
                                    )
                                    .fill(EMERALD),
                                )
                                .clicked()
                            {
                                action.toggle_stop_watching = true;
                            }
                        });
                    });
                } else if vm.preview_paused_eco {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.vertical_centered(|ui| {
                        ui.add_space(((rect.height() - 88.0).max(0.0) / 2.0).max(38.0));
                        ui.label(
                            RichText::new(if vm.is_self {
                                "Transmitindo ao vivo (previa pausada)"
                            } else {
                                "Visualizacao pausada na grade (Modo Eco)"
                            })
                            .size(13.0)
                            .semibold()
                            .color(TEXT_PRIMARY),
                        );
                        ui.add_space(3.0);
                        let sub = vm
                            .resolution_label
                            .as_deref()
                            .unwrap_or("Ative para ver aqui ou clique em Focar");
                        ui.label(RichText::new(sub).size(11.5).color(TEXT_MUTED));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let row_w = 228.0_f32.min(rect.width() - 12.0);
                            ui.add_space((ui.available_width() - row_w).max(0.0) / 2.0);
                            let act_lbl = if vm.is_self {
                                "Ver previa"
                            } else {
                                "Ativar visualizacao"
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(act_lbl)
                                            .size(11.5)
                                            .semibold()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(EMERALD)
                                    .rounding(Rounding::same(7.0)),
                                )
                                .clicked()
                            {
                                action.toggle_eco_preview = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Focar").size(11.5).color(TEXT_PRIMARY),
                                    )
                                    .fill(BG_ELEVATED)
                                    .rounding(Rounding::same(7.0)),
                                )
                                .clicked()
                            {
                                action.toggle_focus = true;
                            }
                        });
                    });
                } else if vm.is_resuming {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Retomando transmissao...",
                        egui::FontId::proportional(13.0),
                        TEXT_SECONDARY,
                    );
                } else if vm.is_ad_paused {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Transmissao pausada temporariamente pelo transmissor",
                        egui::FontId::proportional(12.5),
                        TEXT_SECONDARY,
                    );
                } else if let Some(tex) = vm.texture {
                    let tex_size = tex.size_vec2();
                    let fit = fit_aspect(tex_size, rect.size());
                    let img_rect = egui::Rect::from_center_size(rect.center(), fit);
                    ui.painter().image(
                        tex.id(),
                        img_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else if let Some(note) = vm.placeholder_note {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        note,
                        egui::FontId::proportional(12.5),
                        TEXT_SECONDARY,
                    );
                } else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Conectando midia...",
                        egui::FontId::proportional(12.5),
                        TEXT_MUTED,
                    );
                }

                // Todos os botoes circulares de controle diretamente em cima da midia (canto superior direito)
                let compact = rect.width() < 260.0;
                let btn_d = if compact { 24.0 } else { 30.0 };
                let icon_sz = if compact { 12.5 } else { 15.0 };
                let gap = if compact { 4.0 } else { 6.0 };
                let dark_circle = Color32::from_rgba_unmultiplied(12, 14, 19, 195);
                let mut overlay_interacted = false;

                let overlay_rect = Rect::from_min_max(
                    pos2(rect.left() + 8.0, rect.top() + 8.0),
                    pos2(rect.right() - 8.0, rect.top() + 8.0 + btn_d),
                );
                let mut ov_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(overlay_rect)
                        .layout(Layout::right_to_left(Align::Center)),
                );
                ov_ui.spacing_mut().item_spacing.x = gap;

                // Ordem visual (da esquerda para direita): [Stats] [Eco] [PiP] [Foco] [Tela cheia] [Parar/Sair]
                if vm.is_self {
                    let stop_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        Color32::from_rgba_unmultiplied(220, 38, 38, 220),
                        "Parar de compartilhar esta tela/janela",
                        |p, c, col| icons::paint_stop_square(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= stop_r.hovered() || stop_r.clicked();
                    if stop_r.clicked() {
                        action.stop_self_share = true;
                    }
                } else if !vm.is_stopped_by_us {
                    let leave_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        dark_circle,
                        "Parar de assistir esta transmissao (poupa rede e CPU)",
                        |p, c, col| icons::paint_close_x(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= leave_r.hovered() || leave_r.clicked();
                    if leave_r.clicked() {
                        action.toggle_stop_watching = true;
                    }
                }

                if !vm.is_stopped_by_us {
                    let fs_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        if vm.is_fullscreen { BLUE } else { dark_circle },
                        if vm.is_fullscreen {
                            "Sair da tela cheia (Esc)"
                        } else {
                            "Tela cheia"
                        },
                        |p, c, col| icons::paint_fullscreen(p, c, icon_sz, col, vm.is_fullscreen),
                        Color32::WHITE,
                    );
                    overlay_interacted |= fs_r.hovered() || fs_r.clicked();
                    if fs_r.clicked() {
                        action.toggle_fullscreen = true;
                    }
                }

                if !vm.is_fullscreen {
                    let foc_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        if vm.is_focused { BLUE } else { dark_circle },
                        if vm.is_focused { "Sair do foco" } else { "Focar midia" },
                        |p, c, col| icons::paint_focus(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= foc_r.hovered() || foc_r.clicked();
                    if foc_r.clicked() {
                        action.toggle_focus = true;
                    }
                }

                if !vm.is_stopped_by_us {
                    let pip_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        if vm.is_pip { EMERALD } else { dark_circle },
                        if vm.is_pip {
                            "Fechar Picture-in-Picture (PiP)"
                        } else {
                            "Abrir em Picture-in-Picture (PiP)"
                        },
                        |p, c, col| icons::paint_pip(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= pip_r.hovered() || pip_r.clicked();
                    if pip_r.clicked() {
                        action.toggle_pip = true;
                    }
                }

                if !vm.is_focused && !vm.is_fullscreen && !vm.is_stopped_by_us {
                    let eco_active = !vm.preview_paused_eco;
                    let eco_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        dark_circle,
                        if vm.preview_paused_eco {
                            "Ativar visualizacao na grade"
                        } else {
                            "Ocultar previa na grade (Modo Eco)"
                        },
                        |p, c, col| icons::paint_eye(p, c, icon_sz, col, eco_active),
                        Color32::WHITE,
                    );
                    overlay_interacted |= eco_r.hovered() || eco_r.clicked();
                    if eco_r.clicked() {
                        action.toggle_eco_preview = true;
                    }
                }

                if vm.show_stats_button {
                    let stats_tip = if vm.is_self {
                        "Sua transmissao (estatisticas)"
                    } else {
                        "Estatisticas da conexao"
                    };
                    let stats_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        if vm.stats_open { EMERALD } else { dark_circle },
                        stats_tip,
                        |p, c, col| icons::paint_stats_bars(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= stats_r.hovered() || stats_r.clicked();
                    if stats_r.clicked() {
                        action.toggle_stats = true;
                    }
                }

                let mut rx_btn_rect = Rect::NOTHING;
                if vm.show_reaction_btn && !vm.is_stopped_by_us {
                    let rx_r = icons::media_overlay_circle_button(
                        &mut ov_ui,
                        btn_d,
                        if vm.reaction_picker_open {
                            EMERALD
                        } else {
                            dark_circle
                        },
                        "Reagir a esta midia",
                        |p, c, col| icons::paint_reaction_smile(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    rx_btn_rect = rx_r.rect;
                    overlay_interacted |= rx_r.hovered() || rx_r.clicked();
                    if rx_r.clicked() {
                        action.toggle_reaction_picker = true;
                    }
                }

                #[cfg(feature = "mod_ferramentas")]
                if vm.show_reaction_btn && vm.reaction_picker_open && !vm.is_stopped_by_us {
                    let picker_w = (rect.width() - 20.0).clamp(180.0, 344.0);
                    let picker_rect = Rect::from_min_size(
                        pos2((rect.right() - picker_w - 8.0).max(rect.left() + 8.0), rect.top() + btn_d + 14.0),
                        Vec2::new(picker_w, 36.0),
                    );
                    let mut p_ui = ui.new_child(egui::UiBuilder::new().max_rect(picker_rect));
                    let pop_resp = egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(15, 18, 25, 242))
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(16.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                        .show(&mut p_ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                                for &em in crate::net::room_tools::REACTION_EMOJIS {
                                    let (er, b) = ui.allocate_exact_size(
                                        Vec2::splat(24.0),
                                        egui::Sense::click(),
                                    );
                                    if b.hovered() {
                                        ui.painter().rect_filled(
                                            er,
                                            Rounding::same(6.0),
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                                        );
                                    }
                                    crate::ui::native_emoji::paint_native_emoji(
                                        ui,
                                        er.shrink(2.0),
                                        em,
                                        Color32::WHITE,
                                        15.0,
                                    );
                                    overlay_interacted |= b.hovered() || b.clicked();
                                    if b.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        action.send_reaction_emoji = Some(em.to_string());
                                    }
                                }
                            });
                        });
                    let actual_picker_rect = pop_resp.response.rect;
                    let clicked_outside = ui.ctx().input(|i| {
                        i.pointer.any_pressed()
                            && i.pointer.interact_pos().is_some_and(|p| {
                                !actual_picker_rect.contains(p) && !rx_btn_rect.contains(p)
                            })
                    });
                    if clicked_outside || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                        action.close_reaction_picker = true;
                    }
                }
                let _ = rx_btn_rect;

                if !vm.floating_reactions.is_empty() {
                    paint_floating_reactions(ui.ctx(), ui.painter(), rect, vm.floating_reactions);
                }

                if resp.double_clicked()
                    && !overlay_interacted
                    && !vm.is_stopped_by_us
                    && !vm.preview_paused_eco
                {
                    action.toggle_focus = true;
                }

                // Barra inferior de volume da transmissao (quando aplicavel)
                if vm.allow_audio_controls && !vm.is_stopped_by_us {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let mute_btn = ui.add(
                            egui::Button::new(
                                RichText::new(if vm.muted { "Mudo" } else { "Audio" })
                                    .size(11.0)
                                    .color(Color32::WHITE),
                            )
                            .fill(if vm.muted { RED } else { BG_ELEVATED })
                            .min_size(Vec2::new(46.0, 20.0)),
                        );
                        if mute_btn.clicked() {
                            action.toggle_mute = true;
                        }
                        let mut pct = vm.volume * 100.0;
                        let slider = ui.add(
                            egui::Slider::new(&mut pct, 0.0..=200.0)
                                .show_value(false)
                                .step_by(1.0),
                        );
                        if slider.double_clicked() {
                            action.new_volume = Some(1.0);
                        } else if slider.changed() {
                            action.new_volume = Some(pct / 100.0);
                        }
                        ui.label(
                            RichText::new(format!("{}%", pct.round() as i32))
                                .size(11.0)
                                .color(TEXT_SECONDARY),
                        );
                    });
                }
            });
        });

    let _ = vm.tile_id;
    action
}

pub(crate) fn fit_aspect(src: Vec2, max_box: Vec2) -> Vec2 {
    if src.x <= 0.0 || src.y <= 0.0 {
        return max_box;
    }
    let scale = (max_box.x / src.x).min(max_box.y / src.y);
    Vec2::new(src.x * scale, src.y * scale)
}

#[allow(dead_code)]
pub fn paint_floating_reactions(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    reactions: &[(&str, &str, f32, f32)],
) {
    let travel = (rect.height() * 0.72).clamp(90.0, 260.0);
    for &(emoji, name, age_secs, x_frac) in reactions {
        if age_secs > 3.45 {
            continue;
        }
        let t = (age_secs / 3.4).clamp(0.0, 1.0);
        let alpha_f = if t < 0.12 {
            t / 0.12
        } else if t > 0.72 {
            1.0 - (t - 0.72) / 0.28
        } else {
            1.0
        };
        let alpha = (alpha_f.clamp(0.0, 1.0) * 255.0).round() as u8;
        let tint = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
        let sway = (t * std::f32::consts::TAU * 1.4).sin() * 10.0;
        let x = rect.left() + rect.width() * x_frac.clamp(0.10, 0.88) + sway;
        let y = rect.bottom() - 28.0 - travel * t;
        let pos = pos2(x, y);
        let tex_id = crate::ui::native_emoji::get_or_load_emoji_texture(ctx, emoji);

        if name.trim().is_empty() {
            if let Some(tid) = tex_id {
                painter.image(
                    tid,
                    Rect::from_center_size(pos, Vec2::splat(26.0)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    tint,
                );
            } else {
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    emoji,
                    egui::FontId::proportional(24.0),
                    tint,
                );
            }
        } else {
            let name_w = painter
                .layout_no_wrap(
                    name.to_string(),
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                )
                .size()
                .x
                .min(115.0);
            let approx_w = (name_w + 34.0).max(56.0);
            let pill = Rect::from_center_size(pos, Vec2::new(approx_w, 26.0));
            painter.rect_filled(
                pill,
                Rounding::same(13.0),
                Color32::from_rgba_unmultiplied(15, 18, 25, ((alpha as f32) * 0.78) as u8),
            );
            let em_rect = Rect::from_center_size(
                pos2(pill.left() + 14.0, pill.center().y),
                Vec2::splat(17.0),
            );
            if let Some(tid) = tex_id {
                painter.image(
                    tid,
                    em_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    tint,
                );
            } else {
                painter.text(
                    em_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    emoji,
                    egui::FontId::proportional(14.0),
                    tint,
                );
            }
            painter.text(
                pos2(pill.left() + 26.0, pill.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(12.0),
                Color32::from_rgba_unmultiplied(245, 248, 255, alpha),
            );
        }
    }
}
