//! Modal centralizado de Gerenciar Sala e Permissoes (modificacao `permissoes_chamada`).

use crate::ui::theme::SemiBoldExt;
use egui::{Align, Color32, CursorIcon, Layout, RichText, Rounding, Sense, Stroke, Vec2};
use std::collections::HashSet;

use crate::app::GoLiveLiteApp;
use crate::config::MOD_PERMISSOES_CHAMADA;
use crate::net::protocol::{resolve_name_color_rgb, verified_badge_tone, ROOM_PERMISSION_ROWS};
use crate::ui::icons;
use crate::ui::theme::{
    centered_movable_window, BG_ELEVATED, BG_SURFACE, BG_TILE, BLUE_LIGHT, BORDER_SUBTLE, EMERALD,
    GOLD, RED, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageRoomView {
    Menu,
    Admins,
    Limit,
    Bans,
    Permissions,
}

pub fn render_manage_room_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.has_modification(MOD_PERMISSOES_CHAMADA) || app.current_room.is_none() {
        return;
    }
    let Some(current_view) = app
        .manage_room_mod
        .as_ref()
        .filter(|m| m.show_manage_room_modal)
        .map(|m| m.manage_room_view)
    else {
        return;
    };

    let is_owner = app.is_room_owner();
    let is_mgr = app.is_room_manager();
    if !is_mgr {
        if let Some(ref mut m) = app.manage_room_mod {
            m.show_manage_room_modal = false;
        }
        return;
    }

    let mut open = true;
    let mut close_clicked = false;
    let title = match current_view {
        ManageRoomView::Menu => "Gerenciar sala",
        ManageRoomView::Admins => "Gerenciar administradores",
        ManageRoomView::Limit => "Limite de participantes",
        ManageRoomView::Bans => "Banimentos",
        ManageRoomView::Permissions => "Gerenciar permissoes",
    };

    centered_movable_window(
        title,
        "manage_room_modal_window",
        ctx,
        Vec2::new(370.0, 420.0),
    )
    .open(&mut open)
    .title_bar(false)
    .default_width(370.0)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::same(16.0)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(342.0);
            ui.set_max_width(360.0);

            // Cabecalho igual as imagens 4 e 5: [<-] [Engrenagem] Titulo [X]
            ui.horizontal(|ui| {
                if current_view != ManageRoomView::Menu {
                    let back_r = icons::icon_button(
                        ui,
                        Vec2::splat(26.0),
                        Color32::TRANSPARENT,
                        Stroke::NONE,
                        Rounding::same(13.0),
                        "Voltar",
                        |p, c, col| icons::paint_arrow_back(p, c, 14.0, col),
                        TEXT_SECONDARY,
                    );
                    if back_r.clicked() {
                        if let Some(ref mut m) = app.manage_room_mod {
                            m.manage_room_view = ManageRoomView::Menu;
                        }
                    }
                }
                let (gr, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                icons::paint_gear(ui.painter(), gr.center(), 14.0, TEXT_SECONDARY);
                ui.label(RichText::new(title).size(14.5).semibold().color(TEXT_PRIMARY));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let xr = icons::icon_button(
                        ui,
                        Vec2::splat(26.0),
                        Color32::TRANSPARENT,
                        Stroke::NONE,
                        Rounding::same(13.0),
                        "Fechar",
                        |p, c, col| icons::paint_close_x(p, c, 12.5, col),
                        TEXT_SECONDARY,
                    );
                    if xr.clicked() {
                        close_clicked = true;
                    }
                });
            });
            ui.add_space(10.0);

            match current_view {
                ManageRoomView::Menu => render_menu_view(app, ui, is_owner),
                ManageRoomView::Admins => render_admins_view(app, ui, is_owner),
                ManageRoomView::Limit => render_limit_view(app, ui),
                ManageRoomView::Bans => render_bans_view(app, ui),
                ManageRoomView::Permissions => render_permissions_view(app, ui, is_mgr),
            }
        });

    if !open || close_clicked {
        if let Some(ref mut m) = app.manage_room_mod {
            m.show_manage_room_modal = false;
        }
    }
}

