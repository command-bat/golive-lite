//! Icones vetoriais puros pintados via `egui::Painter` (zero texturas extras, zero alocacoes).

use egui::{pos2, vec2, Color32, CursorIcon, Painter, Pos2, Rect, Response, Rounding, Sense, Stroke, Vec2};

use crate::net::protocol::VerifiedTone;

pub fn paint_circular_avatar(
    p: &Painter,
    rect: Rect,
    tex_id: egui::TextureId,
    border: Stroke,
) {
    p.image(
        tex_id,
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    if border.width > 0.0 && border.color != Color32::TRANSPARENT {
        p.circle_stroke(rect.center(), rect.width().min(rect.height()) * 0.5 - 0.5, border);
    }
}

pub fn paint_hand_raise(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    // Palma + 4 dedos erguidos + polegar inspirado em MdBackHand
    let s = sz * 0.46;
    let palm = Rect::from_min_max(
        pos2(c.x - s * 0.48, c.y - s * 0.02),
        pos2(c.x + s * 0.44, c.y + s * 0.86),
    );
    p.rect_filled(palm, Rounding::same((s * 0.34).max(2.0)), col);
    let finger_w = (s * 0.19).max(1.8);
    for (fx, top_y) in [
        (-0.37_f32, -0.58_f32),
        (-0.13, -0.86),
        (0.11, -0.82),
        (0.34, -0.52),
    ] {
        let r = Rect::from_min_max(
            pos2(c.x + s * fx - finger_w * 0.5, c.y + s * top_y),
            pos2(c.x + s * fx + finger_w * 0.5, c.y + s * 0.16),
        );
        p.rect_filled(r, Rounding::same(finger_w * 0.5), col);
    }
    // Polegar inclinado na lateral esquerda
    p.line_segment(
        [pos2(c.x - s * 0.44, c.y + s * 0.48), pos2(c.x - s * 0.82, c.y + s * 0.08)],
        Stroke::new((finger_w * 1.05).max(2.0), col),
    );
}

pub fn icon_button(
    ui: &mut egui::Ui,
    size: Vec2,
    fill: Color32,
    stroke: Stroke,
    rounding: Rounding,
    tooltip: &str,
    paint_fn: impl FnOnce(&Painter, Pos2, Color32),
    icon_color: Color32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let bg = if resp.hovered() {
        brighten(fill, 14)
    } else {
        fill
    };
    ui.painter().rect(rect, rounding, bg, stroke);
    paint_fn(ui.painter(), rect.center(), icon_color);
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}

fn brighten(c: Color32, delta: u8) -> Color32 {
    if c == Color32::TRANSPARENT {
        return Color32::from_rgba_unmultiplied(255, 255, 255, 16);
    }
    Color32::from_rgba_unmultiplied(
        c.r().saturating_add(delta),
        c.g().saturating_add(delta),
        c.b().saturating_add(delta),
        c.a(),
    )
}

pub fn paint_home(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.5;
    let roof = vec![
        pos2(c.x, c.y - s * 0.92),
        pos2(c.x + s * 0.95, c.y - s * 0.05),
        pos2(c.x + s * 0.65, c.y - s * 0.05),
        pos2(c.x + s * 0.65, c.y + s * 0.85),
        pos2(c.x - s * 0.65, c.y + s * 0.85),
        pos2(c.x - s * 0.65, c.y - s * 0.05),
        pos2(c.x - s * 0.95, c.y - s * 0.05),
    ];
    p.add(egui::Shape::closed_line(roof, Stroke::new(1.5_f32, col)));
    p.rect_stroke(
        Rect::from_min_max(
            pos2(c.x - s * 0.22, c.y + s * 0.25),
            pos2(c.x + s * 0.22, c.y + s * 0.85),
        ),
        Rounding::same(1.5),
        Stroke::new(1.3_f32, col),
    );
}

pub fn paint_lock(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.5;
    let body = Rect::from_min_max(
        pos2(c.x - s * 0.62, c.y - s * 0.05),
        pos2(c.x + s * 0.62, c.y + s * 0.82),
    );
    p.rect_stroke(body, Rounding::same(2.0), Stroke::new(1.4_f32, col));
    let shackle = vec![
        pos2(c.x - s * 0.38, c.y - s * 0.05),
        pos2(c.x - s * 0.38, c.y - s * 0.55),
        pos2(c.x - s * 0.18, c.y - s * 0.8),
        pos2(c.x + s * 0.18, c.y - s * 0.8),
        pos2(c.x + s * 0.38, c.y - s * 0.55),
        pos2(c.x + s * 0.38, c.y - s * 0.05),
    ];
    p.add(egui::Shape::line(shackle, Stroke::new(1.4_f32, col)));
}

pub fn paint_globe(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let r = sz * 0.44;
    p.circle_stroke(c, r, Stroke::new(1.35_f32, col));
    p.line_segment([pos2(c.x - r, c.y), pos2(c.x + r, c.y)], Stroke::new(1.1_f32, col));
    p.line_segment([pos2(c.x, c.y - r), pos2(c.x, c.y + r)], Stroke::new(1.1_f32, col));
}

pub fn paint_link(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.line_segment([pos2(c.x - s * 0.4, c.y + s * 0.4), pos2(c.x + s * 0.4, c.y - s * 0.4)], st);
    p.circle_stroke(pos2(c.x - s * 0.48, c.y + s * 0.42), s * 0.42, st);
    p.circle_stroke(pos2(c.x + s * 0.48, c.y - s * 0.42), s * 0.42, st);
}

pub fn paint_shop(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    // Storefront awning + base inspired by PiStorefront / Material storefront
    let s = sz * 0.50;
    let st = Stroke::new(1.45_f32, col);
    // Toldo superior (trapezoid awning)
    let top_y = c.y - s * 0.78;
    let mid_y = c.y - s * 0.16;
    let bot_y = c.y + s * 0.80;
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x - s * 0.68, top_y),
            pos2(c.x + s * 0.68, top_y),
            pos2(c.x + s * 0.90, mid_y),
            pos2(c.x - s * 0.90, mid_y),
        ],
        Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 32),
        st,
    ));
    // Divisorias do toldo
    for frac in [-0.30_f32, 0.30_f32] {
        p.line_segment(
            [pos2(c.x + s * frac * 0.75, top_y), pos2(c.x + s * frac, mid_y)],
            Stroke::new(1.15_f32, col),
        );
    }
    // Corpo da loja
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.72, mid_y), pos2(c.x + s * 0.72, bot_y)),
        Rounding { nw: 0.0, ne: 0.0, sw: 2.0, se: 2.0 },
        st,
    );
    // Porta central da loja
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.24, c.y + s * 0.20), pos2(c.x + s * 0.24, bot_y)),
        Rounding { nw: 1.5, ne: 1.5, sw: 0.0, se: 0.0 },
        Stroke::new(1.20_f32, col),
    );
}

