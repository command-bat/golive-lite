//! Modal flutuante de Perfil (proprio ou de outros participantes, modificacao `perfil`).

use crate::ui::theme::SemiBoldExt;
use egui::{pos2, Align, Color32, CursorIcon, Layout, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::GoLiveLiteApp;
use crate::config::MOD_PERFIL;
use crate::net::protocol::{
    format_duration_pt, format_points_br, resolve_name_color_rgb,
    verified_badge_tone, PlanTier,
};
use crate::ui::icons;
use crate::ui::participant_row::avatar_color_for;
use crate::ui::theme::{
    centered_movable_window, BG_ELEVATED, BG_SURFACE, BG_TILE, BLUE_LIGHT, BORDER_SUBTLE, EMERALD,
    EMERALD_BRIGHT, GOLD, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileTarget {
    Account(String),
    Guest {
        name: String,
        avatar_url: Option<String>,
    },
}

pub fn render_profile_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.has_modification(MOD_PERFIL) {
        return;
    }
    let Some(target) = app
        .profile_mod
        .as_ref()
        .and_then(|p| p.open_profile_target.clone())
    else {
        return;
    };

    let mut open = true;
    let mut close_clicked = false;
    centered_movable_window(
        "Perfil do Usuario",
        "golive_user_profile_modal",
        ctx,
        Vec2::new(450.0, 460.0),
    )
    .open(&mut open)
    .title_bar(false)
    .default_width(450.0)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(16.0))
                .inner_margin(egui::Margin::same(16.0)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.set_max_width(450.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Perfil").size(14.5).semibold().color(TEXT_PRIMARY));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let xr = icons::icon_button(
                        ui,
                        Vec2::splat(26.0),
                        BG_ELEVATED,
                        Stroke::NONE,
                        Rounding::same(13.0),
                        "Fechar",
                        |p, c, col| icons::paint_close_x(p, c, 12.0, col),
                        TEXT_SECONDARY,
                    );
                    if xr.clicked() {
                        close_clicked = true;
                    }

                    if let ProfileTarget::Account(ref uid) = target {
                        let site_btn = egui::Button::new(
                            RichText::new("Abrir no site").size(11.5).color(TEXT_PRIMARY),
                        )
                        .fill(BG_ELEVATED)
                        .rounding(Rounding::same(8.0));
                        if ui
                            .add(site_btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            crate::ui::external_link::request(&format!(
                                "https://golive.nemtudo.me/user/{}",
                                uid
                            ));
                        }
                    }
                });
            });
            ui.add_space(10.0);

            match target {
                ProfileTarget::Guest { name, avatar_url } => {
                    let av_tex = app.resolve_and_load_avatar(avatar_url.as_deref());
                    render_guest_card(ui, &name, av_tex);
                }
                ProfileTarget::Account(uid) => {
                    let cached_prof = app
                        .profile_mod
                        .as_ref()
                        .and_then(|p| p.profile_cache.get(&uid).cloned());
                    let is_loading = app
                        .profile_mod
                        .as_ref()
                        .is_some_and(|p| p.profile_loading_id.as_deref() == Some(uid.as_str()));
                    let prof_err = app
                        .profile_mod
                        .as_ref()
                        .and_then(|p| p.profile_error.clone());
                    if let Some(prof) = cached_prof {
                        let av_tex = app.resolve_and_load_avatar(prof.account.avatar_url.as_deref());
                        let banner_tex =
                            app.resolve_and_load_profile_banner(prof.account.banner_url.as_deref());
                        render_account_card(ui, &prof, av_tex, banner_tex);
                        // Com "Amigos" instalada: o botao de amizade do cartao (SocialActions).
                        #[cfg(feature = "mod_amigos")]
                        if app.amigos.is_some() {
                            ui.add_space(10.0);
                            crate::ui::friends_view::render_profile_friend_action(app, ui, &uid);
                        }
                    } else if is_loading {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new("Carregando perfil...")
                                    .size(13.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(24.0);
                        });
                    } else if let Some(ref err) = prof_err {
                        ui.label(
                            RichText::new(err)
                                .size(12.5)
                                .color(Color32::from_rgb(252, 165, 165)),
                        );
                    }
                }
            }
        });

    if !open || close_clicked || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.close_profile_modal();
    }
}