fn menu_row_button(
    ui: &mut egui::Ui,
    label: &str,
    hint: Option<&str>,
    icon_color: Color32,
    paint_icon: impl FnOnce(&egui::Painter, egui::Pos2, Color32),
) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 42.0), Sense::click());
    let bg = if resp.hovered() { BG_ELEVATED } else { BG_TILE };
    ui.painter().rect(
        rect,
        Rounding::same(10.0),
        bg,
        Stroke::new(1.0_f32, BORDER_SUBTLE),
    );
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 20.0, rect.center().y),
        icon_color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 36.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        TEXT_PRIMARY,
    );
    if let Some(h) = hint {
        ui.painter().text(
            egui::pos2(rect.right() - 28.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            h,
            egui::FontId::proportional(11.5),
            TEXT_MUTED,
        );
    }
    icons::paint_chevron_right(
        ui.painter(),
        egui::pos2(rect.right() - 15.0, rect.center().y),
        12.0,
        TEXT_MUTED,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn render_menu_view(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, is_owner: bool) {
    if is_owner
        && menu_row_button(ui, "Gerenciar administradores", None, GOLD, |p, c, col| {
            icons::paint_crown(p, c, 14.0, col)
        })
    {
        if let Some(ref mut m) = app.manage_room_mod {
            m.manage_room_view = ManageRoomView::Admins;
        }
    }
    ui.add_space(6.0);

    let limit_hint = match app.room_member_limit {
        Some(n) => format!("{}", n),
        None => "sem limite".to_string(),
    };
    if menu_row_button(
        ui,
        "Limite de participantes",
        Some(&limit_hint),
        BLUE_LIGHT,
        |p, c, col| icons::paint_groups(p, c, 14.5, col),
    ) {
        let draft = app
            .room_member_limit
            .map(|n| n.to_string())
            .unwrap_or_default();
        if let Some(ref mut m) = app.manage_room_mod {
            m.manage_room_limit_draft = draft;
            m.manage_room_view = ManageRoomView::Limit;
        }
    }
    ui.add_space(6.0);

    if is_owner
        && menu_row_button(ui, "Banimentos", None, RED, |p, c, col| {
            icons::paint_gavel(p, c, 14.0, col)
        })
    {
        app.request_room_bans();
        if let Some(ref mut m) = app.manage_room_mod {
            m.manage_room_view = ManageRoomView::Bans;
        }
    }
    ui.add_space(6.0);

    if menu_row_button(
        ui,
        "Gerenciar permissoes",
        None,
        TEXT_SECONDARY,
        |p, c, col| icons::paint_gear(p, c, 14.0, col),
    ) {
        if let Some(ref mut m) = app.manage_room_mod {
            m.manage_room_view = ManageRoomView::Permissions;
        }
    }

    if !is_owner {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Apenas o dono da sala pode gerenciar administradores e banimentos.")
                .size(11.0)
                .color(TEXT_MUTED),
        );
    }
}

fn render_admins_view(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, is_owner: bool) {
    if !is_owner {
        if let Some(ref mut m) = app.manage_room_mod {
            m.manage_room_view = ManageRoomView::Menu;
        }
        return;
    }
    ui.label(
        RichText::new(format!("Administradores ({})", app.room_admins.len()))
            .size(12.0)
            .semibold()
            .color(TEXT_SECONDARY),
    );
    ui.add_space(4.0);

    let admins = app.room_admins.clone();
    if admins.is_empty() {
        ui.label(
            RichText::new("Nenhum administrador definido. Promova alguem abaixo.")
                .size(11.5)
                .color(TEXT_MUTED),
        );
    } else {
        for adm in &admins {
            let live_peer = app
                .peers
                .iter()
                .find(|p| p.user_id.as_deref() == Some(adm.id.as_str()));
            let label = live_peer
                .map(|p| p.name.clone())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    if adm.name.is_empty() {
                        None
                    } else {
                        Some(adm.name.clone())
                    }
                })
                .unwrap_or_else(|| adm.id.clone());

            egui::Frame::none()
                .fill(BG_TILE)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&label).size(12.5).semibold().color(TEXT_PRIMARY));
                        if live_peer.is_none() {
                            ui.label(RichText::new("(fora da sala)").size(11.0).color(TEXT_MUTED));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Remover").size(11.0).color(RED),
                                    )
                                    .fill(BG_ELEVATED)
                                    .rounding(Rounding::same(6.0)),
                                )
                                .clicked()
                            {
                                app.remove_room_admin(&adm.id);
                            }
                        });
                    });
                });
            ui.add_space(3.0);
        }
    }

    ui.add_space(10.0);
    ui.label(
        RichText::new("Participantes na sala")
            .size(12.0)
            .semibold()
            .color(TEXT_SECONDARY),
    );
    ui.add_space(4.0);

    let admin_ids: HashSet<&str> = admins.iter().map(|a| a.id.as_str()).collect();
    let promotable: Vec<_> = app
        .peers
        .iter()
        .filter(|p| {
            !p.is_obs()
                && !p.is_silent_moderator()
                && p.user_id.is_some()
                && !app.is_peer_owner(p.user_id.as_deref())
        })
        .cloned()
        .collect();

    if promotable.is_empty() {
        ui.label(
            RichText::new("Nenhum outro participante com conta conectado.")
                .size(11.5)
                .color(TEXT_MUTED),
        );
    } else {
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for p in promotable {
                let Some(ref uid) = p.user_id else {
                    continue;
                };
                let already = admin_ids.contains(uid.as_str());
                let name_col = resolve_name_color_rgb(p.name_color.as_deref())
                    .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                    .unwrap_or(TEXT_PRIMARY);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&p.name).size(12.5).semibold().color(name_col));
                    if let Some(tone) = p.flags.as_deref().and_then(verified_badge_tone) {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                        icons::paint_verified_seal(ui.painter(), r.center(), 5.5, tone);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !already,
                                egui::Button::new(
                                    RichText::new(if already {
                                        "Ja e admin"
                                    } else {
                                        "Tornar admin"
                                    })
                                    .size(11.0),
                                )
                                .fill(BG_ELEVATED)
                                .rounding(Rounding::same(6.0)),
                            )
                            .clicked()
                        {
                            app.add_room_admin(uid);
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
    }
}