pub fn paint_bell(p: &Painter, c: Pos2, sz: f32, col: Color32, muted_slash: bool) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x - s * 0.72, c.y + s * 0.45),
            pos2(c.x - s * 0.55, c.y - s * 0.25),
            pos2(c.x - s * 0.32, c.y - s * 0.72),
            pos2(c.x + s * 0.32, c.y - s * 0.72),
            pos2(c.x + s * 0.55, c.y - s * 0.25),
            pos2(c.x + s * 0.72, c.y + s * 0.45),
        ],
        st,
    ));
    p.line_segment([pos2(c.x - s * 0.22, c.y + s * 0.75), pos2(c.x + s * 0.22, c.y + s * 0.75)], st);
    if muted_slash {
        p.line_segment(
            [pos2(c.x - s * 0.85, c.y + s * 0.75), pos2(c.x + s * 0.85, c.y - s * 0.75)],
            Stroke::new(1.6_f32, col),
        );
    }
}

pub fn paint_dots_vertical(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let d = sz * 0.32;
    let r = (sz * 0.095).max(1.3);
    p.circle_filled(pos2(c.x, c.y - d), r, col);
    p.circle_filled(c, r, col);
    p.circle_filled(pos2(c.x, c.y + d), r, col);
}

pub fn paint_user_plus(p: &Painter, c: Pos2, sz: f32, col: Color32) {
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
    p.line_segment([pos2(c.x + s * 0.60, c.y - s * 0.40), pos2(c.x + s * 0.60, c.y + s * 0.10)], st);
}

pub fn paint_verified_seal(p: &Painter, c: Pos2, r: f32, tone: VerifiedTone) {
    let (fill, rim) = match tone {
        VerifiedTone::Ruby => (
            Color32::from_rgb(225, 29, 72),
            Color32::from_rgb(159, 18, 57),
        ),
        VerifiedTone::Gold => (
            Color32::from_rgb(234, 179, 8),
            Color32::from_rgb(161, 98, 7),
        ),
        VerifiedTone::Blue => (
            Color32::from_rgb(59, 130, 246),
            Color32::from_rgb(29, 78, 216),
        ),
    };
    // Selo recortado suave de 12 lobulos igual ao VerifiedBadge oficial
    let steps = 48usize;
    let mut pts = Vec::with_capacity(steps);
    for i in 0..steps {
        let ang = (i as f32) * std::f32::consts::TAU / (steps as f32);
        let wave = (12.0 * ang).cos();
        let rad = r * (0.90 + 0.14 * wave);
        pts.push(pos2(c.x + ang.cos() * rad, c.y + ang.sin() * rad));
    }
    p.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(0.8_f32, rim)));

    // Checkmark central nitido
    let check = vec![
        pos2(c.x - r * 0.40, c.y + r * 0.04),
        pos2(c.x - r * 0.10, c.y + r * 0.34),
        pos2(c.x + r * 0.44, c.y - r * 0.30),
    ];
    p.add(egui::Shape::line(
        check,
        Stroke::new((r * 0.24).max(1.45), Color32::WHITE),
    ));
}

pub fn paint_crown(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.5;
    let pts = vec![
        pos2(c.x - s * 0.88, c.y + s * 0.55),
        pos2(c.x - s * 0.95, c.y - s * 0.42),
        pos2(c.x - s * 0.35, c.y + s * 0.05),
        pos2(c.x, c.y - s * 0.68),
        pos2(c.x + s * 0.35, c.y + s * 0.05),
        pos2(c.x + s * 0.95, c.y - s * 0.42),
        pos2(c.x + s * 0.88, c.y + s * 0.55),
    ];
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

pub fn paint_desktop_app(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.3_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.88, c.y - s * 0.72), pos2(c.x + s * 0.88, c.y + s * 0.38)),
        Rounding::same(1.8),
        st,
    );
    p.line_segment([pos2(c.x, c.y + s * 0.38), pos2(c.x, c.y + s * 0.78)], st);
    p.line_segment([pos2(c.x - s * 0.48, c.y + s * 0.78), pos2(c.x + s * 0.48, c.y + s * 0.78)], st);
    let q = s * 0.22;
    p.rect_filled(
        Rect::from_min_max(pos2(c.x - q, c.y - s * 0.17 - q), pos2(c.x + q, c.y - s * 0.17 + q)),
        Rounding::same(0.5),
        col,
    );
}

pub fn paint_desktop_lite_app(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.3_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.88, c.y - s * 0.72), pos2(c.x + s * 0.88, c.y + s * 0.38)),
        Rounding::same(1.8),
        st,
    );
    p.line_segment([pos2(c.x, c.y + s * 0.38), pos2(c.x, c.y + s * 0.78)], st);
    p.line_segment([pos2(c.x - s * 0.48, c.y + s * 0.78), pos2(c.x + s * 0.48, c.y + s * 0.78)], st);
    let cy = c.y - s * 0.17;
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x + s * 0.12, cy - s * 0.38),
            pos2(c.x - s * 0.28, cy + s * 0.05),
            pos2(c.x + s * 0.04, cy + s * 0.05),
            pos2(c.x - s * 0.12, cy + s * 0.38),
            pos2(c.x + s * 0.28, cy - s * 0.05),
            pos2(c.x - s * 0.04, cy - s * 0.05),
        ],
        Stroke::new(1.1_f32, col),
    ));
}

pub fn paint_phone_app(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.52, c.y - s * 0.86), pos2(c.x + s * 0.52, c.y + s * 0.86)),
        Rounding::same(2.0),
        Stroke::new(1.3_f32, col),
    );
    p.circle_filled(pos2(c.x, c.y + s * 0.58), 1.0, col);
}

pub fn paint_mic(p: &Painter, c: Pos2, sz: f32, col: Color32, off: bool) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.30, c.y - s * 0.82), pos2(c.x + s * 0.30, c.y + s * 0.18)),
        Rounding::same(3.5),
        st,
    );
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.62, c.y - s * 0.05),
            pos2(c.x - s * 0.48, c.y + s * 0.42),
            pos2(c.x, c.y + s * 0.54),
            pos2(c.x + s * 0.48, c.y + s * 0.42),
            pos2(c.x + s * 0.62, c.y - s * 0.05),
        ],
        st,
    ));
    p.line_segment([pos2(c.x, c.y + s * 0.54), pos2(c.x, c.y + s * 0.88)], st);
    p.line_segment([pos2(c.x - s * 0.35, c.y + s * 0.88), pos2(c.x + s * 0.35, c.y + s * 0.88)], st);
    if off {
        p.line_segment(
            [pos2(c.x - s * 0.85, c.y + s * 0.80), pos2(c.x + s * 0.85, c.y - s * 0.80)],
            Stroke::new(1.65_f32, col),
        );
    }
}

pub fn paint_headphones(p: &Painter, c: Pos2, sz: f32, col: Color32, off: bool) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.78, c.y + s * 0.25),
            pos2(c.x - s * 0.70, c.y - s * 0.45),
            pos2(c.x - s * 0.30, c.y - s * 0.78),
            pos2(c.x + s * 0.30, c.y - s * 0.78),
            pos2(c.x + s * 0.70, c.y - s * 0.45),
            pos2(c.x + s * 0.78, c.y + s * 0.25),
        ],
        st,
    ));
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.88, c.y + s * 0.08), pos2(c.x - s * 0.46, c.y + s * 0.74)),
        Rounding::same(2.0),
        st,
    );
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x + s * 0.46, c.y + s * 0.08), pos2(c.x + s * 0.88, c.y + s * 0.74)),
        Rounding::same(2.0),
        st,
    );
    if off {
        p.line_segment(
            [pos2(c.x - s * 0.88, c.y + s * 0.80), pos2(c.x + s * 0.88, c.y - s * 0.80)],
            Stroke::new(1.65_f32, col),
        );
    }
}

