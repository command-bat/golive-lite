//! Faixa `{N} on` + `"Anuncie aqui tambem!"` e card de parceiro oficial (sidebar esquerda).

use crate::ui::theme::SemiBoldExt;
use egui::{Align, Color32, CursorIcon, Layout, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::GoLiveLiteApp;
use crate::net::protocol::{is_safe_http_url, parse_hex_rgb};
use crate::ui::theme::{
    BG_ELEVATED, BG_SURFACE, BORDER_SUBTLE, CTA_BLUE, EMERALD_BRIGHT, RED, TEXT_MUTED,
    TEXT_SECONDARY,
};

const ADVERTISE_URL: &str = "https://go.nemtudo.me/golive-partner-nemtudodiscord";

pub fn render_sidebar_partner_section(
    app: &mut GoLiveLiteApp,
    ui: &mut egui::Ui,
    max_height: f32,
) {
    let cap_h = max_height.max(150.0);
    ui.allocate_ui(Vec2::new(ui.available_width(), cap_h), |ui| {
        ui.set_max_height(cap_h);
        // 1. Duas capsulas escuras acima do card: esquerda `● {N} on` e direita `Anuncie aqui tambem!`
        ui.horizontal(|ui| {
            let total_w = ui.available_width();
            let left_w = (total_w * 0.34).clamp(68.0, 96.0);
            let right_w = (total_w - left_w - 6.0).max(90.0);

            egui::Frame::none()
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.set_min_width(left_w - 16.0);
                    ui.horizontal(|ui| {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(Vec2::new(7.0, 12.0), Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 2.8, EMERALD_BRIGHT);
                        let online_txt = match app.people_online {
                            Some(n) => format!("{} on", n),
                            None => "Online".to_string(),
                        };
                        ui.label(
                            RichText::new(online_txt)
                                .size(11.0)
                                .semibold()
                                .color(EMERALD_BRIGHT),
                        );
                    });
                });

            let adv_pill = egui::Button::new(
                RichText::new("Anuncie aqui também!")
                    .size(11.0)
                    .color(TEXT_SECONDARY),
            )
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(14.0))
            .min_size(Vec2::new(right_w, 24.0));

            if ui
                .add(adv_pill)
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Alternar visualizacao de anuncio / contato comercial")
                .clicked()
            {
                if app.partner_data.is_some() {
                    app.show_partner_house_override = !app.show_partner_house_override;
                } else {
                    crate::ui::external_link::request(ADVERTISE_URL);
                }
            }
        });

        ui.add_space(6.0);

        let partner = app.effective_partner();
        let card_bg_arr = parse_hex_rgb(partner.background_color.as_deref(), [255, 255, 255]);
        let card_fg_arr = parse_hex_rgb(partner.text_color.as_deref(), [17, 24, 39]);
        let btn_bg_arr = parse_hex_rgb(
            partner.button_background_color.as_deref(),
            if partner.is_house_ad() {
                [79, 117, 255]
            } else {
                [5, 150, 105]
            },
        );
        let btn_fg_arr = parse_hex_rgb(partner.button_text_color.as_deref(), [255, 255, 255]);

        let card_bg = Color32::from_rgb(card_bg_arr[0], card_bg_arr[1], card_bg_arr[2]);
        let card_fg = Color32::from_rgb(card_fg_arr[0], card_fg_arr[1], card_fg_arr[2]);
        let btn_bg = Color32::from_rgb(btn_bg_arr[0], btn_bg_arr[1], btn_bg_arr[2]);
        let btn_fg = Color32::from_rgb(btn_fg_arr[0], btn_fg_arr[1], btn_fg_arr[2]);
        let is_light_card = (card_bg.r() as u16 + card_bg.g() as u16 + card_bg.b() as u16) > 400;

        let mut cta_clicked = false;
        let mut watch_again_clicked = false;
        let card_scroll_max_h = (cap_h - 32.0).max(115.0);

        egui::ScrollArea::vertical()
            .id_salt("partner_card_scroll")
            .max_height(card_scroll_max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Frame::none()
                    .fill(card_bg)
                    .stroke(Stroke::new(
                        1.0_f32,
                        if is_light_card {
                            Color32::from_rgb(228, 228, 231)
                        } else {
                            BORDER_SUBTLE
                        },
                    ))
                    .rounding(Rounding::same(14.0))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        // 1. Banner no topo (max-h-32 = 128px, fiel a PartnerCard.tsx)
                        if let Some(ref tex) = app.partner_banner_tex {
                            if !app.show_partner_house_override {
                                let avail_w = ui.available_width().max(110.0);
                                let sz = tex.size_vec2();
                                let aspect = if sz.x > 0.0 && sz.y > 0.0 {
                                    (sz.y / sz.x).clamp(0.32, 0.72)
                                } else {
                                    0.46
                                };
                                let target_size = Vec2::new(avail_w, (avail_w * aspect).min(128.0));
                                let (img_rect, img_resp) =
                                    ui.allocate_exact_size(target_size, Sense::click());
                                ui.painter().image(
                                    tex.id(),
                                    img_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );
                                if img_resp.clicked() {
                                    cta_clicked = true;
                                }
                                ui.add_space(8.0);
                            } else {
                                render_house_ad_banner(ui, &mut cta_clicked);
                            }
                        } else if partner.is_house_ad() {
                            render_house_ad_banner(ui, &mut cta_clicked);
                        }

                        // 2. Selo "PATROCINADO" + icone opcional
                        ui.horizontal(|ui| {
                            let muted_text = if is_light_card {
                                Color32::from_rgb(110, 113, 124)
                            } else {
                                card_fg.linear_multiply(0.70)
                            };
                            ui.label(
                                RichText::new("PATROCINADO")
                                    .size(9.5)
                                    .semibold()
                                    .color(muted_text),
                            );

                            if let Some(ref icon_tex) = app.partner_icon_tex {
                                if !app.show_partner_house_override {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add(
                                            egui::Image::new(icon_tex)
                                                .fit_to_exact_size(Vec2::splat(16.0))
                                                .rounding(Rounding::same(4.0)),
                                        );
                                    });
                                }
                            }
                        });

                        ui.add_space(4.0);

                        if !partner.title.trim().is_empty() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(partner.title.trim())
                                        .size(13.5)
                                        .semibold()
                                        .color(card_fg),
                                )
                                .wrap(),
                            );
                        }

                        if !partner.description.trim().is_empty() {
                            ui.add_space(3.0);
                            let desc_col = if is_light_card {
                                Color32::from_rgb(82, 82, 91)
                            } else {
                                card_fg.linear_multiply(0.82)
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(partner.description.trim())
                                        .size(11.5)
                                        .color(desc_col),
                                )
                                .wrap(),
                            );
                        }

                        // 3. Botoes principais (Entrar em contato / CTA + Voltar ou Assistir de novo)
                        if let Some(ref btn_label) = partner.button_label {
                            let lbl = btn_label.trim();
                            let url_ok = partner
                                .button_url
                                .as_deref()
                                .is_some_and(is_safe_http_url);
                            if !lbl.is_empty() && url_ok {
                                ui.add_space(9.0);
                                let cta_btn = egui::Button::new(
                                    RichText::new(lbl).size(12.5).semibold().color(btn_fg),
                                )
                                .fill(btn_bg)
                                .rounding(Rounding::same(8.0))
                                .min_size(Vec2::new(ui.available_width(), 32.0));
                                if ui
                                    .add(cta_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    cta_clicked = true;
                                }
                            }
                        }

                        if app.show_partner_house_override {
                            ui.add_space(4.0);
                            let back_btn = egui::Button::new(
                                RichText::new("Voltar ao parceiro")
                                    .size(11.5)
                                    .semibold()
                                    .color(card_fg),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(
                                1.0_f32,
                                if is_light_card {
                                    Color32::from_rgb(212, 212, 216)
                                } else {
                                    BORDER_SUBTLE
                                },
                            ))
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(ui.available_width(), 28.0));
                            if ui
                                .add(back_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                app.show_partner_house_override = false;
                            }
                        } else if partner
                            .reward_video_url
                            .as_deref()
                            .is_some_and(is_safe_http_url)
                        {
                            ui.add_space(4.0);
                            let watch_btn = egui::Button::new(
                                RichText::new("Assistir de novo")
                                    .size(11.5)
                                    .semibold()
                                    .color(card_fg),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(
                                1.0_f32,
                                if is_light_card {
                                    Color32::from_rgb(212, 212, 216)
                                } else {
                                    BORDER_SUBTLE
                                },
                            ))
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(ui.available_width(), 28.0));
                            if ui
                                .add(watch_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                watch_again_clicked = true;
                            }
                        }
                    });
            });

        if cta_clicked {
            app.click_partner_cta(&partner);
        } else if watch_again_clicked {
            if let Some(ref url) = partner.reward_video_url {
                crate::ui::external_link::request(url);
            }
        }
    });

    let _ = (BG_ELEVATED, TEXT_MUTED, CTA_BLUE);
}

fn render_house_ad_banner(ui: &mut egui::Ui, cta_clicked: &mut bool) {
    let w = ui.available_width().max(110.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 96.0), Sense::click());
    ui.painter()
        .rect_filled(rect, Rounding::same(8.0), Color32::from_rgb(8, 8, 10));
    ui.painter().text(
        egui::pos2(rect.center().x, rect.center().y - 12.0),
        egui::Align2::CENTER_CENTER,
        "Go Live",
        egui::FontId::proportional(14.0),
        Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.center().y + 12.0),
        egui::Align2::CENTER_CENTER,
        "ANUNCIE AQUI!!",
        egui::FontId::proportional(17.5),
        RED,
    );
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        *cta_clicked = true;
    }
    ui.add_space(8.0);
}
