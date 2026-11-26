use egui::{pos2, Color32, CursorIcon, RichText, Rounding, Sense, Stroke, TextureId, Vec2};

use crate::net::protocol::{resolve_name_color_rgb, verified_badge_tone};
use crate::ui::icons;
use crate::ui::theme::{
    BG_ELEVATED, BG_MAIN, BLUE_LIGHT, BORDER_SUBTLE, EMERALD_BRIGHT, GOLD, INDIGO, RED,
    TEXT_MUTED,
};

pub struct ParticipantRowInput<'a> {
    pub display_name: &'a str,
    pub is_self: bool,
    pub speaking: bool,
    pub mic_on: bool,
    pub silenced: bool,
    pub mics_muted: bool,
    pub sharing_screen: bool,
    pub sharing_camera: bool,
    pub muted: bool,
    pub flags: &'a [String],
    pub name_color: Option<&'a str>,
    pub avatar_tex: Option<TextureId>,
    pub is_owner: bool,
    pub is_admin: bool,
    pub is_app: bool,
    pub is_mobile_app: bool,
    /// No Go Live - Lite: o mesmo icone de computador, com o raio.
    pub is_lite_app: bool,
    pub hand_raised: bool,
    pub menu_open: bool,
    pub has_menu_actions: bool,
}

pub struct ParticipantRowOutput {
    pub open_menu_pos: Option<egui::Pos2>,
}