pub fn paint_speaker(p: &Painter, c: Pos2, sz: f32, col: Color32, muted: bool) {
    let s = sz * 0.48;
    let cone = vec![
        pos2(c.x - s * 0.78, c.y - s * 0.28),
        pos2(c.x - s * 0.35, c.y - s * 0.28),
        pos2(c.x + s * 0.15, c.y - s * 0.70),
        pos2(c.x + s * 0.15, c.y + s * 0.70),
        pos2(c.x - s * 0.35, c.y + s * 0.28),
        pos2(c.x - s * 0.78, c.y + s * 0.28),
    ];
    p.add(egui::Shape::convex_polygon(cone, col, Stroke::NONE));
    let st = Stroke::new(1.35_f32, col);
    if muted {
        p.line_segment([pos2(c.x + s * 0.36, c.y - s * 0.28), pos2(c.x + s * 0.84, c.y + s * 0.28)], st);
        p.line_segment([pos2(c.x + s * 0.84, c.y - s * 0.28), pos2(c.x + s * 0.36, c.y + s * 0.28)], st);
    } else {
        p.add(egui::Shape::line(
            vec![
                pos2(c.x + s * 0.42, c.y - s * 0.32),
                pos2(c.x + s * 0.58, c.y),
                pos2(c.x + s * 0.42, c.y + s * 0.32),
            ],
            st,
        ));
        p.add(egui::Shape::line(
            vec![
                pos2(c.x + s * 0.68, c.y - s * 0.55),
                pos2(c.x + s * 0.90, c.y),
                pos2(c.x + s * 0.68, c.y + s * 0.55),
            ],
            st,
        ));
    }
}

pub fn paint_screen(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.90, c.y - s * 0.68), pos2(c.x + s * 0.90, c.y + s * 0.38)),
        Rounding::same(2.2),
        st,
    );
    p.line_segment([pos2(c.x, c.y + s * 0.38), pos2(c.x, c.y + s * 0.78)], st);
    p.line_segment([pos2(c.x - s * 0.45, c.y + s * 0.78), pos2(c.x + s * 0.45, c.y + s * 0.78)], st);
}

pub fn paint_camera(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.88, c.y - s * 0.50), pos2(c.x + s * 0.32, c.y + s * 0.50)),
        Rounding::same(2.2),
        st,
    );
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x + s * 0.36, c.y - s * 0.18),
            pos2(c.x + s * 0.90, c.y - s * 0.46),
            pos2(c.x + s * 0.90, c.y + s * 0.46),
            pos2(c.x + s * 0.36, c.y + s * 0.18),
        ],
        st,
    ));
}

pub fn paint_video_source(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.85, c.y - s * 0.65), pos2(c.x + s * 0.85, c.y + s * 0.65)),
        Rounding::same(2.0),
        st,
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x - s * 0.20, c.y - s * 0.30),
            pos2(c.x + s * 0.34, c.y),
            pos2(c.x - s * 0.20, c.y + s * 0.30),
        ],
        col,
        Stroke::NONE,
    ));
}

pub fn paint_music(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.line_segment([pos2(c.x - s * 0.15, c.y + s * 0.48), pos2(c.x - s * 0.15, c.y - s * 0.72)], st);
    p.line_segment([pos2(c.x - s * 0.15, c.y - s * 0.72), pos2(c.x + s * 0.55, c.y - s * 0.52)], st);
    p.circle_filled(pos2(c.x - s * 0.38, c.y + s * 0.50), s * 0.28, col);
}

/// O icone de "Ferramentas" do site: `MdHandyman`, desenhado do proprio SVG.
pub fn paint_tools(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    crate::ui::svg_icon::paint(p, "md_handyman", crate::ui::svg_icon::MD_HANDYMAN, c, sz, col);
}

pub fn paint_eye(p: &Painter, c: Pos2, sz: f32, col: Color32, active: bool) {
    let s = sz * 0.48;
    let st = Stroke::new(1.45_f32, col);
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x - s * 0.90, c.y),
            pos2(c.x - s * 0.40, c.y - s * 0.52),
            pos2(c.x + s * 0.40, c.y - s * 0.52),
            pos2(c.x + s * 0.90, c.y),
            pos2(c.x + s * 0.40, c.y + s * 0.52),
            pos2(c.x - s * 0.40, c.y + s * 0.52),
        ],
        st,
    ));
    p.circle_filled(c, s * 0.24, col);
    if !active {
        p.line_segment(
            [pos2(c.x - s * 0.80, c.y + s * 0.70), pos2(c.x + s * 0.80, c.y - s * 0.70)],
            Stroke::new(1.5_f32, col),
        );
    }
}


pub fn paint_hangup(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    // Receptor telefonico solido curvado para baixo (estilo GoLive vermelho/branco da imagem 1)
    let span = sz * 0.50;
    let arch_st = Stroke::new((sz * 0.21).max(2.6), col);
    let pts: Vec<Pos2> = (0..=8)
        .map(|i| {
            let t = (i as f32) / 8.0;
            let x = c.x - span * 0.72 + t * (span * 1.44);
            let arc = (t * std::f32::consts::PI).sin();
            let y = c.y + span * 0.14 - arc * (span * 0.42);
            pos2(x, y)
        })
        .collect();
    p.add(egui::Shape::line(pts, arch_st));

    let ear_w = span * 0.46;
    let ear_h = span * 0.32;
    p.rect_filled(
        Rect::from_center_size(pos2(c.x - span * 0.62, c.y + span * 0.22), vec2(ear_w, ear_h)),
        Rounding::same(2.2),
        col,
    );
    p.rect_filled(
        Rect::from_center_size(pos2(c.x + span * 0.62, c.y + span * 0.22), vec2(ear_w, ear_h)),
        Rounding::same(2.2),
        col,
    );
}

#[allow(dead_code)]
pub fn paint_palette(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let r = sz * 0.42;
    p.circle_stroke(c, r, Stroke::new(1.35_f32, col));
    p.circle_filled(pos2(c.x - r * 0.35, c.y - r * 0.25), 1.3, col);
    p.circle_filled(pos2(c.x + r * 0.25, c.y - r * 0.35), 1.3, col);
    p.circle_filled(pos2(c.x + r * 0.35, c.y + r * 0.15), 1.3, col);
}

pub fn paint_gear(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let r_out = sz * 0.42;
    let r_in = sz * 0.18;
    let st = Stroke::new(1.4_f32, col);
    p.circle_stroke(c, r_out * 0.74, st);
    p.circle_stroke(c, r_in, st);
    for i in 0..6 {
        let a = (i as f32) * std::f32::consts::TAU / 6.0;
        p.line_segment(
            [
                pos2(c.x + a.cos() * (r_out * 0.65), c.y + a.sin() * (r_out * 0.65)),
                pos2(c.x + a.cos() * r_out, c.y + a.sin() * r_out),
            ],
            Stroke::new(1.8_f32, col),
        );
    }
}