fn render_limit_view(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    let present = app
        .peers
        .iter()
        .filter(|p| !p.is_obs() && !p.is_silent_moderator())
        .count()
        + 1;
    ui.label(
        RichText::new("Defina quantas pessoas cabem nesta sala (entre 2 e 200).")
            .size(12.0)
            .color(TEXT_SECONDARY),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!("Agora: {} participante(s) na sala.", present))
            .size(11.5)
            .color(TEXT_MUTED),
    );
    ui.add_space(8.0);

    let mut set_limit_action: Option<Option<u32>> = None;
    ui.horizontal(|ui| {
        if let Some(ref mut m) = app.manage_room_mod {
            ui.add_sized(
                Vec2::new(110.0, 30.0),
                egui::TextEdit::singleline(&mut m.manage_room_limit_draft)
                    .hint_text("Sem limite"),
            );
            if ui
                .add(
                    egui::Button::new(RichText::new("Salvar").size(12.5).semibold().color(Color32::WHITE))
                        .fill(EMERALD)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(ui.available_width(), 30.0)),
                )
                .clicked()
            {
                let trimmed = m.manage_room_limit_draft.trim();
                if trimmed.is_empty() {
                    set_limit_action = Some(None);
                } else if let Ok(v) = trimmed.parse::<u32>() {
                    set_limit_action = Some(Some(v.clamp(2, 200)));
                }
            }
        }
    });
    if let Some(lim) = set_limit_action {
        app.set_room_member_limit(lim);
    }

    if app.room_member_limit.is_some() {
        ui.add_space(8.0);
        if ui
            .add(
                egui::Button::new(RichText::new("Remover limite").size(12.0).color(TEXT_SECONDARY))
                    .fill(BG_ELEVATED)
                    .rounding(Rounding::same(8.0))
                    .min_size(Vec2::new(ui.available_width(), 28.0)),
            )
            .clicked()
        {
            if let Some(ref mut m) = app.manage_room_mod {
                m.manage_room_limit_draft.clear();
            }
            app.set_room_member_limit(None);
        }
    }
}