/// Linha de participante fiel ao GoLive web (avatar + pontinho online + selo Pro/Verificado + coroa + icone app + icones vetoriais).
pub fn render_participant_row(
    ui: &mut egui::Ui,
    input: ParticipantRowInput<'_>,
) -> ParticipantRowOutput {
    let mut out = ParticipantRowOutput {
        open_menu_pos: None,
    };

    // Site: sem borda; a propria linha e zinc-900, menu aberto zinc-800 (hover do site).
    let border_col = Color32::TRANSPARENT;
    let bg_col = if input.menu_open {
        BORDER_SUBTLE
    } else if input.is_self {
        BG_ELEVATED
    } else {
        Color32::TRANSPARENT
    };

    let frame_resp = egui::Frame::none()
        .fill(bg_col)
        .stroke(Stroke::new(1.0_f32, border_col))
        .rounding(Rounding::same(14.0)) // rounded-lg da sala nova
        .inner_margin(egui::Margin::symmetric(12.0, 8.0)) // px-3 py-2
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;

                // 1. Avatar circular 22x22 + ponto verde online
                let av_sense = if input.has_menu_actions {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (av_rect, av_resp) = ui.allocate_exact_size(Vec2::splat(22.0), av_sense);
                if input.has_menu_actions && av_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    out.open_menu_pos = Some(pos2(av_rect.right() + 12.0, av_rect.top()));
                }
                if let Some(tex_id) = input.avatar_tex {
                    icons::paint_circular_avatar(
                        ui.painter(),
                        av_rect,
                        tex_id,
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                    );
                } else {
                    let initial = input
                        .display_name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string();
                    ui.painter()
                        .circle_filled(av_rect.center(), 11.0, avatar_color_for(input.display_name));
                    ui.painter().text(
                        av_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        initial,
                        egui::FontId::proportional(10.5),
                        Color32::WHITE,
                    );
                }
                if input.speaking {
                    ui.painter().circle_stroke(
                        av_rect.center(),
                        12.2,
                        Stroke::new(1.8_f32, EMERALD_BRIGHT),
                    );
                }
                let dot_c = pos2(av_rect.right() - 2.0, av_rect.bottom() - 2.0);
                ui.painter().circle_filled(dot_c, 3.6, BG_MAIN);
                ui.painter().circle_filled(dot_c, 2.5, EMERALD_BRIGHT);

                // 2. Nome (cor equipada da loja ou verde falando) + Selo + Coroa + Icone App + (voce)
                let name_color = if input.speaking {
                    Color32::from_rgb(52, 211, 153) // text-emerald-400 falando
                } else if input.muted {
                    TEXT_MUTED
                } else if let Some([r, g, b]) = resolve_name_color_rgb(input.name_color) {
                    Color32::from_rgb(r, g, b)
                } else {
                    if input.is_self { Color32::from_rgb(243, 244, 246) } else { Color32::from_rgb(210, 212, 218) }
                };

                let name_resp = ui.add(
                    egui::Label::new(
                        RichText::new(input.display_name)
                            .size(14.0)
                            .color(name_color),
                    )
                    .truncate()
                    .sense(if input.has_menu_actions {
                        Sense::click()
                    } else {
                        Sense::hover()
                    }),
                );
                if input.has_menu_actions {
                    let nresp = name_resp.on_hover_cursor(CursorIcon::PointingHand);
                    if nresp.clicked() || nresp.secondary_clicked() {
                        out.open_menu_pos = Some(pos2(nresp.rect.left(), nresp.rect.bottom() + 4.0));
                    }
                }

                if let Some(tone) = verified_badge_tone(input.flags) {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(13.0, 14.0), Sense::hover());
                    icons::paint_verified_seal(ui.painter(), r.center(), 5.8, tone);
                }

                if input.is_owner {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
                    icons::paint_crown(ui.painter(), r.center(), 11.0, GOLD);
                    resp.on_hover_text("Dono da sala");
                } else if input.is_admin {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(11.0, 11.0), Sense::hover());
                    icons::paint_crown(ui.painter(), r.center(), 10.0, TEXT_MUTED);
                    resp.on_hover_text("Administrador da sala");
                }

                if input.is_app {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(13.0, 13.0), Sense::hover());
                    if input.is_self || input.is_lite_app {
                        icons::paint_desktop_lite_app(ui.painter(), r.center(), 12.0, INDIGO);
                        resp.on_hover_text("Usando o Go Live - Lite");
                    } else {
                        icons::paint_desktop_app(ui.painter(), r.center(), 12.0, INDIGO);
                        resp.on_hover_text("Usando o Go Live Desktop App");
                    }
                } else if input.is_mobile_app {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(11.0, 13.0), Sense::hover());
                    icons::paint_phone_app(ui.painter(), r.center(), 11.5, INDIGO);
                    resp.on_hover_text("Usando o Go Live App Mobile");
                }

                if input.is_self {
                    ui.label(RichText::new("(você)").size(12.0).color(TEXT_MUTED));
                }

                if input.hand_raised {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(13.0, 13.0), Sense::hover());
                    icons::paint_hand_raise(
                        ui.painter(),
                        r.center(),
                        12.0,
                        Color32::from_rgb(251, 191, 36),
                    );
                    resp.on_hover_text(format!("{} levantou a mão", input.display_name));
                }

                // 3. Direita: icones vetoriais de status (tela, cam, surdo, mic, mudo local)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;

                    if !input.is_self && input.muted {
                        let (r, resp) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_speaker(ui.painter(), r.center(), 12.5, RED, true);
                        resp.on_hover_text("Silenciado para voce (clique no nome para ajustar)");
                    }

                    if input.mics_muted {
                        let (r, resp) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_headphones(ui.painter(), r.center(), 12.5, TEXT_MUTED, true);
                        resp.on_hover_text("Audio da chamada mutado");
                    }

                    if input.silenced {
                        let (r, resp) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_mic(ui.painter(), r.center(), 12.5, RED, true);
                        resp.on_hover_text("Silenciado por um administrador");
                    } else if input.mic_on {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_mic(ui.painter(), r.center(), 12.5, BLUE_LIGHT, false);
                    } else {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_mic(ui.painter(), r.center(), 12.5, TEXT_MUTED, true);
                    }

                    if input.sharing_camera {
                        let (r, resp) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_camera(
                            ui.painter(),
                            r.center(),
                            12.5,
                            Color32::from_rgb(167, 139, 250),
                        );
                        resp.on_hover_text("Transmitindo camera");
                    }

                    if input.sharing_screen {
                        let (r, resp) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::paint_screen(ui.painter(), r.center(), 12.5, EMERALD_BRIGHT);
                        resp.on_hover_text("Transmitindo tela");
                    }
                });
            });
        });

    if out.open_menu_pos.is_some() {
        out.open_menu_pos = Some(pos2(frame_resp.response.rect.right() + 6.0, frame_resp.response.rect.top()));
    } else if input.has_menu_actions && frame_resp.response.secondary_clicked() {
        out.open_menu_pos = Some(pos2(frame_resp.response.rect.right() + 6.0, frame_resp.response.rect.top()));
    }

    out
}

pub fn avatar_color_for(name: &str) -> Color32 {
    let mut hash: u32 = 2166136261;
    for b in name.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let palette = [
        Color32::from_rgb(37, 99, 235),
        Color32::from_rgb(5, 150, 105),
        Color32::from_rgb(147, 51, 234),
        Color32::from_rgb(217, 119, 6),
        Color32::from_rgb(219, 39, 119),
        Color32::from_rgb(13, 148, 136),
    ];
    palette[(hash as usize) % palette.len()]
}