pub fn paint_coin(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    // Moeda dourada estilo BsCoin com borda, anel interno e simbolo vertical
    let r = sz * 0.46;
    let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 46);
    p.circle_filled(c, r, fill);
    p.circle_stroke(c, r, Stroke::new(1.55_f32, col));
    p.circle_stroke(c, r * 0.68, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 150)));
    // Tracinho vertical + $ minimalista
    p.line_segment(
        [pos2(c.x, c.y - r * 0.42), pos2(c.x, c.y + r * 0.42)],
        Stroke::new(1.25_f32, col),
    );
    let sw = r * 0.24;
    let sh = r * 0.28;
    p.add(egui::Shape::line(
        vec![
            pos2(c.x + sw, c.y - sh * 0.72),
            pos2(c.x - sw * 0.9, c.y - sh * 0.72),
            pos2(c.x - sw * 0.9, c.y),
            pos2(c.x + sw * 0.9, c.y),
            pos2(c.x + sw * 0.9, c.y + sh * 0.72),
            pos2(c.x - sw, c.y + sh * 0.72),
        ],
        Stroke::new(1.15_f32, col),
    ));
}

pub fn paint_chevron_up(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.38;
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s, c.y + s * 0.45),
            pos2(c.x, c.y - s * 0.50),
            pos2(c.x + s, c.y + s * 0.45),
        ],
        Stroke::new(1.35_f32, col),
    ));
}

pub fn paint_chevron_down(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.38;
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s, c.y - s * 0.45),
            pos2(c.x, c.y + s * 0.50),
            pos2(c.x + s, c.y - s * 0.45),
        ],
        Stroke::new(1.35_f32, col),
    ));
}

pub fn paint_plus(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let st = Stroke::new(1.5_f32, col);
    p.line_segment([pos2(c.x - s, c.y), pos2(c.x + s, c.y)], st);
    p.line_segment([pos2(c.x, c.y - s), pos2(c.x, c.y + s)], st);
}

pub fn paint_send(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x - s * 0.82, c.y - s * 0.72),
            pos2(c.x + s * 0.88, c.y),
            pos2(c.x - s * 0.82, c.y + s * 0.72),
            pos2(c.x - s * 0.42, c.y),
        ],
        col,
        Stroke::NONE,
    ));
}

pub fn media_overlay_circle_button(
    ui: &mut egui::Ui,
    diameter: f32,
    fill: Color32,
    tooltip: &str,
    paint_fn: impl FnOnce(&Painter, Pos2, Color32),
    icon_color: Color32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let bg = if resp.hovered() {
        Color32::from_rgba_unmultiplied(
            fill.r().saturating_add(22),
            fill.g().saturating_add(22),
            fill.b().saturating_add(22),
            fill.a().max(235),
        )
    } else {
        fill
    };
    ui.painter().rect(
        rect,
        Rounding::same(diameter * 0.5),
        bg,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 34)),
    );
    paint_fn(ui.painter(), rect.center(), icon_color);
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}

pub fn paint_pip(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let hw = sz * 0.48;
    let hh = sz * 0.36;
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - hw, c.y - hh), pos2(c.x + hw, c.y + hh)),
        Rounding::same(2.2),
        Stroke::new(1.6_f32, col),
    );
    p.rect_filled(
        Rect::from_min_max(
            pos2(c.x + sz * 0.02, c.y - sz * 0.02),
            pos2(c.x + sz * 0.36, c.y + sz * 0.24),
        ),
        Rounding::same(1.2),
        col,
    );
}

pub fn paint_focus(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let half = sz * 0.44;
    let arm = sz * 0.22;
    let st = Stroke::new(1.6_f32, col);
    for (sx, sy) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let corner = pos2(c.x + sx * half, c.y + sy * half);
        p.line_segment([corner, pos2(corner.x - sx * arm, corner.y)], st);
        p.line_segment([corner, pos2(corner.x, corner.y - sy * arm)], st);
    }
    p.circle_stroke(c, sz * 0.16, st);
}

pub fn paint_fullscreen(p: &Painter, c: Pos2, sz: f32, col: Color32, exit: bool) {
    let half = sz * 0.44;
    let st = Stroke::new(1.6_f32, col);
    if !exit {
        let arm = sz * 0.26;
        for (sx, sy) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let corner = pos2(c.x + sx * half, c.y + sy * half);
            p.line_segment([corner, pos2(corner.x - sx * arm, corner.y)], st);
            p.line_segment([corner, pos2(corner.x, corner.y - sy * arm)], st);
        }
    } else {
        let inner = sz * 0.16;
        for (sx, sy) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let corner = pos2(c.x + sx * inner, c.y + sy * inner);
            p.line_segment([corner, pos2(c.x + sx * half, corner.y)], st);
            p.line_segment([corner, pos2(corner.x, c.y + sy * half)], st);
        }
    }
}

pub fn paint_stats_bars(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let bar_w = (sz * 0.17).max(2.0);
    let gap = sz * 0.12;
    let total_w = bar_w * 3.0 + gap * 2.0;
    let start_x = c.x - total_w * 0.5;
    let bottom_y = c.y + sz * 0.41;
    for (idx, frac) in [(0, 0.34_f32), (1, 0.58_f32), (2, 0.82_f32)] {
        let bx = start_x + (idx as f32) * (bar_w + gap);
        let h = sz * frac;
        p.rect_filled(
            Rect::from_min_max(pos2(bx, bottom_y - h), pos2(bx + bar_w, bottom_y)),
            Rounding::same(0.8),
            col,
        );
    }
}

pub fn paint_stop_square(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    p.rect_filled(
        Rect::from_center_size(c, Vec2::splat(sz * 0.54)),
        Rounding::same(1.8),
        col,
    );
}

pub fn paint_close_x(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let d = sz * 0.32;
    let st = Stroke::new(1.75_f32, col);
    p.line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], st);
    p.line_segment([pos2(c.x + d, c.y - d), pos2(c.x - d, c.y + d)], st);
}

fn paint_sidebar_toggle(p: &Painter, c: Pos2, sz: f32, col: Color32, divider_left: bool, arrow_right: bool) {
    let hw = sz * 0.44;
    let hh = sz * 0.42;
    let st = Stroke::new(1.45_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - hw, c.y - hh), pos2(c.x + hw, c.y + hh)),
        Rounding::same(2.4),
        st,
    );
    let div_x = if divider_left {
        c.x - hw * 0.32
    } else {
        c.x + hw * 0.32
    };
    p.line_segment([pos2(div_x, c.y - hh), pos2(div_x, c.y + hh)], st);

    let ax = if divider_left {
        c.x + hw * 0.32
    } else {
        c.x - hw * 0.32
    };
    let aw = sz * 0.13;
    let ah = sz * 0.17;
    if arrow_right {
        p.add(egui::Shape::line(
            vec![pos2(ax - aw, c.y - ah), pos2(ax + aw, c.y), pos2(ax - aw, c.y + ah)],
            st,
        ));
    } else {
        p.add(egui::Shape::line(
            vec![pos2(ax + aw, c.y - ah), pos2(ax - aw, c.y), pos2(ax + aw, c.y + ah)],
            st,
        ));
    }
}

pub fn paint_panel_left_close(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    paint_sidebar_toggle(p, c, sz, col, true, false);
}

pub fn paint_panel_left_open(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    paint_sidebar_toggle(p, c, sz, col, true, true);
}

pub fn paint_panel_right_close(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    paint_sidebar_toggle(p, c, sz, col, false, true);
}