fn render_guest_card(ui: &mut egui::Ui, name: &str, av_tex: Option<egui::TextureId>) {
    egui::Frame::none()
        .fill(BG_TILE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                paint_profile_avatar(ui, name, av_tex, 52.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(name).size(16.0).semibold().color(TEXT_PRIMARY));
                    ui.label(
                        RichText::new("Convidado nesta chamada (sem conta Go Live registrada)")
                            .size(12.0)
                            .color(TEXT_MUTED),
                    );
                });
            });
        });
}

fn parse_hex_color(s: &str) -> Option<Color32> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Color32::from_rgb(r, g, b));
    }
    if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
        return Some(Color32::from_rgb(r, g, b));
    }
    None
}

fn parse_css_gradient_banner(raw: &str) -> Option<(Color32, Color32, f32, f32)> {
    let t = raw.trim();
    let lower = t.to_ascii_lowercase();
    if !lower.contains("linear-gradient(") {
        return None;
    }
    let inner = t.split_once('(')?.1.rsplit_once(')')?.0;
    let parts: Vec<&str> = inner.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let mut angle = 135.0_f32;
    let mut stops: Vec<(Color32, Option<f32>)> = Vec::new();
    for part in parts {
        let p_low = part.to_ascii_lowercase();
        if p_low.ends_with("deg") {
            if let Ok(deg) = p_low.trim_end_matches("deg").trim().parse::<f32>() {
                angle = deg;
            }
            continue;
        }
        if let Some(hex_tok) = part.split_whitespace().find(|tok| tok.starts_with('#')) {
            if let Some(col) = parse_hex_color(hex_tok) {
                let pct = part
                    .split_whitespace()
                    .find(|tok| tok.ends_with('%'))
                    .and_then(|tok| tok.trim_end_matches('%').parse::<f32>().ok())
                    .map(|p| (p / 100.0).max(0.2));
                stops.push((col, pct));
            }
        }
    }
    if stops.len() >= 2 {
        let from = stops[0].0;
        let to = stops.last().unwrap().0;
        let end_stop = stops.last().unwrap().1.unwrap_or(1.0);
        Some((from, to, angle, end_stop))
    } else {
        None
    }
}

fn lerp_color32(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
    )
}