fn render_bans_view(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Pessoas banidas nao conseguem entrar novamente nesta sala.")
            .size(11.5)
            .color(TEXT_SECONDARY),
    );
    ui.add_space(8.0);

    let bans = app
        .manage_room_mod
        .as_ref()
        .map(|m| m.room_bans.clone())
        .unwrap_or_default();
    if bans.is_empty() {
        ui.label(
            RichText::new("Ninguem banido desta sala.")
                .size(12.0)
                .color(TEXT_MUTED),
        );
    } else {
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            for ban in bans {
                ui.horizontal(|ui| {
                    let lbl = if ban.name.is_empty() {
                        &ban.id
                    } else {
                        &ban.name
                    };
                    ui.label(RichText::new(lbl).size(12.5).color(TEXT_PRIMARY));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Desbanir").size(11.0))
                                    .fill(BG_ELEVATED)
                                    .rounding(Rounding::same(6.0)),
                            )
                            .clicked()
                        {
                            app.unban_member(&ban.id);
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
    }
}

fn render_permissions_view(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, is_mgr: bool) {
    ui.add(
        egui::Label::new(
            RichText::new(
                "Ao desativar uma opcao, so o dono e os administradores da sala continuam podendo fazer aquilo.",
            )
            .size(11.5)
            .color(TEXT_SECONDARY),
        )
        .wrap(),
    );
    ui.add_space(8.0);

    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        for &(key, label) in ROOM_PERMISSION_ROWS {
            let allowed = app.room_permissions.get(key);
            let icon_col = if allowed { EMERALD } else { TEXT_MUTED };
            let (row_rect, row_resp) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 32.0),
                if is_mgr { Sense::click() } else { Sense::hover() },
            );
            if row_resp.hovered() {
                ui.painter()
                    .rect_filled(row_rect, Rounding::same(8.0), BG_ELEVATED);
            }

            let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(row_rect));
            row_ui.horizontal_centered(|ui| {
                ui.add_space(4.0);
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                paint_permission_icon(ui.painter(), ir.center(), key, icon_col);
                ui.add_space(4.0);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(4.0);
                    let sw = icons::paint_toggle_switch(ui, allowed, is_mgr);
                    if is_mgr && sw.clicked() {
                        app.set_room_permission(key, !allowed);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(label).size(12.0).color(TEXT_PRIMARY))
                                .wrap(),
                        );
                    });
                });
            });

            if is_mgr && row_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                app.set_room_permission(key, !allowed);
            }
            ui.add_space(2.0);
        }
    });
}

fn paint_permission_icon(p: &egui::Painter, c: egui::Pos2, key: &str, col: Color32) {
    match key {
        "mic" => icons::paint_mic(p, c, 13.5, col, false),
        "screen" => icons::paint_screen(p, c, 13.5, col),
        "camera" => icons::paint_camera(p, c, 13.5, col),
        "videoSource" => icons::paint_video_source(p, c, 13.5, col),
        "chat" => icons::paint_chat_bubble(p, c, 13.5, col),
        "gif" => {
            p.text(
                c,
                egui::Align2::CENTER_CENTER,
                "GIF",
                egui::FontId::proportional(8.5),
                col,
            );
        }
        "image" => icons::paint_image_frame(p, c, 13.5, col),
        "theme" => icons::paint_palette(p, c, 13.5, col),
        "tools" => icons::paint_tools(p, c, 13.5, col),
        "music" => icons::paint_music(p, c, 13.5, col),
        "recording" => icons::paint_record_dot(p, c, 12.0, col),
        "transcript" => icons::paint_subtitles(p, c, 13.5, col),
        "clips" => icons::paint_scissors(p, c, 13.5, col),
        _ => icons::paint_gear(p, c, 13.5, col),
    }
}