pub fn paint_panel_right_open(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    paint_sidebar_toggle(p, c, sz, col, false, false);
}

pub fn paint_chevron_right(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.32;
    p.add(egui::Shape::line(
        vec![pos2(c.x - s * 0.45, c.y - s), pos2(c.x + s * 0.55, c.y), pos2(c.x - s * 0.45, c.y + s)],
        Stroke::new(1.5_f32, col),
    ));
}

pub fn paint_arrow_back(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.6_f32, col);
    p.line_segment([pos2(c.x - s * 0.75, c.y), pos2(c.x + s * 0.78, c.y)], st);
    p.add(egui::Shape::line(
        vec![pos2(c.x - s * 0.15, c.y - s * 0.55), pos2(c.x - s * 0.78, c.y), pos2(c.x - s * 0.15, c.y + s * 0.55)],
        st,
    ));
}

pub fn paint_groups(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    let st = Stroke::new(1.35_f32, col);
    p.circle_stroke(pos2(c.x - s * 0.32, c.y - s * 0.28), s * 0.28, st);
    p.circle_stroke(pos2(c.x + s * 0.35, c.y - s * 0.22), s * 0.22, st);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.85, c.y + s * 0.68),
            pos2(c.x - s * 0.62, c.y + s * 0.18),
            pos2(c.x + s * 0.02, c.y + s * 0.18),
            pos2(c.x + s * 0.22, c.y + s * 0.68),
        ],
        st,
    ));
    p.add(egui::Shape::line(
        vec![pos2(c.x + s * 0.22, c.y + s * 0.22), pos2(c.x + s * 0.68, c.y + s * 0.22), pos2(c.x + s * 0.85, c.y + s * 0.65)],
        st,
    ));
}

pub fn paint_gavel(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    let st = Stroke::new(1.6_f32, col);
    p.line_segment([pos2(c.x - s * 0.55, c.y + s * 0.65), pos2(c.x + s * 0.35, c.y - s * 0.25)], st);
    p.rect_filled(
        Rect::from_center_size(pos2(c.x + s * 0.28, c.y - s * 0.36), vec2(s * 0.92, s * 0.44)),
        Rounding::same(1.5),
        col,
    );
    p.line_segment([pos2(c.x - s * 0.78, c.y + s * 0.82), pos2(c.x + s * 0.15, c.y + s * 0.82)], st);
}

pub fn paint_chat_bubble(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    let st = Stroke::new(1.4_f32, col);
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x - s * 0.82, c.y - s * 0.62),
            pos2(c.x + s * 0.82, c.y - s * 0.62),
            pos2(c.x + s * 0.82, c.y + s * 0.38),
            pos2(c.x - s * 0.18, c.y + s * 0.38),
            pos2(c.x - s * 0.68, c.y + s * 0.75),
            pos2(c.x - s * 0.55, c.y + s * 0.38),
            pos2(c.x - s * 0.82, c.y + s * 0.38),
        ],
        st,
    ));
}

pub fn paint_image_frame(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    let st = Stroke::new(1.4_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.82, c.y - s * 0.68), pos2(c.x + s * 0.82, c.y + s * 0.68)),
        Rounding::same(2.0),
        st,
    );
    p.circle_filled(pos2(c.x - s * 0.35, c.y - s * 0.25), s * 0.14, col);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.65, c.y + s * 0.48),
            pos2(c.x - s * 0.10, c.y + s * 0.05),
            pos2(c.x + s * 0.20, c.y + s * 0.28),
            pos2(c.x + s * 0.58, c.y - s * 0.08),
            pos2(c.x + s * 0.75, c.y + s * 0.48),
        ],
        st,
    ));
}

pub fn paint_record_dot(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    p.circle_filled(c, sz * 0.34, col);
}

pub fn paint_reaction_smile(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let r = sz * 0.44;
    let st = Stroke::new(1.4_f32, col);
    p.circle_stroke(c, r, st);
    p.circle_filled(pos2(c.x - r * 0.34, c.y - r * 0.18), (r * 0.13).max(1.1), col);
    p.circle_filled(pos2(c.x + r * 0.34, c.y - r * 0.18), (r * 0.13).max(1.1), col);
    let mut smile = Vec::with_capacity(9);
    for i in 0..=8 {
        let t = i as f32 / 8.0;
        let a = std::f32::consts::PI * (0.22 + 0.56 * t);
        smile.push(pos2(c.x + a.cos() * r * 0.52, c.y + a.sin() * r * 0.52));
    }
    p.add(egui::Shape::line(smile, st));
}

pub fn paint_brush(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.45;
    let st = Stroke::new(1.5_f32, col);
    p.line_segment([pos2(c.x - s * 0.65, c.y + s * 0.65), pos2(c.x + s * 0.65, c.y - s * 0.65)], st);
    p.line_segment([pos2(c.x + s * 0.30, c.y - s * 0.70), pos2(c.x + s * 0.70, c.y - s * 0.30)], st);
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x - s * 0.78, c.y + s * 0.78),
            pos2(c.x - s * 0.32, c.y + s * 0.68),
            pos2(c.x - s * 0.68, c.y + s * 0.32),
        ],
        col,
        Stroke::NONE,
    ));
}

pub fn paint_highlighter(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.45;
    let st = Stroke::new(1.5_f32, col);
    p.line_segment([pos2(c.x - s * 0.55, c.y + s * 0.50), pos2(c.x + s * 0.45, c.y - s * 0.50)], Stroke::new(3.2_f32, col));
    p.line_segment([pos2(c.x - s * 0.72, c.y + s * 0.75), pos2(c.x + s * 0.68, c.y + s * 0.75)], st);
}

pub fn paint_line(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    p.line_segment([pos2(c.x - s * 0.72, c.y + s * 0.68), pos2(c.x + s * 0.72, c.y - s * 0.68)], Stroke::new(1.65_f32, col));
}

pub fn paint_arrow_ne(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.55_f32, col);
    let tip = pos2(c.x + s * 0.68, c.y - s * 0.68);
    p.line_segment([pos2(c.x - s * 0.68, c.y + s * 0.68), tip], st);
    p.add(egui::Shape::line(
        vec![
            pos2(tip.x - s * 0.52, tip.y),
            tip,
            pos2(tip.x, tip.y + s * 0.52),
        ],
        st,
    ));
}

pub fn paint_rect_shape(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    p.rect_stroke(
        Rect::from_center_size(c, Vec2::new(s * 1.48, s * 1.15)),
        Rounding::same(2.0),
        Stroke::new(1.5_f32, col),
    );
}

pub fn paint_circle_shape(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    p.circle_stroke(c, sz * 0.38, Stroke::new(1.5_f32, col));
}

pub fn paint_text_tt(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let st = Stroke::new(1.5_f32, col);
    p.line_segment([pos2(c.x - s * 0.85, c.y - s * 0.55), pos2(c.x - s * 0.05, c.y - s * 0.55)], st);
    p.line_segment([pos2(c.x - s * 0.45, c.y - s * 0.55), pos2(c.x - s * 0.45, c.y + s * 0.62)], st);
    p.line_segment([pos2(c.x + s * 0.12, c.y - s * 0.15), pos2(c.x + s * 0.78, c.y - s * 0.15)], st);
    p.line_segment([pos2(c.x + s * 0.45, c.y - s * 0.15), pos2(c.x + s * 0.45, c.y + s * 0.62)], st);
}