fn paint_gradient_banner(
    painter: &egui::Painter,
    rect: Rect,
    from: Color32,
    to: Color32,
    angle_deg: f32,
    end_stop: f32,
) {
    // Gradiente suave com ângulo CSS real (`135deg` = superior esquerdo -> inferior direito) em Mesh de 4 vértices
    let rad = angle_deg.to_radians();
    // CSS 0deg = para cima (0, -1), 90deg = direita (1, 0), 135deg = (+0.707, +0.707)
    let dx = rad.sin();
    let dy = -rad.cos();
    let half_proj = (dx.abs() + dy.abs()) * 0.5;
    let span = (half_proj * 2.0 * end_stop.max(0.4)).max(0.1);
    let corner_color = |nx: f32, ny: f32| -> Color32 {
        let proj = (nx - 0.5) * dx + (ny - 0.5) * dy;
        let t = (proj + half_proj) / span;
        lerp_color32(from, to, t)
    };

    let c_tl = corner_color(0.0, 0.0);
    let c_tr = corner_color(1.0, 0.0);
    let c_br = corner_color(1.0, 1.0);
    let c_bl = corner_color(0.0, 1.0);

    let mut mesh = egui::Mesh::default();
    let uv = egui::pos2(0.0, 0.0);
    mesh.vertices.push(egui::epaint::Vertex { pos: rect.left_top(), uv, color: c_tl });
    mesh.vertices.push(egui::epaint::Vertex { pos: rect.right_top(), uv, color: c_tr });
    mesh.vertices.push(egui::epaint::Vertex { pos: rect.right_bottom(), uv, color: c_br });
    mesh.vertices.push(egui::epaint::Vertex { pos: rect.left_bottom(), uv, color: c_bl });
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

fn render_account_card(
    ui: &mut egui::Ui,
    prof: &crate::net::auth::UserProfileData,
    av_tex: Option<egui::TextureId>,
    banner_tex: Option<egui::TextureId>,
) {
    let acc = &prof.account;
    let tier = PlanTier::from_flags(&acc.flags, true);
    let name_col = resolve_name_color_rgb(acc.equipped_name_color.as_deref())
        .map(|[r, g, b]| Color32::from_rgb(r, g, b))
        .unwrap_or(TEXT_PRIMARY);

    // Banner real igual `UserProfileCard.tsx`:
    // 1) bannerUrl, 2) equippedProfileColor (solido ou linear-gradient), 3) profileTheme, 4) linear-gradient(135deg, #18181b 0%, #10b981 140%)
    let banner_w = ui.available_width();
    let (b_rect, _) = ui.allocate_exact_size(Vec2::new(banner_w, 102.0), Sense::hover());
    if let Some(tid) = banner_tex {
        ui.painter().image(
            tid,
            b_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else if let Some((c1, c2, ang, end_stop)) = acc
        .equipped_profile_color
        .as_deref()
        .and_then(parse_css_gradient_banner)
    {
        paint_gradient_banner(ui.painter(), b_rect, c1, c2, ang, end_stop);
    } else if let Some(c) = acc.equipped_profile_color.as_deref().and_then(|s| {
        parse_hex_color(s).or_else(|| resolve_name_color_rgb(Some(s)).map(|[r, g, b]| Color32::from_rgb(r, g, b)))
    }) {
        let dark = lerp_color32(c, Color32::from_rgb(12, 16, 24), 0.45);
        paint_gradient_banner(ui.painter(), b_rect, c, dark, 135.0, 1.2);
    } else if let Some(ref theme) = acc.profile_theme {
        let c1 = theme
            .get("from")
            .or_else(|| theme.get("color1"))
            .and_then(|v| v.as_str())
            .and_then(parse_hex_color)
            .unwrap_or_else(|| Color32::from_rgb(24, 24, 27));
        let c2 = theme
            .get("to")
            .or_else(|| theme.get("color2"))
            .and_then(|v| v.as_str())
            .and_then(parse_hex_color)
            .unwrap_or_else(|| Color32::from_rgb(16, 185, 129));
        let ang = theme
            .get("angle")
            .and_then(|v| v.as_f64())
            .unwrap_or(135.0) as f32;
        paint_gradient_banner(ui.painter(), b_rect, c1, c2, ang, 1.0);
    } else {
        // Banner padrao oficial do GoLive (`linear-gradient(135deg, #18181b 0%, #10b981 140%)`)
        paint_gradient_banner(
            ui.painter(),
            b_rect,
            Color32::from_rgb(24, 24, 27),
            Color32::from_rgb(16, 185, 129),
            135.0,
            1.4,
        );
    }
    ui.painter()
        .rect_stroke(b_rect, Rounding::same(4.0), Stroke::new(1.0_f32, BORDER_SUBTLE));

    // Avatar sobreposto no canto esquerdo do banner + nome/selos
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        paint_profile_avatar(ui, &acc.display_name, av_tex, 54.0);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&acc.display_name)
                        .size(17.0)
                        .semibold()
                        .color(name_col),
                );
                if let Some(tone) = verified_badge_tone(&acc.flags) {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    icons::paint_verified_seal(ui.painter(), r.center(), 7.2, tone);
                }
            });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("@{}", acc.username))
                        .size(12.0)
                        .color(TEXT_MUTED),
                );
                let (pill_bg, pill_border, pill_fg) = match tier {
                    PlanTier::ProUltra => (
                        Color32::from_rgb(136, 19, 55),
                        Color32::from_rgb(251, 113, 133),
                        Color32::from_rgb(255, 228, 230),
                    ),
                    PlanTier::PremiumMax => (
                        Color32::from_rgb(120, 53, 15),
                        Color32::from_rgb(251, 191, 36),
                        Color32::from_rgb(254, 243, 199),
                    ),
                    PlanTier::Premium => (
                        Color32::from_rgb(30, 58, 138),
                        Color32::from_rgb(96, 165, 250),
                        Color32::from_rgb(219, 234, 254),
                    ),
                    _ => (BG_ELEVATED, BORDER_SUBTLE, EMERALD_BRIGHT),
                };
                egui::Frame::none()
                    .fill(pill_bg)
                    .stroke(Stroke::new(1.0_f32, pill_border))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(7.0, 1.5))
                    .show(ui, |ui| {
                        ui.label(RichText::new(tier.label()).size(10.5).semibold().color(pill_fg));
                    });
                let (cr, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                icons::paint_coin(ui.painter(), cr.center(), 12.5, GOLD);
                ui.label(
                    RichText::new(format_points_br(acc.points))
                        .size(11.5)
                        .semibold()
                        .color(GOLD),
                );
            });
        });
    });

    if let Some((ref room, ppl)) = prof.live_room {
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(EMERALD.linear_multiply(0.18))
            .stroke(Stroke::new(1.0_f32, EMERALD))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Ao vivo em #{} ({} na sala)", room, ppl))
                            .size(12.0)
                            .semibold()
                            .color(EMERALD_BRIGHT),
                    );
                });
            });
    }

    if let Some(ref bio) = acc.bio {
        let trimmed = bio.trim();
        if !trimmed.is_empty() {
            ui.add_space(8.0);
            egui::Frame::none()
                .fill(BG_TILE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(RichText::new(trimmed).size(12.5).color(TEXT_PRIMARY))
                            .wrap(),
                    );
                });
        }
    }

    // 3 cards de estatisticas (Tempo em chamada, Microfone, Compartilhando)
    ui.add_space(10.0);
    let col_w = ((ui.available_width() - 12.0) / 3.0).max(100.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        stat_box(ui, col_w, "Em chamada", acc.call_seconds, BLUE_LIGHT);
        stat_box(ui, col_w, "Microfone", acc.mic_seconds, EMERALD_BRIGHT);
        stat_box(ui, col_w, "Transmitindo", acc.share_seconds, GOLD);
    });

    if !acc.profile_links.is_empty() {
        ui.add_space(10.0);
        ui.label(RichText::new("Links").size(11.5).semibold().color(TEXT_SECONDARY));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for link in &acc.profile_links {
                let lbl = link
                    .name
                    .as_deref()
                    .or(link.auto_name.as_deref())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&link.network);
                if ui
                    .add(
                        egui::Button::new(RichText::new(lbl).size(11.5).color(TEXT_PRIMARY))
                            .fill(BG_ELEVATED)
                            .rounding(Rounding::same(8.0)),
                    )
                    .on_hover_text(&link.url)
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .clicked()
                {
                    crate::ui::external_link::request(&link.url);
                }
            }
        });
    }
}