pub fn paint_eraser(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.45_f32, col);
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x - s * 0.10, c.y - s * 0.68),
            pos2(c.x + s * 0.72, c.y + s * 0.12),
            pos2(c.x + s * 0.16, c.y + s * 0.68),
            pos2(c.x - s * 0.68, c.y + s * 0.68),
            pos2(c.x - s * 0.76, c.y + s * 0.06),
        ],
        st,
    ));
    p.line_segment([pos2(c.x - s * 0.36, c.y - s * 0.34), pos2(c.x + s * 0.32, c.y + s * 0.34)], st);
}

pub fn paint_undo(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.5_f32, col);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.62, c.y - s * 0.15),
            pos2(c.x + s * 0.25, c.y - s * 0.15),
            pos2(c.x + s * 0.64, c.y + s * 0.18),
            pos2(c.x + s * 0.50, c.y + s * 0.56),
            pos2(c.x + s * 0.05, c.y + s * 0.60),
        ],
        st,
    ));
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.28, c.y - s * 0.52),
            pos2(c.x - s * 0.66, c.y - s * 0.15),
            pos2(c.x - s * 0.28, c.y + s * 0.22),
        ],
        st,
    ));
}

pub fn paint_redo(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.5_f32, col);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x + s * 0.62, c.y - s * 0.15),
            pos2(c.x - s * 0.25, c.y - s * 0.15),
            pos2(c.x - s * 0.64, c.y + s * 0.18),
            pos2(c.x - s * 0.50, c.y + s * 0.56),
            pos2(c.x - s * 0.05, c.y + s * 0.60),
        ],
        st,
    ));
    p.add(egui::Shape::line(
        vec![
            pos2(c.x + s * 0.28, c.y - s * 0.52),
            pos2(c.x + s * 0.66, c.y - s * 0.15),
            pos2(c.x + s * 0.28, c.y + s * 0.22),
        ],
        st,
    ));
}

pub fn paint_trash(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.4_f32, col);
    p.line_segment([pos2(c.x - s * 0.72, c.y - s * 0.46), pos2(c.x + s * 0.72, c.y - s * 0.46)], st);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.28, c.y - s * 0.72), pos2(c.x + s * 0.28, c.y - s * 0.46)),
        Rounding::same(1.0),
        st,
    );
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.54, c.y - s * 0.46), pos2(c.x + s * 0.54, c.y + s * 0.74)),
        Rounding::same(1.5),
        st,
    );
    p.line_segment([pos2(c.x - s * 0.18, c.y - s * 0.18), pos2(c.x - s * 0.18, c.y + s * 0.46)], st);
    p.line_segment([pos2(c.x + s * 0.18, c.y - s * 0.18), pos2(c.x + s * 0.18, c.y + s * 0.46)], st);
}

pub fn paint_paste(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.35_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.60, c.y - s * 0.52), pos2(c.x + s * 0.60, c.y + s * 0.74)),
        Rounding::same(2.0),
        st,
    );
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.28, c.y - s * 0.74), pos2(c.x + s * 0.28, c.y - s * 0.44)),
        Rounding::same(1.0),
        st,
    );
    p.line_segment([pos2(c.x - s * 0.30, c.y + s * 0.02), pos2(c.x + s * 0.30, c.y + s * 0.02)], st);
    p.line_segment([pos2(c.x - s * 0.30, c.y + s * 0.34), pos2(c.x + s * 0.18, c.y + s * 0.34)], st);
}

pub fn paint_pan_move(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.35_f32, col);
    p.line_segment([pos2(c.x - s * 0.74, c.y), pos2(c.x + s * 0.74, c.y)], st);
    p.line_segment([pos2(c.x, c.y - s * 0.74), pos2(c.x, c.y + s * 0.74)], st);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        let tip = pos2(c.x + dx * s * 0.74, c.y + dy * s * 0.74);
        let px = -dy * s * 0.24;
        let py = dx * s * 0.24;
        let back = pos2(tip.x - dx * s * 0.24, tip.y - dy * s * 0.24);
        p.add(egui::Shape::line(
            vec![pos2(back.x + px, back.y + py), tip, pos2(back.x - px, back.y - py)],
            st,
        ));
    }
}

pub fn paint_align_lines(p: &Painter, c: Pos2, sz: f32, col: Color32, mode: u8) {
    let s = sz * 0.44;
    let st = Stroke::new(1.4_f32, col);
    for (i, w_frac) in [1.0_f32, 0.62, 0.86, 0.56].iter().enumerate() {
        let y = c.y - s * 0.54 + (i as f32) * s * 0.36;
        let full_w = s * 1.48;
        let line_w = full_w * w_frac;
        let x0 = match mode {
            1 => c.x - line_w * 0.5,
            2 => c.x + full_w * 0.5 - line_w,
            _ => c.x - full_w * 0.5,
        };
        p.line_segment([pos2(x0, y), pos2(x0 + line_w, y)], st);
    }
}

pub fn paint_list_bullet(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.4_f32, col);
    for row in [-0.48_f32, 0.0, 0.48] {
        let y = c.y + s * row;
        p.circle_filled(pos2(c.x - s * 0.62, y), 1.4, col);
        p.line_segment([pos2(c.x - s * 0.26, y), pos2(c.x + s * 0.74, y)], st);
    }
}

pub fn paint_list_number(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.4_f32, col);
    for (idx, row) in [-0.48_f32, 0.0, 0.48].iter().enumerate() {
        let y = c.y + s * row;
        p.line_segment([pos2(c.x - s * 0.72, y - 1.5 + (idx as f32) * 0.4), pos2(c.x - s * 0.46, y + 1.5)], Stroke::new(1.2_f32, col));
        p.line_segment([pos2(c.x - s * 0.24, y), pos2(c.x + s * 0.74, y)], st);
    }
}

pub fn paint_quote(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let st = Stroke::new(1.5_f32, col);
    for off in [-0.36_f32, 0.30] {
        let qx = c.x + s * off;
        p.rect_filled(Rect::from_min_max(pos2(qx - 2.0, c.y - 3.0), pos2(qx + 2.0, c.y + 1.0)), Rounding::same(1.0), col);
        p.line_segment([pos2(qx + 1.5, c.y), pos2(qx - 0.5, c.y + 4.0)], st);
    }
}

pub fn paint_table(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let st = Stroke::new(1.3_f32, col);
    let r = Rect::from_center_size(c, Vec2::new(s * 1.48, s * 1.24));
    p.rect_stroke(r, Rounding::same(1.5), st);
    p.line_segment([pos2(r.left(), c.y - s * 0.16), pos2(r.right(), c.y - s * 0.16)], st);
    p.line_segment([pos2(r.left(), c.y + s * 0.24), pos2(r.right(), c.y + s * 0.24)], st);
    p.line_segment([pos2(c.x - s * 0.22, r.top()), pos2(c.x - s * 0.22, r.bottom())], st);
    p.line_segment([pos2(c.x + s * 0.22, r.top()), pos2(c.x + s * 0.22, r.bottom())], st);
}

pub fn paint_fill_bucket(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let st = Stroke::new(1.4_f32, col);
    p.add(egui::Shape::closed_line(
        vec![
            pos2(c.x - s * 0.58, c.y - s * 0.10),
            pos2(c.x - s * 0.02, c.y - s * 0.66),
            pos2(c.x + s * 0.52, c.y - s * 0.12),
            pos2(c.x - s * 0.04, c.y + s * 0.44),
        ],
        st,
    ));
    p.circle_filled(pos2(c.x + s * 0.56, c.y + s * 0.42), 2.1, col);
}

pub fn paint_clear_format(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let st = Stroke::new(1.4_f32, col);
    p.line_segment([pos2(c.x - s * 0.36, c.y - s * 0.56), pos2(c.x + s * 0.52, c.y - s * 0.56)], st);
    p.line_segment([pos2(c.x + s * 0.08, c.y - s * 0.56), pos2(c.x - s * 0.16, c.y + s * 0.52)], st);
    p.line_segment([pos2(c.x - s * 0.64, c.y - s * 0.64), pos2(c.x + s * 0.64, c.y + s * 0.64)], st);
}

pub fn paint_tool_kind_icon(p: &Painter, c: Pos2, sz: f32, kind: &str, col: Color32) {
    match kind {
        // Os mesmos icones do painel do site (`components/roomTools/toolIcons.tsx`).
        "whiteboard" => crate::ui::svg_icon::paint(p, "md_outline_draw", crate::ui::svg_icon::MD_OUTLINE_DRAW, c, sz, col),
        "tasks" => crate::ui::svg_icon::paint(p, "md_checklist", crate::ui::svg_icon::MD_CHECKLIST, c, sz, col),
        "annotate" => crate::ui::svg_icon::paint(p, "md_gesture", crate::ui::svg_icon::MD_GESTURE, c, sz, col),
        "calc" => {
            let s = sz * 0.44;
            let st = Stroke::new(1.4_f32, col);
            let r = Rect::from_center_size(c, Vec2::new(s * 1.36, s * 1.62));
            p.rect_stroke(r, Rounding::same(2.0), st);
            p.rect_filled(
                Rect::from_min_max(pos2(r.left() + 2.5, r.top() + 2.5), pos2(r.right() - 2.5, c.y - s * 0.18)),
                Rounding::same(1.0),
                col,
            );
            for gy in [0.16_f32, 0.50] {
                for gx in [-0.30_f32, 0.0, 0.30] {
                    p.circle_filled(pos2(c.x + s * gx, c.y + s * gy), 1.15, col);
                }
            }
        }
        "wheel" => {
            let r = sz * 0.42;
            p.circle_stroke(c, r, Stroke::new(1.5_f32, col));
            for i in 0..6 {
                let a = (i as f32) * std::f32::consts::TAU / 6.0;
                p.line_segment([c, pos2(c.x + a.cos() * r, c.y + a.sin() * r)], Stroke::new(1.1_f32, col));
            }
            p.circle_filled(c, 2.0, col);
        }
        "board" => paint_brush(p, c, sz, col),
        "doc" | "notepad" => {
            let s = sz * 0.44;
            let st = Stroke::new(1.4_f32, col);
            let r = Rect::from_center_size(c, Vec2::new(s * 1.34, s * 1.62));
            p.rect_stroke(r, Rounding::same(2.0), st);
            p.line_segment([pos2(r.left() + 3.0, c.y - s * 0.28), pos2(r.right() - 3.0, c.y - s * 0.28)], st);
            p.line_segment([pos2(r.left() + 3.0, c.y + s * 0.06), pos2(r.right() - 3.0, c.y + s * 0.06)], st);
            p.line_segment([pos2(r.left() + 3.0, c.y + s * 0.40), pos2(c.x + s * 0.15, c.y + s * 0.40)], st);
        }
        "sheet" => paint_table(p, c, sz, col),
        "code" => {
            let s = sz * 0.44;
            let st = Stroke::new(1.5_f32, col);
            p.add(egui::Shape::line(vec![pos2(c.x - s * 0.24, c.y - s * 0.42), pos2(c.x - s * 0.68, c.y), pos2(c.x - s * 0.24, c.y + s * 0.42)], st));
            p.add(egui::Shape::line(vec![pos2(c.x + s * 0.24, c.y - s * 0.42), pos2(c.x + s * 0.68, c.y), pos2(c.x + s * 0.24, c.y + s * 0.42)], st));
        }
        "poll" => {
            let s = sz * 0.42;
            p.rect_filled(Rect::from_min_max(pos2(c.x - s * 0.68, c.y + s * 0.05), pos2(c.x - s * 0.30, c.y + s * 0.68)), Rounding::same(1.0), col);
            p.rect_filled(Rect::from_min_max(pos2(c.x - s * 0.18, c.y - s * 0.62), pos2(c.x + s * 0.20, c.y + s * 0.68)), Rounding::same(1.0), col);
            p.rect_filled(Rect::from_min_max(pos2(c.x + s * 0.32, c.y - s * 0.24), pos2(c.x + s * 0.70, c.y + s * 0.68)), Rounding::same(1.0), col);
        }
        "reactions" => paint_reaction_smile(p, c, sz, col),
        _ => paint_tools(p, c, sz, col),
    }
}

pub fn paint_windows_logo(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let gap = 1.6_f32;
    let half = s * 0.78 - gap * 0.5;
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let min = pos2(
            if sx < 0.0 { c.x - gap * 0.5 - half } else { c.x + gap * 0.5 },
            if sy < 0.0 { c.y - gap * 0.5 - half } else { c.y + gap * 0.5 },
        );
        p.rect_filled(Rect::from_min_size(min, Vec2::splat(half)), Rounding::same(1.0), col);
    }
}

pub fn paint_subtitles(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    let st = Stroke::new(1.4_f32, col);
    p.rect_stroke(
        Rect::from_min_max(pos2(c.x - s * 0.86, c.y - s * 0.60), pos2(c.x + s * 0.86, c.y + s * 0.60)),
        Rounding::same(2.0),
        st,
    );
    p.line_segment([pos2(c.x - s * 0.52, c.y + s * 0.10), pos2(c.x - s * 0.08, c.y + s * 0.10)], st);
    p.line_segment([pos2(c.x + s * 0.08, c.y + s * 0.10), pos2(c.x + s * 0.52, c.y + s * 0.10)], st);
    p.line_segment([pos2(c.x - s * 0.52, c.y + s * 0.34), pos2(c.x + s * 0.28, c.y + s * 0.34)], st);
}

pub fn paint_scissors(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.45;
    let st = Stroke::new(1.4_f32, col);
    p.circle_stroke(pos2(c.x - s * 0.52, c.y + s * 0.42), s * 0.26, st);
    p.circle_stroke(pos2(c.x + s * 0.52, c.y + s * 0.42), s * 0.26, st);
    p.line_segment([pos2(c.x - s * 0.35, c.y + s * 0.22), pos2(c.x + s * 0.48, c.y - s * 0.68)], st);
    p.line_segment([pos2(c.x + s * 0.35, c.y + s * 0.22), pos2(c.x - s * 0.48, c.y - s * 0.68)], st);
}