fn stat_box(ui: &mut egui::Ui, width: f32, label: &str, secs: u64, accent: Color32) {
    egui::Frame::none()
        .fill(BG_TILE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_min_width(width - 20.0);
            ui.label(RichText::new(label).size(11.0).color(TEXT_MUTED));
            ui.add_space(2.0);
            ui.label(
                RichText::new(format_duration_pt(secs))
                    .size(14.0)
                    .semibold()
                    .color(accent),
            );
        });
}

fn paint_profile_avatar(
    ui: &mut egui::Ui,
    name: &str,
    av_tex: Option<egui::TextureId>,
    size: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if let Some(tid) = av_tex {
        icons::paint_circular_avatar(
            ui.painter(),
            rect,
            tid,
            Stroke::new(1.5_f32, BORDER_SUBTLE),
        );
    } else {
        let initial = name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        ui.painter()
            .circle_filled(rect.center(), size * 0.5, avatar_color_for(name));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            egui::FontId::proportional(size * 0.42),
            Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::parse_css_gradient_banner;
    use egui::Color32;

    #[test]
    fn test_parse_css_gradient_banner_and_default_avatar_asset() {
        let parsed = parse_css_gradient_banner("linear-gradient(135deg, #18181b 0%, #10b981 140%)")
            .expect("css gradient");
        assert_eq!(parsed.0, Color32::from_rgb(24, 24, 27));
        assert_eq!(parsed.1, Color32::from_rgb(16, 185, 129));
        assert!((parsed.2 - 135.0).abs() < 0.01);
        assert!((parsed.3 - 1.4).abs() < 0.01);

        let av_bytes = include_bytes!("../../assets/default_avatar_01.png");
        let (w, h, rgba) =
            crate::media::image_decode::decode_image_to_rgba(av_bytes, 128, 128)
                .expect("default avatar png must decode via WIC");
        assert_eq!((w, h), (128, 128));
        assert_eq!(rgba.len(), 128 * 128 * 4);
    }
}