pub fn paint_toggle_switch(ui: &mut egui::Ui, active: bool, enabled: bool) -> Response {
    let size = Vec2::new(36.0, 20.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let track_col = if active {
        if enabled {
            Color32::from_rgb(5, 150, 105)
        } else {
            Color32::from_rgb(16, 100, 74)
        }
    } else if resp.hovered() && enabled {
        Color32::from_rgb(63, 63, 70)
    } else {
        Color32::from_rgb(58, 66, 78)
    };
    ui.painter().rect_filled(rect, Rounding::same(10.0), track_col);
    let knob_x = if active {
        rect.right() - 10.0
    } else {
        rect.left() + 10.0
    };
    let knob_col = if enabled {
        Color32::WHITE
    } else {
        Color32::from_rgb(200, 200, 210)
    };
    ui.painter()
        .circle_filled(pos2(knob_x, rect.center().y), 7.5, knob_col);
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn paint_user_outline(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let st = Stroke::new(1.4_f32, col);
    p.circle_stroke(pos2(c.x, c.y - s * 0.36), s * 0.34, st);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.68, c.y + s * 0.72),
            pos2(c.x - s * 0.48, c.y + s * 0.22),
            pos2(c.x + s * 0.48, c.y + s * 0.22),
            pos2(c.x + s * 0.68, c.y + s * 0.72),
        ],
        st,
    ));
}

pub fn paint_discord(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.48;
    let body = vec![
        pos2(c.x - s * 0.46, c.y - s * 0.56),
        pos2(c.x - s * 0.18, c.y - s * 0.46),
        pos2(c.x + s * 0.18, c.y - s * 0.46),
        pos2(c.x + s * 0.46, c.y - s * 0.56),
        pos2(c.x + s * 0.86, c.y - s * 0.16),
        pos2(c.x + s * 0.90, c.y + s * 0.44),
        pos2(c.x + s * 0.46, c.y + s * 0.64),
        pos2(c.x + s * 0.28, c.y + s * 0.42),
        pos2(c.x - s * 0.28, c.y + s * 0.42),
        pos2(c.x - s * 0.46, c.y + s * 0.64),
        pos2(c.x - s * 0.90, c.y + s * 0.44),
        pos2(c.x - s * 0.86, c.y - s * 0.16),
    ];
    p.add(egui::Shape::closed_line(body, Stroke::new(1.35_f32, col)));
    p.circle_filled(pos2(c.x - s * 0.34, c.y + s * 0.04), (s * 0.15).max(1.2), col);
    p.circle_filled(pos2(c.x + s * 0.34, c.y + s * 0.04), (s * 0.15).max(1.2), col);
}

pub fn paint_x_twitter(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    p.line_segment(
        [pos2(c.x - s * 0.78, c.y - s * 0.78), pos2(c.x + s * 0.78, c.y + s * 0.78)],
        Stroke::new(2.0_f32, col),
    );
    p.line_segment(
        [pos2(c.x + s * 0.76, c.y - s * 0.78), pos2(c.x - s * 0.76, c.y + s * 0.78)],
        Stroke::new(1.4_f32, col),
    );
}

pub fn paint_instagram(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.35_f32, col);
    p.rect_stroke(
        Rect::from_center_size(c, Vec2::splat(s * 1.66)),
        Rounding::same(3.2),
        st,
    );
    p.circle_stroke(c, s * 0.38, st);
    p.circle_filled(pos2(c.x + s * 0.48, c.y - s * 0.48), (s * 0.12).max(1.0), col);
}

pub fn paint_square_cloud(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    let st = Stroke::new(1.35_f32, col);
    // Double overlapping rounded squares matching Square Cloud mark
    p.rect_stroke(
        Rect::from_min_max(
            pos2(c.x - s * 0.78, c.y - s * 0.78),
            pos2(c.x + s * 0.46, c.y + s * 0.46),
        ),
        Rounding::same(2.0),
        st,
    );
    p.rect_stroke(
        Rect::from_min_max(
            pos2(c.x - s * 0.46, c.y - s * 0.46),
            pos2(c.x + s * 0.78, c.y + s * 0.78),
        ),
        Rounding::same(2.0),
        st,
    );
}

pub fn paint_checkmark(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.45;
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.75, c.y + s * 0.05),
            pos2(c.x - s * 0.18, c.y + s * 0.62),
            pos2(c.x + s * 0.80, c.y - s * 0.55),
        ],
        Stroke::new((sz * 0.16).max(1.4), col),
    ));
}

pub fn paint_pointer_select(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.46;
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x - s * 0.52, c.y - s * 0.76),
            pos2(c.x + s * 0.64, c.y + s * 0.08),
            pos2(c.x + s * 0.06, c.y + s * 0.18),
            pos2(c.x + s * 0.34, c.y + s * 0.72),
            pos2(c.x + s * 0.08, c.y + s * 0.82),
            pos2(c.x - s * 0.20, c.y + s * 0.28),
            pos2(c.x - s * 0.52, c.y + s * 0.62),
        ],
        col,
        Stroke::NONE,
    ));
}

pub fn paint_shuffle(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.4_f32, col);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.80, c.y - s * 0.45),
            pos2(c.x - s * 0.20, c.y - s * 0.45),
            pos2(c.x + s * 0.22, c.y + s * 0.45),
            pos2(c.x + s * 0.75, c.y + s * 0.45),
        ],
        st,
    ));
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.80, c.y + s * 0.45),
            pos2(c.x - s * 0.20, c.y + s * 0.45),
            pos2(c.x + s * 0.22, c.y - s * 0.45),
            pos2(c.x + s * 0.75, c.y - s * 0.45),
        ],
        st,
    ));
    for sy in [-0.45_f32, 0.45] {
        let tip = pos2(c.x + s * 0.78, c.y + s * sy);
        p.add(egui::Shape::line(
            vec![
                pos2(tip.x - s * 0.24, tip.y - s * 0.22),
                tip,
                pos2(tip.x - s * 0.24, tip.y + s * 0.22),
            ],
            st,
        ));
    }
}

pub fn paint_sort_az(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.44;
    let st = Stroke::new(1.35_f32, col);
    p.line_segment([pos2(c.x - s * 0.46, c.y - s * 0.65), pos2(c.x - s * 0.46, c.y + s * 0.65)], st);
    p.add(egui::Shape::line(
        vec![
            pos2(c.x - s * 0.72, c.y + s * 0.36),
            pos2(c.x - s * 0.46, c.y + s * 0.68),
            pos2(c.x - s * 0.20, c.y + s * 0.36),
        ],
        st,
    ));
    p.line_segment([pos2(c.x + s * 0.05, c.y - s * 0.42), pos2(c.x + s * 0.72, c.y - s * 0.42)], st);
    p.line_segment([pos2(c.x + s * 0.05, c.y), pos2(c.x + s * 0.56, c.y)], st);
    p.line_segment([pos2(c.x + s * 0.05, c.y + s * 0.42), pos2(c.x + s * 0.38, c.y + s * 0.42)], st);
}

pub fn paint_dice(p: &Painter, c: Pos2, sz: f32, col: Color32) {
    let s = sz * 0.42;
    let r = Rect::from_center_size(c, Vec2::splat(s * 1.65));
    p.rect_stroke(r, Rounding::same(2.6), Stroke::new(1.35_f32, col));
    p.circle_filled(c, 1.3, col);
    p.circle_filled(pos2(c.x - s * 0.42, c.y - s * 0.42), 1.3, col);
    p.circle_filled(pos2(c.x + s * 0.42, c.y + s * 0.42), 1.3, col);
}
