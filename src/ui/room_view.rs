use crate::ui::theme::SemiBoldExt;
use egui::{pos2, Align, Color32, CursorIcon, Layout, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use std::sync::atomic::Ordering;

use crate::app::{AppSystemTab, GoLiveLiteApp, MediaTileEntry, UserSettingsTab};
use crate::config::{vk_name, MOD_PERFIL, MOD_PERMISSOES_CHAMADA};
#[cfg(feature = "mod_ferramentas")]
use crate::config::MOD_FERRAMENTAS;
use crate::media::audio::MicCapture;
use crate::media::audio_mixer::AudioMixer;
use crate::net::protocol::{
    format_points_br, is_private_room_handle, resolve_name_color_rgb,
    room_category_label, split_private_room_handle, verified_badge_tone,
};
use crate::net::signaling::now_epoch_ms;
use crate::ui::app_options_modal::{render_app_system_modal, render_user_settings_modal};
use crate::ui::chat_panel::render_chat_panel;
use crate::ui::icons;
#[cfg(feature = "mod_permissoes_chamada")]
use crate::ui::manage_room_modal::render_manage_room_modal;
use crate::ui::participant_row::{avatar_color_for, render_participant_row, ParticipantRowInput};
use crate::ui::partner_card::render_sidebar_partner_section;
#[cfg(feature = "mod_perfil")]
use crate::ui::profile_modal::render_profile_modal;
use crate::ui::quality_modal::{render_share_picker_modal, render_stream_quality_modal};
#[cfg(feature = "mod_ferramentas")]
use crate::ui::room_tools_modal::render_room_tools_modal;
use crate::ui::stats_overlay::render_stats_overlay;
use crate::ui::theme::{
    centered_movable_window, AMBER, BG_ELEVATED, BORDER_STRONG, BG_MAIN, BG_SURFACE, BORDER_SUBTLE,
    EMERALD, EMERALD_BRIGHT, GOLD, RED, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use crate::ui::top_banners::render_top_warns;
use crate::ui::video_tile::{fit_aspect, render_video_tile, VideoTileViewModel};

pub fn render_room_view(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    let tiles = app.collect_media_tiles();
    let has_media = !tiles.is_empty();
    let left_collapsed = has_media && app.left_sidebar_collapsed;
    let right_collapsed = has_media && app.right_sidebar_collapsed;
    let mut participant_menu_opened_this_frame = false;
    render_pip_viewport(app, ctx, &tiles);

    if app.fullscreen_tile_id.is_some() {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::F11)) {
            app.exit_fullscreen(ctx);
        } else if let Some(fs_id) = app.fullscreen_tile_id.clone() {
            if let Some(fs_tile) = tiles
                .iter()
                .find(|t| t.tile_id == fs_id && !t.is_stopped_by_us)
                .cloned()
            {
                render_fullscreen_stage(app, ctx, &fs_tile);
                return;
            } else {
                app.exit_fullscreen(ctx);
            }
        }
    }

    // 0. Top Warns (StatusBanner + AnnouncementBanner ao vivo) no topo absoluto
    render_top_warns(app, ctx);

    // 1. Cabecalho da sala nova (data-rr-header): bg-zinc-950, border-b white/10, px-4 py-2.
    egui::TopBottomPanel::top("golive_header")
        .exact_height(48.0)
        .frame(
            egui::Frame::none()
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(26)))
                .inner_margin(egui::Margin::symmetric(16.0, 8.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0; // gap-2

                // Botao Inicio (Casinha) -> sai da sala para a Home
                let home_resp = icons::icon_button(
                    ui,
                    Vec2::splat(30.0), // p-1.5 + icone text-lg
                    Color32::TRANSPARENT,
                    Stroke::NONE,
                    Rounding::same(14.0),
                    "Voltar ao início",
                    |p, c, col| icons::paint_home(p, c, 18.0, col),
                    TEXT_SECONDARY,
                );
                if home_resp.clicked() {
                    app.leave_room();
                    return;
                }
                // Divisor `h-6 w-px bg-zinc-800` entre o inicio e o nome da sala.
                let (div, _) = ui.allocate_exact_size(Vec2::new(1.0, 24.0), Sense::hover());
                ui.painter().vline(div.center().x, div.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));

                let room_name = app.current_room.clone().unwrap_or_default();
                let is_priv = is_private_room_handle(&room_name);
                if let Some((name, code)) = split_private_room_handle(&room_name) {
                    ui.label(RichText::new(name).size(18.0).semibold().color(ZINC_50));
                    ui.label(RichText::new(code).size(12.0).color(TEXT_MUTED));
                } else {
                    ui.label(
                        RichText::new(room_name.clone())
                            .size(18.0)
                            .semibold()
                            .color(ZINC_50),
                    );
                }

                let (badge_rect, badge_resp) =
                    ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                if is_priv {
                    icons::paint_lock(ui.painter(), badge_rect.center(), 12.5, TEXT_MUTED);
                    badge_resp.on_hover_text("Sala privada");
                } else {
                    icons::paint_globe(ui.painter(), badge_rect.center(), 12.5, TEXT_MUTED);
                    badge_resp.on_hover_text("Sala pública");
                }

                if let Some(cat_lbl) = room_category_label(app.room_category.as_deref()) {
                    egui::Frame::none()
                        .fill(BG_ELEVATED)
                        .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(26)))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(7.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(cat_lbl).size(10.5).color(TEXT_SECONDARY));
                        });
                }

                // Direita: [Compartilhar sala] [Loja/Pro] [Sino TopWarns] [Opcoes do App ...]
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    let menu_resp = icons::icon_button(
                        ui,
                        Vec2::splat(30.0),
                        if app.show_app_system_modal {
                            Color32::from_rgb(42, 49, 60)
                        } else {
                            BG_ELEVATED
                        },
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                        Rounding::same(15.0),
                        "Opcoes do app (bandeja e sobre)",
                        |p, c, col| icons::paint_dots_vertical(p, c, 14.0, col),
                        TEXT_PRIMARY,
                    );
                    if menu_resp.clicked() {
                        app.app_system_tab = AppSystemTab::System;
                        app.show_app_system_modal = !app.show_app_system_modal;
                    }

                    crate::ui::top_banners::alerts_bell_button(app, ui, 28.0);

                    let pro_resp = icons::icon_button(
                        ui,
                        Vec2::splat(28.0),
                        Color32::from_rgb(13, 46, 35),
                        Stroke::new(1.0_f32, Color32::from_rgb(20, 83, 56)),
                        Rounding::same(8.0),
                        "Go Live Pro / Loja",
                        |p, c, col| icons::paint_shop(p, c, 14.5, col),
                        EMERALD_BRIGHT,
                    );
                    if pro_resp.clicked() {
                        crate::ui::external_link::request("https://golive.nemtudo.me/pro");
                    }

                    let copy_recent = app.copied_feedback_until_ms > now_epoch_ms()
                        || app.room_link_copied_until_ms > now_epoch_ms();
                    let (share_rect, share_resp) = ui.allocate_exact_size(
                        Vec2::new(if copy_recent { 108.0 } else { 126.0 }, 28.0),
                        Sense::click(),
                    );
                    let share_bg = if copy_recent {
                        EMERALD
                    } else if share_resp.hovered() {
                        Color32::from_rgb(42, 49, 60)
                    } else {
                        BG_ELEVATED
                    };
                    ui.painter().rect(
                        share_rect,
                        Rounding::same(14.0),
                        share_bg,
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                    );
                    icons::paint_link(
                        ui.painter(),
                        pos2(share_rect.left() + 15.0, share_rect.center().y),
                        12.0,
                        Color32::WHITE,
                    );
                    ui.painter().text(
                        pos2(share_rect.left() + 26.0, share_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        if copy_recent {
                            "Link copiado!"
                        } else {
                            "Compartilhar sala"
                        },
                        egui::FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                    if share_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        app.copy_room_invite_url(ctx);
                        app.copied_feedback_until_ms = now_epoch_ms() + 2000;
                    }
                });
            });
        });

    // Banners de conflito de dispositivo / erro de sala
    if let Some((devices, max_devices)) = app.device_conflict {
        egui::TopBottomPanel::top("device_conflict_banner").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Voce ja esta nesta sala em outro dispositivo ({}/{}). Deseja entrar mesmo assim?",
                        devices, max_devices
                    ))
                    .color(AMBER),
                );
                if ui.button("Entrar mesmo assim").clicked() {
                    app.confirm_device_join();
                }
                if ui.button("Cancelar").clicked() {
                    app.leave_room();
                }
            });
        });
    }

    if let Some(ref err) = app.room_banner_error.clone() {
        egui::TopBottomPanel::top("room_error_banner").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(err).color(Color32::from_rgb(252, 165, 165)));
                if ui.small_button("Fechar").clicked() {
                    app.room_banner_error = None;
                }
            });
        });
    }

    // Popovers e Modais (centralizados e moveis dentro do app)
    crate::ui::top_banners::render_alerts_center(app, ctx);
    render_mic_popover(app, ctx);
    render_speaker_popover(app, ctx);
    render_share_picker_modal(app, ctx);
    render_stream_quality_modal(app, ctx);
    render_app_system_modal(app, ctx);
    render_user_settings_modal(app, ctx);
    #[cfg(feature = "mod_permissoes_chamada")]
    render_manage_room_modal(app, ctx);
    #[cfg(feature = "mod_perfil")]
    render_profile_modal(app, ctx);
    #[cfg(feature = "mod_ferramentas")]
    render_room_tools_modal(app, ctx);

    // 2. Coluna Esquerda (300px fiel ao WatchRoom.tsx): Participantes + "{N} on" / Anuncio de Parceiro
    if !left_collapsed {
        egui::SidePanel::left("participants_panel")
            .resizable(false)
            .default_width(300.0)
            .min_width(300.0)
            .max_width(300.0) // aside w-[300px] do site
            .frame(
                egui::Frame::none()
                    .fill(BG_MAIN)
                    .inner_margin(egui::Margin { left: 12.0, right: 0.0, top: 12.0, bottom: 12.0 }),
            )
            .show(ctx, |ui| {
                let total_h = ui.available_height();
                let max_partner_h = (total_h * 0.45)
                    .max(150.0)
                    .min((total_h - 114.0).max(150.0));
                let list_card_h = (total_h - max_partner_h - 8.0).max(110.0);

                ui.allocate_ui(Vec2::new(ui.available_width(), list_card_h), |ui| {
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(20.0))
                        .inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 8.0, bottom: 8.0 })
                        .show(ui, |ui| {
                            ui.set_min_size(Vec2::new(ui.available_width(), list_card_h));
                            let visible_indices: Vec<usize> = app
                                .peers
                                .iter()
                                .enumerate()
                                .filter(|(_, p)| !p.is_obs() && !p.is_silent_moderator())
                                .map(|(i, _)| i)
                                .collect();

                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Participantes")
                                        .size(14.0)
                                        .semibold()
                                        .color(ZINC_300),
                                );

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if has_media {
                                        let col_resp = icons::icon_button(
                                            ui,
                                            Vec2::splat(22.0),
                                            Color32::TRANSPARENT,
                                            Stroke::NONE,
                                            Rounding::same(6.0),
                                            "Ocultar participantes",
                                            |p, c, col| icons::paint_panel_left_close(p, c, 14.0, col),
                                            TEXT_MUTED,
                                        );
                                        if col_resp.clicked() {
                                            app.left_sidebar_collapsed = true;
                                        }
                                    }

                                    let cnt = visible_indices.len() + 1;
                                    let cnt_txt = match app.room_member_limit {
                                        Some(lim) => format!("{}/{}", cnt, lim),
                                        None => format!("{}", cnt),
                                    };
                                    // Pilula do site: zinc-800, ambar na ultima vaga, vermelha cheia.
                                    let (pill_bg, pill_fg) = match app.room_member_limit {
                                        Some(lim) if cnt as u64 >= lim as u64 => {
                                            (Color32::from_rgb(69, 10, 10), Color32::from_rgb(252, 165, 165))
                                        }
                                        Some(lim) if cnt as u64 + 1 >= lim as u64 => {
                                            (Color32::from_rgb(69, 26, 3), Color32::from_rgb(252, 211, 77))
                                        }
                                        _ => (BORDER_SUBTLE, ZINC_300),
                                    };
                                    egui::Frame::none()
                                        .fill(pill_bg)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(cnt_txt).size(12.0).semibold().color(pill_fg));
                                        });
                                });
                            });
                            // border-b do cabecalho (de ponta a ponta do cartao).
                            ui.add_space(8.0);
                            let full = ui.max_rect().expand2(Vec2::new(12.0, 0.0)).x_range();
                            let y = ui.cursor().top();
                            ui.painter().hline(full, y, Stroke::new(1.0_f32, BORDER_SUBTLE));
                            ui.add_space(8.0);

                            let now = now_epoch_ms();
                            #[cfg(feature = "mod_perfil")]
                            let show_profile_mod = app.has_modification(MOD_PERFIL);
                            #[cfg(not(feature = "mod_perfil"))]
                            let show_profile_mod = false;
                            let self_uid = app.self_user_id.clone();
                            let self_flags = app.effective_self_flags().to_vec();
                            let self_name_color =
                                app.effective_self_name_color().map(|s| s.to_string());
                            let self_av_url =
                                app.effective_self_avatar_url().map(|s| s.to_string());
                            let self_av_tex =
                                app.resolve_and_load_avatar(self_av_url.as_deref());
                            let self_speaking = app.mic.state.speaking.load(Ordering::Relaxed);
                            let self_mic = app.mic.state.enabled.load(Ordering::Relaxed);
                            let self_deaf = app.mixer.deafened.load(Ordering::Relaxed);
                            let self_silenced = app.is_peer_silenced(self_uid.as_deref());
                            let self_owner = app.is_peer_owner(self_uid.as_deref());
                            let self_admin = app.is_peer_admin(self_uid.as_deref());
                            let self_name_str = app.effective_name().to_string();
                            let self_screen = !app.active_screen_slots.is_empty();
                            let self_cam = app.camera_active;

                            let self_hand_raised = {
                                #[cfg(feature = "mod_ferramentas")]
                                {
                                    // Como no site: a mao so aparece com os microfones da sala bloqueados.
                                    !app.room_permissions.mic
                                        && app.room_tools.as_ref().is_some_and(|rt| {
                                            rt.hands.iter().any(|h| {
                                                app.self_user_id.as_deref() == Some(h.id.as_str())
                                                    || app.self_id.as_deref() == Some(h.id.as_str())
                                            })
                                        })
                                }
                                #[cfg(not(feature = "mod_ferramentas"))]
                                {
                                    false
                                }
                            };

                            egui::ScrollArea::vertical()
                                .id_salt("participants_scroll")
                                .max_height((list_card_h - 42.0).max(50.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let self_menu_open =
                                        app.participant_menu_peer_id.as_deref() == Some("__self__");
                                    let self_res = render_participant_row(
                                        ui,
                                        ParticipantRowInput {
                                            display_name: &self_name_str,
                                            is_self: true,
                                            speaking: self_speaking,
                                            mic_on: self_mic,
                                            silenced: self_silenced,
                                            mics_muted: self_deaf,
                                            sharing_screen: self_screen,
                                            sharing_camera: self_cam,
                                            muted: false,
                                            flags: &self_flags,
                                            name_color: self_name_color.as_deref(),
                                            avatar_tex: self_av_tex,
                                            is_owner: self_owner,
                                            is_admin: self_admin,
                                            is_app: true,
                                            is_mobile_app: false,
                                            is_lite_app: true,
                                            hand_raised: self_hand_raised,
                                            menu_open: self_menu_open,
                                            has_menu_actions: show_profile_mod,
                                        },
                                    );
                                    if let Some(pos) = self_res.open_menu_pos {
                                        if self_menu_open {
                                            app.participant_menu_peer_id = None;
                                            app.participant_menu_pos = None;
                                        } else {
                                            app.participant_menu_peer_id =
                                                Some("__self__".to_string());
                                            app.participant_menu_pos = Some(pos);
                                            participant_menu_opened_this_frame = true;
                                        }
                                    }
                                    ui.add_space(3.0);

                                    for idx in visible_indices {
                                        let peer_av_url = app.peers[idx].avatar_url.clone();
                                        let peer_av_tex =
                                            app.resolve_and_load_avatar(peer_av_url.as_deref());
                                        let p_hand_raised = {
                                            #[cfg(feature = "mod_ferramentas")]
                                            {
                                                let p = &app.peers[idx];
                                                !app.room_permissions.mic
                                                    && app.room_tools.as_ref().is_some_and(|rt| {
                                                    rt.hands.iter().any(|h| {
                                                        h.id == p.id
                                                            || p.user_id.as_deref()
                                                                == Some(h.id.as_str())
                                                    })
                                                })
                                            }
                                            #[cfg(not(feature = "mod_ferramentas"))]
                                            {
                                                false
                                            }
                                        };
                                        let peer = &app.peers[idx];
                                        let vkey = peer.volume_key();
                                        let vol_muted = app.config.is_peer_muted(vkey);
                                        let speaking = app.mixer.is_peer_speaking(&peer.id, now);
                                        let peer_flags =
                                            peer.flags.as_deref().unwrap_or(&[]);
                                        let p_uid = peer.user_id.as_deref();
                                        let p_silenced = app.is_peer_silenced(p_uid);
                                        let p_owner = app.is_peer_owner(p_uid);
                                        let p_admin = app.is_peer_admin(p_uid);
                                        let p_label = peer.display_label();
                                        let p_menu_open = app.participant_menu_peer_id.as_deref()
                                            == Some(peer.id.as_str());

                                        let res = render_participant_row(
                                            ui,
                                            ParticipantRowInput {
                                                display_name: &p_label,
                                                is_self: false,
                                                speaking,
                                                mic_on: peer.mic,
                                                silenced: p_silenced,
                                                mics_muted: peer.mics_muted,
                                                sharing_screen: peer
                                                    .screen
                                                    .unwrap_or(peer.sharing),
                                                sharing_camera: peer.camera.unwrap_or(false),
                                                muted: vol_muted,
                                                flags: peer_flags,
                                                name_color: peer.name_color.as_deref(),
                                                avatar_tex: peer_av_tex,
                                                is_owner: p_owner,
                                                is_admin: p_admin,
                                                is_app: peer.app.unwrap_or(false),
                                                is_mobile_app: peer.mobile_app.unwrap_or(false),
                                                is_lite_app: peer.lite_app.unwrap_or(false),
                                                hand_raised: p_hand_raised,
                                                menu_open: p_menu_open,
                                                has_menu_actions: true,
                                            },
                                        );

                                        if let Some(pos) = res.open_menu_pos {
                                            if p_menu_open {
                                                app.participant_menu_peer_id = None;
                                                app.participant_menu_pos = None;
                                            } else {
                                                app.participant_menu_peer_id =
                                                    Some(peer.id.clone());
                                                app.participant_menu_pos = Some(pos);
                                                participant_menu_opened_this_frame = true;
                                            }
                                        }
                                        ui.add_space(3.0);
                                    }
                                });
                        });
                });

                ui.add_space(8.0);
                render_sidebar_partner_section(app, ui, max_partner_h);
            });
    }

    // 3. Coluna Direita: Abas Chat | Sala + sino + botao recolher + Card do Perfil com Pontos
    if !right_collapsed {
        egui::SidePanel::right("chat_panel")
            .resizable(true)
            .default_width(400.0) // DEFAULT_CHAT_WIDTH do site
            .min_width(260.0)
            .max_width(720.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_MAIN)
                    .inner_margin(egui::Margin { left: 0.0, right: 12.0, top: 12.0, bottom: 12.0 }),
            )
            .show(ctx, |ui| {
                // Barra superior: [ Chat | Sala ] + [Sino mudo] + [PanelRightClose apenas com midia]
                ui.horizontal(|ui| {
                    let right_btns_w = if has_media { 84.0 } else { 42.0 };
                    let pill_w = (ui.available_width() - right_btns_w - 6.0).max(130.0);

                    egui::Frame::none()
                        .fill(BG_ELEVATED) // rounded-xl bg-zinc-900 p-1
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(20.0))
                        .inner_margin(egui::Margin::same(4.0))
                        .show(ui, |ui| {
                            ui.set_width(pill_w - 8.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let tab_w = ((pill_w - 12.0) * 0.5).max(52.0);
                                let chat_sel = !app.right_tab_room;

                                let chat_tab = egui::Button::new(
                                    RichText::new("Chat")
                                        .size(14.0)
                                        .semibold()
                                        .color(if chat_sel { Color32::WHITE } else { TEXT_SECONDARY }),
                                )
                                .fill(if chat_sel { BORDER_SUBTLE } else { Color32::TRANSPARENT })
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(14.0))
                                .min_size(Vec2::new(tab_w, 32.0));
                                if ui.add(chat_tab).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    app.right_tab_room = false;
                                }

                                let room_tab = egui::Button::new(
                                    RichText::new("Sala")
                                        .size(14.0)
                                        .semibold()
                                        .color(if !chat_sel { Color32::WHITE } else { TEXT_SECONDARY }),
                                )
                                .fill(if !chat_sel { BORDER_SUBTLE } else { Color32::TRANSPARENT })
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(14.0))
                                .min_size(Vec2::new(tab_w, 32.0));
                                if ui.add(room_tab).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    app.right_tab_room = true;
                                }
                            });
                        });

                    let mute_bell = icons::icon_button(
                        ui,
                        Vec2::splat(36.0),
                        Color32::TRANSPARENT,
                        Stroke::NONE,
                        Rounding::same(14.0),
                        if app.chat_notifications_muted {
                            "Reativar avisos do chat"
                        } else {
                            "Silenciar avisos do chat"
                        },
                        |p, c, col| {
                            icons::paint_bell(p, c, 13.5, col, app.chat_notifications_muted);
                        },
                        if app.chat_notifications_muted { RED } else { TEXT_MUTED },
                    );
                    if mute_bell.clicked() {
                        app.chat_notifications_muted = !app.chat_notifications_muted;
                    }

                    if has_media {
                        let collapse_r = icons::icon_button(
                            ui,
                            Vec2::splat(36.0),
                            Color32::TRANSPARENT,
                            Stroke::NONE,
                            Rounding::same(14.0),
                            "Ocultar chat e perfil",
                            |p, c, col| icons::paint_panel_right_close(p, c, 14.0, col),
                            TEXT_MUTED,
                        );
                        if collapse_r.clicked() {
                            app.right_sidebar_collapsed = true;
                        }
                    }
                });

                ui.add_space(6.0);
                let profile_h = 54.0;
                let body_h = (ui.available_height() - profile_h - 8.0).max(140.0);

                ui.allocate_ui(Vec2::new(ui.available_width(), body_h), |ui| {
                    ui.set_min_size(Vec2::new(ui.available_width(), body_h));
                    if app.right_tab_room {
                        render_room_info_tab(app, ui, ctx);
                    } else {
                        let typing_names: Vec<String> = app
                            .typing_peer_ids
                            .iter()
                            .filter_map(|id| {
                                app.peers
                                    .iter()
                                    .find(|p| &p.id == id)
                                    .map(|p| p.name.clone())
                            })
                            .collect();

                        let can_tools = app.is_room_manager() || app.room_permissions.tools;
                        let self_uid_clone = app.self_user_id.clone();
                        let self_id_clone = app.self_id.clone();
                        let is_mgr = app.is_room_manager();
                        #[cfg(feature = "mod_ferramentas")]
                        let act = {
                            let has_tools_mod = app.has_modification(MOD_FERRAMENTAS);
                            let tools_opt = if has_tools_mod {
                                app.room_tools.as_deref_mut()
                            } else {
                                None
                            };
                            render_chat_panel(
                                ui,
                                &app.chat_messages,
                                &typing_names,
                                app.chat_blocked_message.as_deref(),
                                &mut app.chat_draft,
                                &mut app.chat_reply_to,
                                &mut app.chat_was_typing,
                                self_id_clone.as_deref(),
                                tools_opt,
                                self_uid_clone.as_deref(),
                                is_mgr,
                                can_tools,
                            )
                        };
                        #[cfg(not(feature = "mod_ferramentas"))]
                        let act = render_chat_panel(
                            ui,
                            &app.chat_messages,
                            &typing_names,
                            app.chat_blocked_message.as_deref(),
                            &mut app.chat_draft,
                            &mut app.chat_reply_to,
                            &mut app.chat_was_typing,
                            self_id_clone.as_deref(),
                            None,
                            self_uid_clone.as_deref(),
                            is_mgr,
                            can_tools,
                        );

                        if act.clear_reply {
                            app.chat_reply_to = None;
                        }
                        if let Some(typing) = act.typing_changed {
                            app.send_typing(typing);
                        }
                        if let Some(text) = act.send_text {
                            app.send_chat(text, act.reply_to);
                            app.chat_reply_to = None;
                        }
                        #[cfg(feature = "mod_ferramentas")]
                        {
                            if let Some((tid, opts)) = act.poll_vote {
                                app.send_tool_json(crate::net::room_tools::cmd_poll_vote(&tid, &opts));
                            }
                            if let Some(tid) = act.poll_close {
                                app.send_tool_json(crate::net::room_tools::cmd_poll_close(&tid));
                            }
                            if let Some((question, options, multi)) = act.create_poll {
                                app.send_tool_json(crate::net::room_tools::cmd_create_tool(
                                    "poll",
                                    serde_json::json!({
                                        "title": "Enquete",
                                        "access": "everyone",
                                        "poll": {
                                            "question": question,
                                            "options": options,
                                            "multi": multi,
                                            "anonymous": false,
                                            "announce": true,
                                            "durationMinutes": serde_json::Value::Null,
                                        }
                                    }),
                                ));
                            }
                            for cmd in act.tool_cmds {
                                app.send_tool_json(cmd);
                            }
                        }
                    }
                });

                ui.add_space(6.0);
                render_user_profile_bar(app, ui);
            });
    }

    // 4. Area Central (UNICO CentralPanel da janela)
    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(BG_MAIN)
                .inner_margin(egui::Margin::same(12.0)), // lg:p-3 lg:gap-3
        )
        .show(ctx, |ui| {
            let dock_h = 54.0; // cartao da barra: botao 40 + p-1.5 + borda
            let stage_h = (ui.available_height() - dock_h - 8.0).max(160.0);

            ui.allocate_ui(Vec2::new(ui.available_width(), stage_h), |ui| {
                ui.set_min_size(Vec2::new(ui.available_width(), stage_h));
                let stage_bounds = ui.available_rect_before_wrap();
                app.stage_rect = Some(stage_bounds);
                if let Some(ref music) = app.room_music {
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Musica na sala:")
                                        .semibold()
                                        .color(EMERALD),
                                );
                                ui.label(
                                    RichText::new(
                                        music
                                            .title
                                            .as_deref()
                                            .or(music.video_id.as_deref())
                                            .unwrap_or("Tocando"),
                                    )
                                    .color(TEXT_PRIMARY),
                                );
                                ui.label(
                                    RichText::new(if music.playing {
                                        "(Reproduzindo)"
                                    } else {
                                        "(Pausada)"
                                    })
                                    .size(11.5)
                                    .color(TEXT_SECONDARY),
                                );
                            });
                        });
                    ui.add_space(6.0);
                }

                if tiles.is_empty() {
                    render_empty_stage_card(app, ui, ctx);
                } else {
                    render_tile_stage_or_grid(app, ui, &tiles);
                }

                #[cfg(feature = "mod_ferramentas")]
                if app.has_modification(MOD_FERRAMENTAS) {
                    render_hands_queue_overlay(app, ctx, stage_bounds, left_collapsed);
                }

                // Botoes flutuantes de reabrir as barras laterais quando recolhidas (somente quando ha midia ativa)
                if left_collapsed {
                    let lr = Rect::from_min_size(
                        pos2(stage_bounds.left() + 8.0, stage_bounds.top() + 8.0),
                        Vec2::splat(30.0),
                    );
                    let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(lr));
                    if icons::icon_button(
                        &mut lui,
                        Vec2::splat(30.0),
                        BG_SURFACE,
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                        Rounding::same(8.0),
                        "Mostrar participantes",
                        |p, c, col| icons::paint_panel_left_open(p, c, 15.0, col),
                        TEXT_PRIMARY,
                    )
                    .clicked()
                    {
                        app.left_sidebar_collapsed = false;
                    }
                }
                if right_collapsed {
                    let rr = Rect::from_min_size(
                        pos2(stage_bounds.right() - 38.0, stage_bounds.top() + 8.0),
                        Vec2::splat(30.0),
                    );
                    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(rr));
                    if icons::icon_button(
                        &mut rui,
                        Vec2::splat(30.0),
                        BG_SURFACE,
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                        Rounding::same(8.0),
                        "Mostrar chat e perfil",
                        |p, c, col| icons::paint_panel_right_open(p, c, 15.0, col),
                        TEXT_PRIMARY,
                    )
                    .clicked()
                    {
                        app.right_sidebar_collapsed = false;
                    }
                }
            });

            ui.add_space(8.0);
            let mut stop_screens_anchor: Option<Rect> = None;
            render_bottom_call_dock(app, ui, &mut stop_screens_anchor);
            if let Some(anchor) = stop_screens_anchor {
                render_stop_screens_popover(app, ctx, anchor);
            }
        });

    render_participant_context_menu(app, ctx, participant_menu_opened_this_frame);
}

fn render_participant_context_menu(
    app: &mut GoLiveLiteApp,
    ctx: &egui::Context,
    opened_this_frame: bool,
) {
    let Some(target_id) = app.participant_menu_peer_id.clone() else {
        return;
    };
    let is_self = target_id == "__self__";
    let show_profile_mod = app.has_modification(MOD_PERFIL);
    let show_perms_mod = app.has_modification(MOD_PERMISSOES_CHAMADA);

    let (
        display_name,
        user_id,
        is_guest,
        flags,
        vkey,
        av_url,
        raw_name,
        is_app,
        silenced,
        is_owner,
        is_admin,
        name_color,
    ) = if is_self {
        let uid = app.self_user_id.clone();
        (
            app.effective_name().to_string(),
            uid.clone(),
            Some(app.logged_account.is_none()),
            app.effective_self_flags().to_vec(),
            String::new(),
            app.effective_self_avatar_url().map(|s| s.to_string()),
            app.effective_name().to_string(),
            true,
            app.is_peer_silenced(uid.as_deref()),
            app.is_peer_owner(uid.as_deref()),
            app.is_peer_admin(uid.as_deref()),
            app.effective_self_name_color().map(|s| s.to_string()),
        )
    } else if let Some(peer) = app.peers.iter().find(|p| p.id == target_id) {
        let p_uid = peer.user_id.clone();
        (
            peer.display_label(),
            p_uid.clone(),
            peer.is_guest,
            peer.flags.clone().unwrap_or_default(),
            peer.volume_key().to_string(),
            peer.avatar_url.clone(),
            peer.name.clone(),
            peer.app.unwrap_or(false),
            app.is_peer_silenced(p_uid.as_deref()),
            app.is_peer_owner(p_uid.as_deref()),
            app.is_peer_admin(p_uid.as_deref()),
            peer.name_color.clone(),
        )
    } else {
        app.participant_menu_peer_id = None;
        app.participant_menu_pos = None;
        return;
    };

    if is_self && !show_profile_mod {
        app.participant_menu_peer_id = None;
        app.participant_menu_pos = None;
        return;
    }

    let av_tex = app.resolve_and_load_avatar(av_url.as_deref());
    let screen = ctx.screen_rect();
    let menu_w = 236.0;
    let est_h = if is_self {
        92.0
    } else if show_perms_mod && app.is_room_manager() && !is_owner && user_id.is_some() {
        265.0
    } else {
        175.0
    };
    let raw_pos = app
        .participant_menu_pos
        .unwrap_or_else(|| pos2(screen.left() + 310.0, screen.top() + 120.0));
    let clamped_pos = pos2(
        raw_pos.x.clamp(screen.left() + 8.0, (screen.right() - menu_w - 8.0).max(8.0)),
        raw_pos.y.clamp(screen.top() + 8.0, (screen.bottom() - est_h - 8.0).max(8.0)),
    );

    let area_resp = egui::Area::new(egui::Id::new("golive_participant_ctx_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(clamped_pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgb(22, 27, 34))
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(20.0))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_width(menu_w - 20.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;

                        // Cabecalho: Avatar + Nome + Selo + Coroa
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            let (av_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                            if let Some(tid) = av_tex {
                                ui.painter().image(
                                    tid,
                                    av_rect,
                                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                                ui.painter().circle_stroke(
                                    av_rect.center(),
                                    13.0,
                                    Stroke::new(1.0_f32, BORDER_SUBTLE),
                                );
                            } else {
                                let initial = display_name
                                    .chars()
                                    .next()
                                    .unwrap_or('?')
                                    .to_uppercase()
                                    .to_string();
                                ui.painter().circle_filled(
                                    av_rect.center(),
                                    13.0,
                                    avatar_color_for(&display_name),
                                );
                                ui.painter().text(
                                    av_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    initial,
                                    egui::FontId::proportional(11.5),
                                    Color32::WHITE,
                                );
                            }
                            if is_app {
                                let badge_c = pos2(av_rect.right() - 2.0, av_rect.bottom() - 2.0);
                                ui.painter().circle_filled(badge_c, 4.0, BG_MAIN);
                                ui.painter().circle_filled(badge_c, 2.8, EMERALD_BRIGHT);
                            }

                            let n_col = resolve_name_color_rgb(name_color.as_deref())
                                .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                                .unwrap_or(TEXT_PRIMARY);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&display_name)
                                        .size(13.5)
                                        .semibold()
                                        .color(n_col),
                                )
                                .truncate(),
                            );
                            if let Some(tone) = verified_badge_tone(&flags) {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                                icons::paint_verified_seal(ui.painter(), r.center(), 6.0, tone);
                            }
                            if is_owner {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                                icons::paint_crown(ui.painter(), r.center(), 11.5, GOLD);
                            } else if is_admin {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                icons::paint_crown(ui.painter(), r.center(), 10.5, TEXT_MUTED);
                            }
                        });

                        // Ver perfil (apenas com modificacao `perfil`)
                        #[cfg(feature = "mod_perfil")]
                        if show_profile_mod
                            && render_participant_menu_btn(
                                ui,
                                "Ver perfil",
                                TEXT_PRIMARY,
                                Some(icons::paint_user_outline),
                            )
                        {
                            app.participant_menu_peer_id = None;
                            app.participant_menu_pos = None;
                            if is_self {
                                app.open_self_profile();
                            } else if let Some(ref uid) = user_id {
                                if !uid.starts_with("guest:") && is_guest != Some(true) {
                                    app.open_profile_account(uid.clone());
                                } else {
                                    app.open_profile_guest(raw_name.clone(), av_url.clone());
                                }
                            } else {
                                app.open_profile_guest(raw_name.clone(), av_url.clone());
                            }
                        }

                        // Volume do usuario + Silenciar para mim
                        if !is_self {
                            ui.separator();
                            ui.label(
                                RichText::new("VOLUME DO USUARIO")
                                    .size(10.5)
                                    .semibold()
                                    .color(TEXT_MUTED),
                            );

                            let vol = app.config.get_peer_volume(&vkey);
                            let mut muted = app.config.is_peer_muted(&vkey);
                            ui.horizontal(|ui| {
                                let (sp_rect, sp_resp) =
                                    ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                                icons::paint_speaker(
                                    ui.painter(),
                                    sp_rect.center(),
                                    13.0,
                                    if muted || vol <= 0.001 {
                                        RED
                                    } else {
                                        TEXT_SECONDARY
                                    },
                                    muted || vol <= 0.001,
                                );
                                if sp_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    muted = !muted;
                                    app.config.set_peer_muted(&vkey, muted);
                                    app.mixer.set_source_muted(&target_id, muted);
                                    app.config.save();
                                }

                                let mut vol_pct = vol * 100.0;
                                let slider = ui.add(
                                    egui::Slider::new(&mut vol_pct, 0.0..=200.0)
                                        .show_value(false)
                                        .step_by(1.0),
                                );
                                if slider.double_clicked() {
                                    app.config.set_peer_volume(&vkey, 1.0);
                                    app.mixer.set_source_volume(&target_id, 1.0);
                                    app.config.save();
                                } else if slider.changed() {
                                    let new_v = vol_pct / 100.0;
                                    app.config.set_peer_volume(&vkey, new_v);
                                    app.mixer.set_source_volume(&target_id, new_v);
                                } else if slider.drag_stopped() {
                                    app.config.save();
                                }

                                ui.label(
                                    RichText::new(format!("{:.0}%", vol_pct))
                                        .size(11.0)
                                        .color(TEXT_SECONDARY),
                                );
                            });

                            let mut mute_chk = muted;
                            if ui.checkbox(&mut mute_chk, "Silenciar para mim").changed() {
                                app.config.set_peer_muted(&vkey, mute_chk);
                                app.mixer.set_source_muted(&target_id, mute_chk);
                                app.config.save();
                            }

                            // Moderacao na chamada (apenas com mod `permissoes_chamada` e gerente)
                            if show_perms_mod && app.is_room_manager() && !is_owner {
                                if let Some(ref uid) = user_id {
                                    ui.separator();
                                    ui.label(
                                        RichText::new("MODERACAO")
                                            .size(10.5)
                                            .semibold()
                                            .color(TEXT_MUTED),
                                    );
                                    let sil_lbl = if silenced {
                                        "Dessilenciar na sala"
                                    } else {
                                        "Silenciar na sala"
                                    };
                                    if render_participant_menu_btn(
                                        ui,
                                        sil_lbl,
                                        TEXT_PRIMARY,
                                        None::<fn(&egui::Painter, egui::Pos2, f32, Color32)>,
                                    ) {
                                        app.room_silence_peer(uid, !silenced);
                                    }
                                    if render_participant_menu_btn(
                                        ui,
                                        "Expulsar",
                                        AMBER,
                                        None::<fn(&egui::Painter, egui::Pos2, f32, Color32)>,
                                    ) {
                                        app.room_kick_peer(uid);
                                        app.participant_menu_peer_id = None;
                                        app.participant_menu_pos = None;
                                    }
                                    if render_participant_menu_btn(
                                        ui,
                                        "Banir",
                                        Color32::from_rgb(252, 165, 165),
                                        None::<fn(&egui::Painter, egui::Pos2, f32, Color32)>,
                                    ) {
                                        app.room_ban_peer(uid);
                                        app.participant_menu_peer_id = None;
                                        app.participant_menu_pos = None;
                                    }
                                }
                            }
                        }
                    });
                });
        });

    if !opened_this_frame {
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let outside = ctx.input(|i| {
            i.pointer.primary_clicked()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !area_resp.response.rect.contains(p))
        });
        if esc || outside {
            app.participant_menu_peer_id = None;
            app.participant_menu_pos = None;
        }
    }
}

fn render_participant_menu_btn<F>(
    ui: &mut egui::Ui,
    label: &str,
    color: Color32,
    icon_fn: Option<F>,
) -> bool
where
    F: FnOnce(&egui::Painter, egui::Pos2, f32, Color32),
{
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, Rounding::same(7.0), BG_ELEVATED);
    }
    if let Some(paint) = icon_fn {
        paint(
            ui.painter(),
            pos2(rect.left() + 12.0, rect.center().y),
            13.0,
            TEXT_SECONDARY,
        );
        ui.painter().text(
            pos2(rect.left() + 26.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.5),
            color,
        );
    } else {
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.5),
            color,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// zinc-100 / zinc-300 da sala nova (texto claro sobre os cartoes).
const ZINC_100: Color32 = Color32::from_rgb(243, 244, 246);
const ZINC_300: Color32 = Color32::from_rgb(210, 212, 218);
const ZINC_50: Color32 = Color32::from_rgb(249, 250, 251);

/// Palco vazio da sala nova (WatchRoom `data-rr-empty`): cartao `rounded-xl`, titulo `text-lg
/// font-semibold`, bloco `max-w-md` com o botao verde `h-12` e a grade 2x2 de `h-11`, e o
/// "Convidar pessoas" depois de um divisor.
fn render_empty_stage_card(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    egui::Frame::none()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(16.0, 0.0))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let block_w = 448.0_f32.min(ui.available_width());
            // Titulo + botoes + convite ~ 300px; o resto fica dividido em cima e embaixo.
            let top_pad = ((ui.available_height() - 300.0) * 0.5).max(16.0);
            ui.vertical_centered(|ui| {
                ui.add_space(top_pad);
                ui.label(
                    RichText::new("Ninguém está transmitindo ainda.")
                        .size(18.0)
                        .semibold()
                        .color(ZINC_100),
                );
                ui.add_space(8.0 + 12.0);

                // Botao principal: h-12 rounded-xl bg-emerald-600 text-base font-semibold.
                let (sh_rect, sh_resp) =
                    ui.allocate_exact_size(Vec2::new(block_w, 48.0), Sense::click());
                ui.painter().rect_filled(
                    sh_rect,
                    Rounding::same(20.0),
                    if sh_resp.hovered() { Color32::from_rgb(4, 120, 87) } else { EMERALD },
                );
                paint_icon_label_centered(
                    ui,
                    sh_rect,
                    "Compartilhar tela",
                    16.0,
                    Color32::WHITE,
                    |p, c, col| icons::paint_screen(p, c, 20.0, col),
                );
                if sh_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    app.open_share_picker();
                }

                // Grade 2 colunas gap-2; o ultimo item sozinho ocupa as duas.
                #[derive(Clone, Copy)]
                enum Act {
                    Camera,
                    VideoSource,
                    Music,
                    #[cfg(feature = "mod_ferramentas")]
                    Tools,
                }
                #[allow(unused_mut)]
                let mut items: Vec<(Act, &str)> = vec![
                    (Act::Camera, "Compartilhar câmera"),
                    (Act::VideoSource, "Adicionar fonte de vídeo"),
                    (Act::Music, "Colocar música na sala"),
                ];
                #[cfg(feature = "mod_ferramentas")]
                if app.has_modification(MOD_FERRAMENTAS)
                    && (app.is_room_manager() || app.room_permissions.tools)
                {
                    items.push((Act::Tools, "Ferramentas"));
                }
                let half_w = (block_w - 8.0) * 0.5;
                let mut clicked: Option<Act> = None;
                for row in items.chunks(2) {
                    ui.add_space(8.0);
                    ui.allocate_ui(Vec2::new(block_w, 44.0), |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            for &(act, label) in row {
                                let w = if row.len() == 1 { block_w } else { half_w };
                                let resp = stage_secondary_btn(ui, Vec2::new(w, 44.0), label, |p, c, col| match act {
                                    Act::Camera => icons::paint_camera(p, c, 20.0, col),
                                    Act::VideoSource => icons::paint_video_source(p, c, 20.0, col),
                                    Act::Music => icons::paint_music(p, c, 20.0, col),
                                    #[cfg(feature = "mod_ferramentas")]
                                    Act::Tools => icons::paint_tools(p, c, 20.0, col),
                                });
                                if resp.clicked() {
                                    clicked = Some(act);
                                }
                            }
                        });
                    });
                }
                match clicked {
                    Some(Act::Camera) => app.toggle_camera(),
                    // Fonte de video e musica ainda nao existem no Lite: abrem a sala no site.
                    Some(Act::VideoSource) | Some(Act::Music) => {
                        crate::ui::external_link::request(&app.room_invite_url())
                    }
                    #[cfg(feature = "mod_ferramentas")]
                    Some(Act::Tools) => {
                        if let Some(ref mut rt) = app.room_tools {
                            rt.panel_open = true;
                        }
                    }
                    None => {}
                }

                // Convidar pessoas: mt-3 border-t pt-4, alinhado a esquerda.
                ui.add_space(12.0);
                let (div, _) = ui.allocate_exact_size(Vec2::new(block_w, 1.0), Sense::hover());
                ui.painter()
                    .hline(div.x_range(), div.center().y, Stroke::new(1.0_f32, BORDER_SUBTLE));
                ui.add_space(16.0);
                let invite_url = app.room_invite_url();
                let copied_now = app.room_link_copied_until_ms > now_epoch_ms();
                ui.allocate_ui_with_layout(Vec2::new(block_w, 0.0), Layout::top_down(Align::LEFT), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.label(RichText::new("Convidar pessoas").size(12.0).semibold().color(TEXT_SECONDARY));
                    egui::Frame::none()
                        .fill(BG_ELEVATED)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(20.0))
                        .inner_margin(egui::Margin { left: 12.0, right: 4.0, top: 4.0, bottom: 4.0 })
                        .show(ui, |ui| {
                            ui.set_width(block_w - 18.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let (lr, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                                icons::paint_link(ui.painter(), lr.center(), 14.0, TEXT_SECONDARY);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let (br, bresp) = ui.allocate_exact_size(
                                        Vec2::new(if copied_now { 100.0 } else { 74.0 }, 36.0),
                                        Sense::click(),
                                    );
                                    ui.painter().rect_filled(
                                        br,
                                        Rounding::same(14.0),
                                        if bresp.hovered() { ZINC_300 } else { ZINC_100 },
                                    );
                                    let dark = BG_ELEVATED;
                                    if copied_now {
                                        paint_icon_label_centered(ui, br, "Copiado", 14.0, dark, |p, c, col| {
                                            icons::paint_checkmark(p, c, 14.0, col)
                                        });
                                    } else {
                                        ui.painter().text(
                                            br.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "Copiar",
                                            egui::FontId::proportional(14.0),
                                            dark,
                                        );
                                    }
                                    if bresp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                        app.copy_room_invite_url(ctx);
                                    }
                                    ui.add(
                                        egui::Label::new(RichText::new(&invite_url).size(14.0).color(ZINC_300))
                                            .truncate(),
                                    );
                                });
                            });
                        });
                });
            });
        });
}

/// Icone de 20px + `gap-2` + texto, centralizados juntos no retangulo (como `justify-center gap-2`).
fn paint_icon_label_centered(
    ui: &egui::Ui,
    rect: Rect,
    label: &str,
    size: f32,
    color: Color32,
    paint_icon: impl FnOnce(&egui::Painter, egui::Pos2, Color32),
) {
    let galley =
        ui.fonts(|f| f.layout_no_wrap(label.to_string(), egui::FontId::proportional(size), color));
    let icon_w = 20.0;
    let total = icon_w + 8.0 + galley.size().x;
    let left = rect.center().x - total * 0.5;
    paint_icon(ui.painter(), pos2(left + icon_w * 0.5, rect.center().y), color);
    ui.painter().galley(
        pos2(left + icon_w + 8.0, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

/// Botao secundario do palco vazio: `h-11 rounded-xl border-zinc-800 bg-zinc-900 text-sm
/// font-semibold text-zinc-100 hover:bg-zinc-800`.
fn stage_secondary_btn(
    ui: &mut egui::Ui,
    size: Vec2,
    label: &str,
    paint_icon: impl FnOnce(&egui::Painter, egui::Pos2, Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect(
        rect,
        Rounding::same(20.0),
        if resp.hovered() { BORDER_SUBTLE } else { BG_ELEVATED },
        Stroke::new(1.0_f32, BORDER_SUBTLE),
    );
    paint_icon_label_centered(ui, rect, label, 14.0, ZINC_100, paint_icon);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn render_bottom_call_dock(
    app: &mut GoLiveLiteApp,
    ui: &mut egui::Ui,
    stop_screens_anchor: &mut Option<Rect>,
) {
    // Sala nova do site (dockInStage): um cartao da largura do palco, `rounded-xl p-1.5`,
    // botoes neutros (zinc-800) quando esta tudo bem e vermelhos so no que esta desligado/ao vivo.
    {
        egui::Frame::none()
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(20.0))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Centraliza a fileira: largura medida no quadro anterior.
                let row_id = egui::Id::new("dock_row_w");
                let prev_w: f32 = ui.ctx().data(|d| d.get_temp(row_id)).unwrap_or(0.0);
                let pad = ((ui.available_width() - prev_w) * 0.5).max(0.0);
                {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.add_space(pad);
                    let row_start = ui.cursor().left();

                    // 1. Microfone: [v | mic]
                    let mic_on = app.mic.state.enabled.load(Ordering::Relaxed);
                    let (mic_arrow, mic_main) = render_split_dock_pill(
                        ui,
                        dock_tone(mic_on),
                        Stroke::NONE,
                        "Ajustes do microfone",
                        if mic_on { "Silenciar microfone" } else { "Ativar microfone" },
                        |p, c| icons::paint_chevron_down(p, c, 10.0, Color32::WHITE),
                        |p, c| icons::paint_mic(p, c, 18.0, Color32::WHITE, !mic_on),
                        true,
                    );
                    if mic_arrow {
                        app.show_mic_popover = !app.show_mic_popover;
                        app.show_speaker_popover = false;
                        if app.show_mic_popover {
                            app.cached_mic_devices = MicCapture::list_input_devices();
                        }
                    }
                    if mic_main {
                        app.toggle_mic();
                    }

                    // 2. Som da sala: [v | fone]
                    let deaf = app.mixer.deafened.load(Ordering::Relaxed);
                    let (aud_arrow, aud_main) = render_split_dock_pill(
                        ui,
                        dock_tone(!deaf),
                        Stroke::NONE,
                        "Dispositivo de saída",
                        if deaf { "Ativar o som da sala" } else { "Desativar o som da sala" },
                        |p, c| icons::paint_chevron_down(p, c, 10.0, Color32::WHITE),
                        |p, c| icons::paint_headphones(p, c, 18.0, Color32::WHITE, deaf),
                        true,
                    );
                    if aud_arrow {
                        app.show_speaker_popover = !app.show_speaker_popover;
                        app.show_mic_popover = false;
                        if app.show_speaker_popover {
                            app.cached_speaker_devices = AudioMixer::list_output_devices();
                        }
                    }
                    if aud_main {
                        app.toggle_deafen();
                    }

                    dock_divider(ui);

                    // 3. Tela: verde para comecar, vermelho enquanto transmite.
                    let active_screens = app.active_screen_slots.len();
                    if active_screens > 0 {
                        let (pill_rect, seg_stop, seg_qual, seg_add) =
                            render_active_screen_triple_pill(ui, active_screens, app.current_plan_tier().max_screens());
                        if seg_stop {
                            app.show_stop_screens_popover = !app.show_stop_screens_popover;
                        }
                        if app.show_stop_screens_popover {
                            *stop_screens_anchor = Some(pill_rect);
                        }
                        if seg_qual {
                            app.show_stop_screens_popover = false;
                            app.show_stream_quality_modal = !app.show_stream_quality_modal;
                        }
                        if seg_add {
                            app.open_share_picker();
                        }
                    } else {
                        app.show_stop_screens_popover = false;
                        let (scr_main, scr_arrow) = render_split_dock_pill(
                            ui,
                            DOCK_NEUTRAL,
                            Stroke::NONE,
                            "Compartilhar tela ou janela",
                            "Qualidade da transmissão",
                            |p, c| icons::paint_screen(p, c, 18.0, Color32::WHITE),
                            |p, c| icons::paint_chevron_down(p, c, 10.0, Color32::WHITE),
                            false,
                        );
                        if scr_main {
                            app.open_share_picker();
                        }
                        if scr_arrow {
                            app.show_stream_quality_modal = !app.show_stream_quality_modal;
                        }
                    }

                    // 4. Camera: [v | cam] — verde para ligar, vermelho ao vivo.
                    let cam_on = app.camera_active;
                    let (cam_arrow, cam_main) = render_split_dock_pill(
                        ui,
                        if cam_on { RED } else { DOCK_NEUTRAL },
                        Stroke::NONE,
                        "Qualidade de vídeo",
                        if cam_on { "Parar câmera" } else { "Compartilhar câmera" },
                        |p, c| icons::paint_chevron_down(p, c, 10.0, Color32::WHITE),
                        |p, c| icons::paint_camera(p, c, 18.0, Color32::WHITE),
                        true,
                    );
                    if cam_arrow {
                        app.show_stream_quality_modal = !app.show_stream_quality_modal;
                    }
                    if cam_main {
                        app.toggle_camera();
                    }

                    dock_divider(ui);

                    // 5. Botao Previa / Modo Eco (Olho)
                    let preview_on = app.config.grid_video_preview;
                    let eye_resp = icons::icon_button(
                        ui,
                        Vec2::new(44.0, DOCK_H),
                        DOCK_NEUTRAL,
                        Stroke::NONE,
                        Rounding::same(DOCK_ROUND),
                        if preview_on {
                            "Previa de videos na grade ativa (clique para Modo Eco)"
                        } else {
                            "Modo Eco ativo (clique para ativar previa de videos na grade)"
                        },
                        |p, c, col| icons::paint_eye(p, c, 18.0, col, preview_on),
                        if preview_on { EMERALD_BRIGHT } else { TEXT_SECONDARY },
                    );
                    if eye_resp.clicked() {
                        let next = !preview_on;
                        app.config.grid_video_preview = next;
                        if !next {
                            app.enabled_grid_preview_tiles.clear();
                        }
                        app.config.save();
                    }

                    dock_divider(ui);

                    #[cfg(feature = "mod_ferramentas")]
                    // 6. Botao Ferramentas + Levantar a mao (somente quando a modificacao `ferramentas` esta instalada)
                    if app.has_modification(MOD_FERRAMENTAS) {
                        let tools_on = app.room_tools.as_ref().is_some_and(|rt| rt.panel_open);
                        // Igual ao site: `canOpenRoomTools` (permissao `tools` ou admin) e o contador
                        // de ferramentas abertas (reacoes nao contam).
                        let can_open_tools = app.is_room_manager() || app.room_permissions.tools;
                        let tool_count = app.room_tools.as_ref().map_or(0, |rt| {
                            rt.tools.iter().filter(|t| t.kind != "reactions").count()
                        });
                        if can_open_tools {
                        let tools_resp = icons::icon_button(
                            ui,
                            Vec2::new(44.0, DOCK_H),
                            if tools_on { BORDER_STRONG } else { DOCK_NEUTRAL },
                            Stroke::NONE,
                            Rounding::same(DOCK_ROUND),
                            "Ferramentas da sala",
                            |p, c, col| icons::paint_tools(p, c, 18.0, col),
                            Color32::WHITE,
                        );
                        if tool_count > 0 {
                            let c = pos2(tools_resp.rect.right() - 3.0, tools_resp.rect.top() + 3.0);
                            let txt = tool_count.min(99).to_string();
                            let w = if txt.len() > 1 { 18.0 } else { 14.0 };
                            let badge = Rect::from_center_size(c, Vec2::new(w, 14.0));
                            ui.painter().rect(
                                badge,
                                Rounding::same(7.0),
                                Color32::WHITE,
                                Stroke::new(1.0_f32, EMERALD),
                            );
                            ui.painter().text(
                                c,
                                egui::Align2::CENTER_CENTER,
                                txt,
                                egui::FontId::proportional(9.5),
                                Color32::from_rgb(4, 120, 87),
                            );
                        }
                        if tools_resp.clicked() {
                            if let Some(ref mut rt) = app.room_tools {
                                rt.panel_open = !rt.panel_open;
                            }
                        }
                        }

                        // `canRaiseHand` do site: levantar a mao e um pedido para falar, entao so
                        // aparece quando o microfone esta bloqueado para voce (e nao e admin).
                        let can_raise_hand = (app.self_user_id.is_some() || app.self_id.is_some())
                            && !app.is_room_manager()
                            && !app.room_permissions.mic;
                        if can_raise_hand {
                            let (i_raised, can_speak_granted) = app
                                .room_tools
                                .as_ref()
                                .map(|rt| {
                                    let raised = rt.hands.iter().any(|h| {
                                        app.self_user_id.as_deref() == Some(h.id.as_str())
                                            || app.self_id.as_deref() == Some(h.id.as_str())
                                    });
                                    let granted = rt.speakers.iter().any(|sid| {
                                        app.self_user_id.as_deref() == Some(sid.as_str())
                                            || app.self_id.as_deref() == Some(sid.as_str())
                                    });
                                    (raised, granted)
                                })
                                .unwrap_or((false, false));
                            let mics_locked = !app.room_permissions.mic && !app.is_room_manager();
                            let hand_tip = if i_raised {
                                "Abaixar a mão"
                            } else if mics_locked && can_speak_granted {
                                "Você foi liberado para falar — ligue o microfone"
                            } else {
                                "Levantar a mão"
                            };
                            let hand_bg = if i_raised {
                                AMBER
                            } else {
                                BORDER_SUBTLE
                            };
                            let hand_border = if i_raised {
                                AMBER
                            } else {
                                BORDER_SUBTLE
                            };
                            let hand_resp = icons::icon_button(
                                ui,
                                Vec2::new(44.0, DOCK_H),
                                hand_bg,
                                Stroke::new(1.0_f32, hand_border),
                                Rounding::same(DOCK_ROUND),
                                hand_tip,
                                |p, c, col| {
                                    icons::paint_hand_raise(p, c, 18.0, col);
                                    if mics_locked && can_speak_granted {
                                        let badge = pos2(c.x + 11.0, c.y - 9.0);
                                        p.circle_filled(badge, 5.5, EMERALD);
                                        icons::paint_mic(p, badge, 7.5, Color32::WHITE, false);
                                    }
                                },
                                if i_raised { Color32::WHITE } else { TEXT_PRIMARY },
                            );
                            if hand_resp.clicked() {
                                app.send_tool_json(crate::net::room_tools::cmd_hand_raise(!i_raised));
                            }
                        }
                    }

                    // 7. Botao Desligar (Vermelho) -> Sai da sala
                    let hangup_resp = icons::icon_button(
                        ui,
                        Vec2::new(44.0, DOCK_H),
                        RED,
                        Stroke::NONE,
                        Rounding::same(DOCK_ROUND),
                        "Sair da sala",
                        |p, c, col| icons::paint_hangup(p, c, 16.0, col),
                        Color32::WHITE,
                    );
                    if hangup_resp.clicked() {
                        app.leave_room();
                    }
                    let row_w = ui.min_rect().right() - row_start;
                    if (row_w - prev_w).abs() > 0.5 {
                        ui.ctx().data_mut(|d| d.insert_temp(row_id, row_w));
                        ui.ctx().request_repaint();
                    }
                });
                }
            });
    }
}

fn render_active_screen_triple_pill(
    ui: &mut egui::Ui,
    active_count: usize,
    max_screens: usize,
) -> (Rect, bool, bool, bool) {
    let pill_w = 104.0_f32;
    let pill_h = DOCK_H;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(pill_w, pill_h), Sense::hover());

    let fill = RED; // ao vivo: vermelho como no site (DOCK_LIVE)
    let border = RED;
    let divider_col = Color32::from_black_alpha(38);
    let icon_col = Color32::WHITE;

    ui.painter()
        .rect(rect, Rounding::same(DOCK_ROUND), fill, Stroke::new(1.0_f32, border));

    let x1 = rect.left() + 38.0;
    let x2 = rect.left() + 66.0;
    let r_left = Rect::from_min_max(rect.min, pos2(x1, rect.max.y));
    let r_mid = Rect::from_min_max(pos2(x1, rect.min.y), pos2(x2, rect.max.y));
    let r_right = Rect::from_min_max(pos2(x2, rect.min.y), rect.max);

    let resp_l = ui.interact(r_left, ui.make_persistent_id("dock_scr_stop_pick"), Sense::click());
    let resp_m = ui.interact(r_mid, ui.make_persistent_id("dock_scr_qual_btn"), Sense::click());
    let resp_r = ui.interact(r_right, ui.make_persistent_id("dock_scr_add_btn"), Sense::click());

    let hover_bg = Color32::from_black_alpha(40);
    if resp_l.hovered() {
        ui.painter().rect_filled(
            r_left,
            Rounding { nw: DOCK_ROUND, sw: DOCK_ROUND, ne: 0.0, se: 0.0 },
            hover_bg,
        );
    }
    if resp_m.hovered() {
        ui.painter().rect_filled(r_mid, Rounding::ZERO, hover_bg);
    }
    if resp_r.hovered() {
        ui.painter().rect_filled(
            r_right,
            Rounding { nw: 0.0, sw: 0.0, ne: DOCK_ROUND, se: DOCK_ROUND },
            hover_bg,
        );
    }

    ui.painter().line_segment(
        [pos2(x1, rect.top() + 5.0), pos2(x1, rect.bottom() - 5.0)],
        Stroke::new(1.0_f32, divider_col),
    );
    ui.painter().line_segment(
        [pos2(x2, rect.top() + 5.0), pos2(x2, rect.bottom() - 5.0)],
        Stroke::new(1.0_f32, divider_col),
    );

    icons::paint_screen(ui.painter(), r_left.center(), 15.5, icon_col);
    icons::paint_chevron_down(ui.painter(), r_mid.center(), 9.0, icon_col);
    icons::paint_plus(
        ui.painter(),
        r_right.center(),
        13.5,
        if active_count >= max_screens {
            TEXT_MUTED
        } else {
            icon_col
        },
    );

    let c_left = resp_l
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Escolher qual transmissao desligar")
        .clicked();
    let c_mid = resp_m
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Qualidade da transmissao")
        .clicked();
    let add_tip = if active_count >= max_screens {
        format!("Limite de {}/{} telas atingido", active_count, max_screens)
    } else {
        "Adicionar outra tela ou janela".to_string()
    };
    let c_right = resp_r
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(add_tip)
        .clicked();

    (rect, c_left, c_mid, c_right)
}

fn render_stop_screens_popover(app: &mut GoLiveLiteApp, ctx: &egui::Context, anchor: Rect) {
    if !app.show_stop_screens_popover || app.active_screen_slots.is_empty() {
        app.show_stop_screens_popover = false;
        return;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.show_stop_screens_popover = false;
        return;
    }

    let items: Vec<(String, String, u32, u32)> = app
        .active_screen_slots
        .iter()
        .map(|w| (w.slot.clone(), w.target_label.clone(), w.output_width, w.output_height))
        .collect();

    let pop_w = 278.0_f32;
    let est_h = (52.0 + items.len() as f32 * 38.0 + if items.len() > 1 { 36.0 } else { 0.0 }).min(280.0);
    let sr = ctx.screen_rect();
    let x = (anchor.center().x - pop_w * 0.5).clamp(sr.left() + 8.0, (sr.right() - pop_w - 8.0).max(sr.left() + 8.0));
    let y = (anchor.top() - est_h - 8.0).max(sr.top() + 8.0);

    let area_resp = egui::Area::new(egui::Id::new("stop_screens_popover"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(x, y))
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(20.0))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_width(pop_w - 20.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Parar transmissao")
                                .size(12.5)
                                .semibold()
                                .color(TEXT_PRIMARY),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{} ativa(s)", items.len()))
                                    .size(11.0)
                                    .color(EMERALD_BRIGHT),
                            );
                        });
                    });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    for (slot, label, w, h) in &items {
                        egui::Frame::none()
                            .fill(BG_ELEVATED)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(label)
                                                    .size(12.0)
                                                    .semibold()
                                                    .color(TEXT_PRIMARY),
                                            )
                                            .truncate(),
                                        );
                                        ui.label(
                                            RichText::new(format!("{} • {}x{}", slot, w, h))
                                                .size(10.5)
                                                .color(TEXT_MUTED),
                                        );
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("Parar")
                                                        .size(11.0)
                                                        .semibold()
                                                        .color(Color32::WHITE),
                                                )
                                                .fill(RED)
                                                .rounding(Rounding::same(6.0))
                                                .min_size(Vec2::new(56.0, 24.0)),
                                            )
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            app.stop_screen_slot(slot);
                                        }
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }

                    if items.len() > 1 {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Parar todas as transmissoes")
                                        .size(11.5)
                                        .semibold()
                                        .color(Color32::WHITE),
                                )
                                .fill(RED)
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(ui.available_width(), 28.0)),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            app.stop_all_screen_shares();
                            app.show_stop_screens_popover = false;
                        }
                    }
                });
        });

    if ctx.input(|i| i.pointer.primary_pressed()) {
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if !area_resp.response.rect.contains(pos) && !anchor.contains(pos) {
                app.show_stop_screens_popover = false;
            }
        }
    }
}

/// Altura dos botoes da barra de chamada do site (`p-2` + icone `h-5`).
const DOCK_H: f32 = 40.0; // [data-rr-calldock] button { min-height: 2.5rem }
/// Botao dividido [seta | icone] (`px-1` + `p-2`).
const DOCK_SPLIT_W: f32 = 62.0;
/// `rounded-lg`.
const DOCK_ROUND: f32 = 14.0; // rounded-lg da sala nova (0.875rem)

/// Sala nova: o "verde" da barra vira neutro (zinc-800); vermelho continua para desligado.
fn dock_tone(on: bool) -> Color32 {
    if on {
        DOCK_NEUTRAL
    } else {
        RED
    }
}

/// Botao "tudo certo" da barra: bg-zinc-800 (hover zinc-700), como [data-rr-calldock] .bg-emerald-600.
const DOCK_NEUTRAL: Color32 = BORDER_SUBTLE;

/// Separador `mx-0.5 h-6 w-px bg-zinc-700`.
fn dock_divider(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(5.0, 24.0), Sense::hover());
    ui.painter()
        .vline(r.center().x, r.y_range(), Stroke::new(1.0_f32, BORDER_STRONG));
}

#[allow(clippy::too_many_arguments)]
fn render_split_dock_pill(
    ui: &mut egui::Ui,
    fill: Color32,
    stroke: Stroke,
    arrow_tooltip: &str,
    main_tooltip: &str,
    paint_arrow: impl FnOnce(&egui::Painter, egui::Pos2),
    paint_main: impl FnOnce(&egui::Painter, egui::Pos2),
    narrow_left: bool,
) -> (bool, bool) {
    // Igual ao site: botao dividido `rounded-lg`, [seta px-1 | icone p-2] com 36px de altura,
    // separados por `border-black/15` e escurecendo no hover (emerald-700 / red-700).
    let (rect, _) = ui.allocate_exact_size(Vec2::new(DOCK_SPLIT_W, DOCK_H), Sense::hover());
    let split_x = if narrow_left { rect.right() - DOCK_H } else { rect.left() + DOCK_H };
    let left_rect = Rect::from_min_max(rect.min, pos2(split_x, rect.max.y));
    let right_rect = Rect::from_min_max(pos2(split_x, rect.min.y), rect.max);
    let r_left = ui.interact(left_rect, ui.make_persistent_id(arrow_tooltip), Sense::click());
    let r_right = ui.interact(right_rect, ui.make_persistent_id(main_tooltip), Sense::click());

    // Vermelho escurece (red-700); neutro clareia (zinc-700).
    let hover = if fill == RED { Color32::from_black_alpha(40) } else { Color32::from_white_alpha(18) };
    let p = ui.painter();
    p.rect(rect, Rounding::same(DOCK_ROUND), fill, stroke);
    if r_left.hovered() {
        p.rect_filled(left_rect, Rounding { nw: DOCK_ROUND, sw: DOCK_ROUND, ne: 0.0, se: 0.0 }, hover);
    }
    if r_right.hovered() {
        p.rect_filled(right_rect, Rounding { nw: 0.0, sw: 0.0, ne: DOCK_ROUND, se: DOCK_ROUND }, hover);
    }
    p.vline(split_x, rect.y_range(), Stroke::new(1.0_f32, Color32::from_black_alpha(38)));
    paint_arrow(p, left_rect.center());
    paint_main(p, right_rect.center());

    let left_clicked = r_left
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(arrow_tooltip)
        .clicked();
    let right_clicked = r_right
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(main_tooltip)
        .clicked();
    (left_clicked, right_clicked)
}

fn render_user_profile_bar(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    let display = app.effective_name().to_string();
    let sub = if let Some(ref acc) = app.logged_account {
        format!("@{}", acc.username)
    } else {
        "Convidado".to_string()
    };
    let pts = app.effective_points();
    let pts_str = format_points_br(pts);
    let flags = app.effective_self_flags().to_vec();
    let name_color = resolve_name_color_rgb(app.effective_self_name_color())
        .map(|[r, g, b]| Color32::from_rgb(r, g, b))
        .unwrap_or(TEXT_PRIMARY);
    let av_url = app.effective_self_avatar_url().map(|s| s.to_string());
    let av_tex = app.resolve_and_load_avatar(av_url.as_deref());
    let initial = display
        .chars()
        .next()
        .unwrap_or('U')
        .to_uppercase()
        .to_string();
    let avatar_bg = avatar_color_for(&display);

    egui::Frame::none()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // Avatar + pontinho online (clicavel para abrir proprio perfil apenas com mod `perfil`)
                let can_open_profile = app.has_modification(MOD_PERFIL);
                let (av_rect, av_resp) = ui.allocate_exact_size(
                    Vec2::splat(30.0),
                    if can_open_profile {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                if let Some(tid) = av_tex {
                    icons::paint_circular_avatar(
                        ui.painter(),
                        av_rect,
                        tid,
                        Stroke::new(1.0_f32, BORDER_SUBTLE),
                    );
                } else {
                    ui.painter().circle_filled(av_rect.center(), 15.0, avatar_bg);
                    ui.painter().text(
                        av_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        initial,
                        egui::FontId::proportional(13.0),
                        Color32::WHITE,
                    );
                }
                let dot_c = pos2(av_rect.right() - 2.0, av_rect.bottom() - 2.0);
                ui.painter().circle_filled(dot_c, 4.0, BG_SURFACE);
                ui.painter().circle_filled(dot_c, 2.8, EMERALD_BRIGHT);
                #[cfg(feature = "mod_perfil")]
                if can_open_profile
                    && av_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Ver meu perfil")
                        .clicked()
                {
                    app.open_self_profile();
                }

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let nresp = ui.add(
                            egui::Label::new(
                                RichText::new(&display)
                                    .size(12.0)
                                    .semibold()
                                    .color(name_color),
                            )
                            .truncate()
                            .sense(if can_open_profile {
                                Sense::click()
                            } else {
                                Sense::hover()
                            }),
                        );
                        #[cfg(feature = "mod_perfil")]
                        if can_open_profile
                            && nresp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text("Ver meu perfil")
                                .clicked()
                        {
                            app.open_self_profile();
                        }
                        #[cfg(not(feature = "mod_perfil"))]
                        let _ = nresp;
                        if let Some(tone) = verified_badge_tone(&flags) {
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                            icons::paint_verified_seal(ui.painter(), r.center(), 5.8, tone);
                        }
                    });
                    ui.label(RichText::new(sub).size(10.5).color(TEXT_MUTED));
                });

                let gear_resp = icons::icon_button(
                    ui,
                    Vec2::splat(26.0),
                    if app.show_user_settings_modal {
                        Color32::from_rgb(42, 49, 60)
                    } else {
                        BG_ELEVATED
                    },
                    Stroke::new(1.0_f32, BORDER_SUBTLE),
                    Rounding::same(8.0),
                    "Configuracoes (Conta e Audio)",
                    |p, c, col| icons::paint_gear(p, c, 13.0, col),
                    if app.show_user_settings_modal {
                        TEXT_PRIMARY
                    } else {
                        TEXT_SECONDARY
                    },
                );
                if gear_resp.clicked() {
                    app.user_settings_tab = UserSettingsTab::Account;
                    app.show_user_settings_modal = !app.show_user_settings_modal;
                }

                // Direita: [Moeda + Pontos]
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    ui.label(
                        RichText::new(pts_str)
                            .size(12.5)
                            .semibold()
                            .color(GOLD),
                    );
                    let (cr, cresp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    icons::paint_coin(ui.painter(), cr.center(), 12.0, GOLD);
                    cresp.on_hover_text("Seus pontos acumulados");
                });
            });
        });
}

fn render_room_info_tab(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let room = app.current_room.clone().unwrap_or_default();
    let is_priv = is_private_room_handle(&room);
    let visible_count = app
        .peers
        .iter()
        .filter(|p| !p.is_obs() && !p.is_silent_moderator())
        .count()
        + 1;
    let cat_lbl =
        room_category_label(app.room_category.as_deref()).unwrap_or("Sem categoria");

    egui::Frame::none()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(20.0))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.label(RichText::new("Detalhes da Sala").size(14.0).semibold().color(TEXT_PRIMARY));
            ui.separator();
            ui.add_space(4.0);

            ui.label(RichText::new("Identificador").size(11.0).color(TEXT_MUTED));
            ui.label(RichText::new(&room).size(13.0).semibold().color(TEXT_PRIMARY));
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!(
                    "{} • {} • {} participante(s)",
                    if is_priv { "Sala Privada" } else { "Sala Publica" },
                    cat_lbl,
                    visible_count
                ))
                .size(11.5)
                .color(TEXT_SECONDARY),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("sala_settings_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    crate::ui::app_options_modal::render_room_settings_panel(app, ui, ctx);
                });
        });
}

fn render_tile_stage_or_grid(
    app: &mut GoLiveLiteApp,
    ui: &mut egui::Ui,
    tiles: &[MediaTileEntry],
) {
    if let Some(ref open_id) = app.open_stats_tile_id {
        let still_visible = tiles
            .iter()
            .any(|t| &t.tile_id == open_id && !t.is_stopped_by_us && t.channel.is_some());
        if !still_visible {
            app.close_stats_overlay();
        }
    }

    let mut open_stats_anchor: Option<(MediaTileEntry, egui::Rect)> = None;

    let focused_idx = app
        .focused_tile_id
        .as_ref()
        .and_then(|fid| tiles.iter().position(|t| &t.tile_id == fid));

    if let Some(fidx) = focused_idx {
        let total = ui.available_size();
        let has_strip = tiles.len() > 1;
        let stage_h = if has_strip {
            (total.y - 135.0).max(200.0)
        } else {
            total.y.max(160.0)
        };
        let stage_size = Vec2::new(total.x.max(180.0), stage_h);
        let (stage_outer, _) = ui.allocate_exact_size(stage_size, Sense::hover());
        ui.painter()
            .rect_filled(stage_outer, Rounding::same(20.0), Color32::from_rgb(13, 17, 23));
        let stage_fit = if tiles[fidx].tool_id.is_some() {
            let w = stage_outer
                .width()
                .min(stage_outer.height() * (16.0 / 9.0))
                .max(180.0)
                .min(stage_outer.width());
            let h = (w * (9.0 / 16.0)).min(stage_outer.height());
            Rect::from_center_size(stage_outer.center(), Vec2::new(w, h))
        } else {
            stage_outer
        };
        let mut stage_ui = ui.new_child(egui::UiBuilder::new().max_rect(stage_fit));
        stage_ui.set_clip_rect(ui.clip_rect().intersect(stage_fit));
        render_single_tile_entry(
            app,
            &mut stage_ui,
            &tiles[fidx],
            stage_fit.size(),
            true,
            &mut open_stats_anchor,
        );

        if has_strip {
            ui.add_space(6.0);
            egui::ScrollArea::horizontal()
                .id_salt("focus_strip_scroll")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (idx, tile) in tiles.iter().enumerate() {
                            if idx == fidx {
                                continue;
                            }
                            let tsize = Vec2::new(210.0, 122.0);
                            #[cfg(feature = "mod_ferramentas")]
                            if let Some(ref tid) = tile.tool_id {
                                let (tr, _) = ui.allocate_exact_size(tsize, Sense::hover());
                                let mut tui =
                                    ui.new_child(egui::UiBuilder::new().max_rect(tr));
                                tui.set_clip_rect(ui.clip_rect().intersect(tr));
                                crate::ui::room_tools_modal::render_tool_thumbnail_tile(
                                    app,
                                    &mut tui,
                                    tid,
                                    tsize,
                                );
                                continue;
                            }
                            render_single_tile_entry(
                                app,
                                ui,
                                tile,
                                tsize,
                                false,
                                &mut open_stats_anchor,
                            );
                        }
                    });
                });
        }
    } else {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Midias na sala ({})", tiles.len()))
                    .size(12.0)
                    .semibold()
                    .color(TEXT_SECONDARY),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let grid_on = app.config.grid_video_preview;
                let eco_btn = egui::Button::new(
                    RichText::new(if grid_on {
                        "Previa na grade: ON"
                    } else {
                        "Previa na grade: OFF (Eco)"
                    })
                    .size(11.5)
                    .semibold()
                    .color(if grid_on { Color32::WHITE } else { TEXT_PRIMARY }),
                )
                .fill(if grid_on { EMERALD } else { BG_ELEVATED })
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(8.0));
                if ui
                    .add(eco_btn)
                    .on_hover_text("Alternar visualizacao de video para todos os tiles na grade (desligado economiza CPU/RAM)")
                    .clicked()
                {
                    let next = !grid_on;
                    app.config.grid_video_preview = next;
                    if !next {
                        app.enabled_grid_preview_tiles.clear();
                    }
                    app.config.save();
                }
            });
        });
        ui.add_space(6.0);

        let avail = ui.available_size();
        let count = tiles.len().max(1);
        let gap = 8.0_f32;
        let mut best_cols = 1usize;
        let mut best_fw = -1.0_f32;
        for c in 1..=count.min(4) {
            let r = (count + c - 1) / c;
            let sw = ((avail.x - gap * (c as f32 - 1.0)) / c as f32).max(120.0);
            let sh = ((avail.y - gap * (r as f32 - 1.0) - 2.0) / r as f32).max(90.0);
            let fw = sw.min(sh * (16.0 / 9.0));
            if fw > best_fw {
                best_fw = fw;
                best_cols = c;
            }
        }
        let cols = best_cols;
        let rows = (count + cols - 1) / cols;
        let slot_w = ((avail.x - gap * (cols as f32 - 1.0)) / cols as f32).max(200.0);
        let slot_h = ((avail.y - gap * (rows as f32 - 1.0) - 2.0) / rows as f32).max(126.0);
        // Proporcao fixa 16:9 bloqueada por tile, com borda preta nas sobras do slot
        let fit_w = slot_w.min(slot_h * (16.0 / 9.0)).max(192.0).min(slot_w);
        let fit_h = (fit_w * (9.0 / 16.0)).min(slot_h);

        egui::ScrollArea::vertical()
            .id_salt("media_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
                for row in 0..rows {
                    let row_items = (tiles.len().saturating_sub(row * cols)).min(cols);
                    let row_total_w =
                        row_items as f32 * slot_w + (row_items.saturating_sub(1) as f32) * gap;
                    let left_pad = ((avail.x - row_total_w) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
                        if left_pad > 0.5 {
                            ui.add_space(left_pad - gap.min(left_pad));
                        }
                        for col in 0..cols {
                            let idx = row * cols + col;
                            if idx < tiles.len() {
                                let (outer_rect, _) = ui.allocate_exact_size(
                                    Vec2::new(slot_w, slot_h),
                                    Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    outer_rect,
                                    Rounding::same(20.0),
                                    Color32::from_rgb(13, 17, 23),
                                );
                                let centered = Rect::from_center_size(
                                    outer_rect.center(),
                                    Vec2::new(fit_w, fit_h),
                                );
                                let mut cell_ui =
                                    ui.new_child(egui::UiBuilder::new().max_rect(centered));
                                cell_ui.set_clip_rect(ui.clip_rect().intersect(centered));
                                render_single_tile_entry(
                                    app,
                                    &mut cell_ui,
                                    &tiles[idx],
                                    centered.size(),
                                    false,
                                    &mut open_stats_anchor,
                                );
                            }
                        }
                    });
                }
            });
    }

    if let Some((tile, video_rect)) = open_stats_anchor {
        let copied_recently = app.stats_copied_until_ms > now_epoch_ms();
        let overlay_act = render_stats_overlay(
            ui.ctx(),
            &tile.tile_id,
            video_rect,
            tile.is_self,
            app.active_stats_snapshot.as_ref(),
            &app.peers,
            copied_recently,
        );
        if overlay_act.close {
            app.close_stats_overlay();
        }
        if overlay_act.copied {
            app.stats_copied_until_ms = now_epoch_ms() + 1500;
        }
    }
}

fn tile_reaction_target(tile: &MediaTileEntry, self_id: Option<&str>) -> String {
    if let Some(ref tid) = tile.tool_id {
        return format!("tool:{}", tid);
    }
    if tile.is_self {
        if let (Some(ch), Some(sid)) = (&tile.channel, self_id) {
            return format!("{}:{}", ch, sid);
        }
    }
    tile.tile_id.clone()
}

fn render_single_tile_entry(
    app: &mut GoLiveLiteApp,
    ui: &mut egui::Ui,
    tile: &MediaTileEntry,
    size: Vec2,
    is_focused: bool,
    open_stats_anchor: &mut Option<(MediaTileEntry, egui::Rect)>,
) {
    #[cfg(feature = "mod_ferramentas")]
    if let Some(ref tool_id) = tile.tool_id {
        crate::ui::room_tools_modal::render_tool_media_tile(
            app,
            ui,
            tool_id,
            size,
            is_focused,
            false,
        );
        return;
    }

    let show_preview = app.is_tile_preview_enabled(&tile.tile_id, is_focused);
    let tex = if show_preview {
        app.texture_pool.get(&tile.texture_key)
    } else {
        None
    };
    // Tamanho do quadro, para a camada de "Anotar na tela" achar a area real do video.
    let tex_size = tex.map(|t| t.size_vec2());
    let vol = tile
        .volume_key
        .as_ref()
        .map(|k| app.config.get_transmission_volume(k))
        .unwrap_or(1.0);
    let muted = tile
        .volume_key
        .as_ref()
        .map(|k| app.config.is_transmission_muted(k))
        .unwrap_or(true);
    let show_stats_button = tile.channel.is_some() && !tile.is_stopped_by_us;
    let stats_open = app.open_stats_tile_id.as_deref() == Some(tile.tile_id.as_str());

    let is_fullscreen = app.fullscreen_tile_id.as_deref() == Some(tile.tile_id.as_str());
    let is_pip = app.pip_tile_id.as_deref() == Some(tile.tile_id.as_str());
    #[cfg(feature = "mod_ferramentas")]
    let (reactions_tid, rx_target, rx_picker_open, rx_tuples) = {
        let reactions_tid = if app.has_modification(MOD_FERRAMENTAS) {
            app.room_tools
                .as_ref()
                .and_then(|rt| rt.reactions_tool_id().map(str::to_string))
        } else {
            None
        };
        let rx_target = tile_reaction_target(tile, app.self_id.as_deref());
        let rx_picker_open = reactions_tid.is_some()
            && app
                .room_tools
                .as_ref()
                .is_some_and(|rt| rt.open_reaction_picker_tile.as_deref() == Some(tile.tile_id.as_str()));
        let now = std::time::Instant::now();
        let rx_tuples: Vec<(&str, &str, f32, f32)> = if reactions_tid.is_some() {
            app.room_tools
                .as_ref()
                .map(|rt| {
                    rt.reactions
                        .iter()
                        .filter(|r| r.target == rx_target || r.target == tile.tile_id)
                        .map(|r| {
                            (
                                r.emoji.as_str(),
                                r.name.as_str(),
                                now.saturating_duration_since(r.created_at).as_secs_f32(),
                                r.x_frac,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        (reactions_tid, rx_target, rx_picker_open, rx_tuples)
    };
    #[cfg(not(feature = "mod_ferramentas"))]
    let (reactions_tid, rx_picker_open, rx_tuples): (
        Option<String>,
        bool,
        Vec<(&str, &str, f32, f32)>,
    ) = (None, false, Vec::new());

    let interaction = render_video_tile(
        ui,
        VideoTileViewModel {
            tile_id: &tile.tile_id,
            title: &tile.title,
            badge: &tile.badge,
            texture: tex,
            is_self: tile.is_self,
            is_focused,
            is_fullscreen,
            is_pip,
            is_stopped_by_us: tile.is_stopped_by_us,
            preview_paused_eco: !show_preview && !tile.is_stopped_by_us && !tile.is_ad_paused,
            is_resuming: tile.is_resuming,
            is_ad_paused: tile.is_ad_paused,
            placeholder_note: tile.placeholder_note.as_deref(),
            muted,
            volume: vol,
            allow_audio_controls: tile.volume_key.is_some(),
            show_stats_button,
            stats_open,
            show_reaction_btn: reactions_tid.is_some(),
            reaction_picker_open: rx_picker_open,
            floating_reactions: &rx_tuples,
            resolution_label: tile.resolution_label.clone(),
        },
        size,
    );

    #[cfg(feature = "mod_ferramentas")]
    {
        if interaction.toggle_reaction_picker {
            if let Some(ref mut rt) = app.room_tools {
                if rx_picker_open {
                    rt.open_reaction_picker_tile = None;
                } else {
                    rt.open_reaction_picker_tile = Some(tile.tile_id.clone());
                }
            }
        } else if interaction.close_reaction_picker {
            if let Some(ref mut rt) = app.room_tools {
                if rt.open_reaction_picker_tile.as_deref() == Some(tile.tile_id.as_str()) {
                    rt.open_reaction_picker_tile = None;
                }
            }
        }
        if let (Some(emoji), Some(rtid)) = (interaction.send_reaction_emoji, reactions_tid) {
            app.send_tool_json(crate::net::room_tools::cmd_reaction(
                &rtid,
                &emoji,
                &rx_target,
            ));
        }
        // "Anotar na tela": tracos sobre esta midia (so com a ferramenta aberta na sala).
        if interaction.video_rect != egui::Rect::NOTHING
            && app.room_tools.as_ref().is_some_and(|rt| rt.tools.iter().any(|t| t.kind == "annotate"))
        {
            let content = match tex_size {
                Some(sz) if sz.x > 0.0 && sz.y > 0.0 => {
                    let r = interaction.video_rect;
                    let scale = (r.width() / sz.x).min(r.height() / sz.y);
                    egui::Rect::from_center_size(r.center(), sz * scale)
                }
                _ => interaction.video_rect,
            };
            let is_mgr = app.is_room_manager();
            let self_uid = app.self_user_id.clone();
            let cmd = app.room_tools.as_mut().and_then(|rt| {
                crate::ui::room_tools_modal::render_annotation_layer(rt, ui, &rx_target, content, self_uid.as_deref(), is_mgr)
            });
            if let Some(cmd) = cmd {
                app.send_tool_json(cmd);
            }
        }
    }
    #[cfg(not(feature = "mod_ferramentas"))]
    let _ = (&reactions_tid, rx_picker_open);

    if interaction.toggle_eco_preview {
        app.toggle_tile_preview(&tile.tile_id);
    }

    if interaction.toggle_stats {
        app.toggle_tile_stats(tile);
    }
    if app.open_stats_tile_id.as_deref() == Some(tile.tile_id.as_str())
        && interaction.video_rect != egui::Rect::NOTHING
    {
        *open_stats_anchor = Some((tile.clone(), interaction.video_rect));
    }

    if interaction.toggle_fullscreen {
        app.toggle_fullscreen(tile.tile_id.clone(), ui.ctx());
    }
    if interaction.toggle_pip {
        app.toggle_pip(tile.tile_id.clone(), ui.ctx());
    }

    if interaction.toggle_focus {
        if is_focused {
            app.focused_tile_id = None;
        } else {
            app.focused_tile_id = Some(tile.tile_id.clone());
        }
    }
    if interaction.stop_self_share {
        if tile.channel.as_deref() == Some("camera") {
            app.stop_camera();
        } else if let Some(ref ch) = tile.channel {
            app.stop_screen_slot(ch);
        }
    }

    if interaction.toggle_stop_watching {
        if let (Some(ch), Some(pid)) = (&tile.channel, &tile.peer_id) {
            if tile.is_stopped_by_us {
                app.resume_watching_peer(ch.clone(), pid.clone());
            } else {
                app.stop_watching_peer(ch.clone(), pid.clone());
            }
        }
    }
    if let Some(ref vkey) = tile.volume_key {
        if interaction.toggle_mute {
            let next = !muted;
            app.config.set_transmission_muted(vkey, next);
            app.config.save();
            if let (Some(ch), Some(pid)) = (&tile.channel, &tile.peer_id) {
                app.mixer.set_source_muted(&format!("{}:{}", ch, pid), next);
            }
        }
        if let Some(new_v) = interaction.new_volume {
            app.config.set_transmission_volume(vkey, new_v);
            app.config.save();
            if let (Some(ch), Some(pid)) = (&tile.channel, &tile.peer_id) {
                app.mixer.set_source_volume(&format!("{}:{}", ch, pid), new_v);
            }
        }
    }
}

fn render_mic_popover(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_mic_popover {
        return;
    }
    let mut open = true;
    centered_movable_window(
        "Configuracoes do Microfone",
        "golive_mic_popover_modal",
        ctx,
        Vec2::new(300.0, 210.0),
    )
    .open(&mut open)
    .default_width(300.0)
    .frame(
        egui::Frame::window(&ctx.style())
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(14.0)),
    )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dispositivo de entrada:").size(12.0).color(TEXT_SECONDARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Atualizar").clicked() {
                        app.cached_mic_devices = MicCapture::list_input_devices();
                    }
                });
            });
            let devices = app.cached_mic_devices.clone();
            let current = app
                .config
                .mic_device_name
                .clone()
                .unwrap_or_else(|| "Padrao do Sistema".to_string());
            egui::ComboBox::from_id_salt("mic_dev_combo")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(app.config.mic_device_name.is_none(), "Padrao do Sistema")
                        .clicked()
                    {
                        app.config.mic_device_name = None;
                        app.config.save();
                        app.restart_mic_if_active();
                    }
                    for d in devices {
                        let sel = app.config.mic_device_name.as_deref() == Some(d.as_str());
                        if ui.selectable_label(sel, &d).clicked() {
                            app.config.mic_device_name = Some(d);
                            app.config.save();
                            app.restart_mic_if_active();
                        }
                    }
                });

            ui.add_space(6.0);
            let mut pct = app.config.mic_gain * 100.0;
            ui.label(
                RichText::new(format!("Ganho do Microfone: {}%", pct.round() as i32))
                    .size(12.0)
                    .color(TEXT_PRIMARY),
            );
            let s = ui.add(egui::Slider::new(&mut pct, 1.0..=200.0).show_value(false));
            if s.double_clicked() {
                app.config.mic_gain = 1.0;
                app.mic.state.set_gain(1.0);
                app.config.save();
            } else if s.changed() {
                app.config.mic_gain = pct / 100.0;
                app.mic.state.set_gain(app.config.mic_gain);
                app.config.save();
            }

            ui.add_space(6.0);
            if ui
                .checkbox(
                    &mut app.config.push_to_talk_enabled,
                    format!(
                        "Apertar para falar (Push-to-Talk: {})",
                        vk_name(app.config.push_to_talk_vk)
                    ),
                )
                .changed()
            {
                app.mic
                    .state
                    .push_to_talk
                    .store(app.config.push_to_talk_enabled, Ordering::Relaxed);
                app.config.save();
            }
        });
    if !open {
        app.show_mic_popover = false;
    }
}

fn render_speaker_popover(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_speaker_popover {
        return;
    }
    let mut open = true;
    centered_movable_window(
        "Dispositivo de Saida de Audio",
        "golive_speaker_popover_modal",
        ctx,
        Vec2::new(300.0, 140.0),
    )
    .open(&mut open)
    .default_width(300.0)
    .frame(
        egui::Frame::window(&ctx.style())
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(14.0)),
    )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dispositivo de saida:").size(12.0).color(TEXT_SECONDARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Atualizar").clicked() {
                        app.cached_speaker_devices = AudioMixer::list_output_devices();
                    }
                });
            });
            let devices = app.cached_speaker_devices.clone();
            let current = app
                .config
                .speaker_device_name
                .clone()
                .unwrap_or_else(|| "Padrao do Sistema".to_string());
            egui::ComboBox::from_id_salt("spk_dev_combo")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(app.config.speaker_device_name.is_none(), "Padrao do Sistema")
                        .clicked()
                    {
                        app.config.speaker_device_name = None;
                        app.config.save();
                        let _ = app.mixer.start(None);
                    }
                    for d in devices {
                        let sel = app.config.speaker_device_name.as_deref() == Some(d.as_str());
                        if ui.selectable_label(sel, &d).clicked() {
                            app.config.speaker_device_name = Some(d.clone());
                            app.config.save();
                            let _ = app.mixer.start(Some(&d));
                        }
                    }
                });
        });
    if !open {
        app.show_speaker_popover = false;
    }
}

fn render_fullscreen_stage(app: &mut GoLiveLiteApp, ctx: &egui::Context, tile: &MediaTileEntry) {
    #[cfg(feature = "mod_ferramentas")]
    if let Some(ref tool_id) = tile.tool_id {
        let tid = tool_id.clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(22, 27, 34))
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                let full = ui.available_size();
                crate::ui::room_tools_modal::render_tool_media_tile(
                    app, ui, &tid, full, true, true,
                );
            });
        return;
    }

    let mut exit_fs = false;
    let mut toggle_pip = false;
    let mut toggle_stats = false;
    let mut stop_self = false;
    let mut stop_watching = false;
    let mut toggle_mute = false;
    let mut new_volume: Option<f32> = None;
    let mut video_rect = Rect::NOTHING;
    #[cfg(feature = "mod_ferramentas")]
    let reactions_tid = if app.has_modification(MOD_FERRAMENTAS) {
        app.room_tools
            .as_ref()
            .and_then(|rt| rt.reactions_tool_id().map(str::to_string))
    } else {
        None
    };
    #[cfg(not(feature = "mod_ferramentas"))]
    let reactions_tid: Option<String> = None;
    let rx_target = tile_reaction_target(tile, app.self_id.as_deref());
    #[cfg(feature = "mod_ferramentas")]
    let rx_picker_open = reactions_tid.is_some()
        && app
            .room_tools
            .as_ref()
            .is_some_and(|rt| rt.open_reaction_picker_tile.as_deref() == Some(tile.tile_id.as_str()));
    #[cfg(not(feature = "mod_ferramentas"))]
    let rx_picker_open = false;
    let mut toggle_rx_picker = false;
    let mut send_rx_emoji: Option<String> = None;

    let stats_open = app.open_stats_tile_id.as_deref() == Some(tile.tile_id.as_str());
    let is_pip = app.pip_tile_id.as_deref() == Some(tile.tile_id.as_str());
    let vol = tile
        .volume_key
        .as_ref()
        .map(|k| app.config.get_transmission_volume(k))
        .unwrap_or(1.0);
    let muted = tile
        .volume_key
        .as_ref()
        .map(|k| app.config.is_transmission_muted(k))
        .unwrap_or(true);
    let tex_info = app
        .texture_pool
        .get(&tile.texture_key)
        .map(|t| (t.id(), t.size_vec2()));

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(Color32::BLACK)
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            let full = ui.available_size();
            let (rect, resp) = ui.allocate_exact_size(full, Sense::click());
            video_rect = rect;
            ui.painter().rect_filled(rect, Rounding::ZERO, Color32::BLACK);

            if tile.is_resuming {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Retomando transmissao...",
                    egui::FontId::proportional(15.0),
                    TEXT_SECONDARY,
                );
            } else if tile.is_ad_paused {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Transmissao pausada temporariamente pelo transmissor",
                    egui::FontId::proportional(14.5),
                    TEXT_SECONDARY,
                );
            } else if let Some((tid, tsz)) = tex_info {
                let fit = fit_aspect(tsz, rect.size());
                let img_rect = Rect::from_center_size(rect.center(), fit);
                ui.painter().image(
                    tid,
                    img_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else if let Some(ref note) = tile.placeholder_note {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    note,
                    egui::FontId::proportional(14.0),
                    TEXT_SECONDARY,
                );
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Conectando midia...",
                    egui::FontId::proportional(14.0),
                    TEXT_MUTED,
                );
            }

            let hover_pos = ctx.input(|i| i.pointer.hover_pos());
            let show_top =
                stats_open || hover_pos.map_or(true, |p| p.y <= rect.top() + 110.0);
            let show_bottom =
                hover_pos.is_some_and(|p| p.y >= rect.bottom() - 80.0);
            let dark_circle = Color32::from_rgba_unmultiplied(12, 14, 19, 200);
            let mut overlay_interacted = false;

            if show_top {
                // Pill superior esquerda com identificacao da midia
                let top_left_rect = Rect::from_min_max(
                    pos2(rect.left() + 14.0, rect.top() + 12.0),
                    pos2((rect.right() - 220.0).max(rect.left() + 260.0), rect.top() + 44.0),
                );
                let mut tl_ui = ui.new_child(egui::UiBuilder::new().max_rect(top_left_rect));
                egui::Frame::none()
                    .fill(dark_circle)
                    .stroke(Stroke::new(
                        1.0_f32,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 34),
                    ))
                    .rounding(Rounding::same(16.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(&mut tl_ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                RichText::new(&tile.badge)
                                    .size(11.0)
                                    .semibold()
                                    .color(EMERALD_BRIGHT),
                            );
                            ui.label(
                                RichText::new(&tile.title)
                                    .size(12.5)
                                    .semibold()
                                    .color(Color32::WHITE),
                            );
                            if let Some(ref res_lbl) = tile.resolution_label {
                                ui.label(
                                    RichText::new(res_lbl)
                                        .size(11.0)
                                        .color(TEXT_SECONDARY),
                                );
                            }
                            ui.label(
                                RichText::new("• Esc para sair")
                                    .size(11.0)
                                    .color(TEXT_MUTED),
                            );
                        });
                    });

                // Botoes circulares no canto superior direito
                let btn_d = 34.0;
                let icon_sz = 16.0;
                let top_right_rect = Rect::from_min_max(
                    pos2(rect.left() + 14.0, rect.top() + 12.0),
                    pos2(rect.right() - 14.0, rect.top() + 12.0 + btn_d),
                );
                let mut tr_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(top_right_rect)
                        .layout(Layout::right_to_left(Align::Center)),
                );
                tr_ui.spacing_mut().item_spacing.x = 7.0;

                if tile.is_self {
                    let sr = icons::media_overlay_circle_button(
                        &mut tr_ui,
                        btn_d,
                        Color32::from_rgba_unmultiplied(220, 38, 38, 225),
                        "Parar de compartilhar esta tela/janela",
                        |p, c, col| icons::paint_stop_square(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= sr.hovered() || sr.clicked();
                    if sr.clicked() {
                        stop_self = true;
                    }
                } else {
                    let lr = icons::media_overlay_circle_button(
                        &mut tr_ui,
                        btn_d,
                        dark_circle,
                        "Parar de assistir esta transmissao",
                        |p, c, col| icons::paint_close_x(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= lr.hovered() || lr.clicked();
                    if lr.clicked() {
                        stop_watching = true;
                    }
                }

                let fsr = icons::media_overlay_circle_button(
                    &mut tr_ui,
                    btn_d,
                    crate::ui::theme::BLUE,
                    "Sair da tela cheia (Esc)",
                    |p, c, col| icons::paint_fullscreen(p, c, icon_sz, col, true),
                    Color32::WHITE,
                );
                overlay_interacted |= fsr.hovered() || fsr.clicked();
                if fsr.clicked() {
                    exit_fs = true;
                }

                let pr = icons::media_overlay_circle_button(
                    &mut tr_ui,
                    btn_d,
                    if is_pip { EMERALD } else { dark_circle },
                    if is_pip {
                        "Fechar Picture-in-Picture (PiP)"
                    } else {
                        "Abrir em Picture-in-Picture (PiP)"
                    },
                    |p, c, col| icons::paint_pip(p, c, icon_sz, col),
                    Color32::WHITE,
                );
                overlay_interacted |= pr.hovered() || pr.clicked();
                if pr.clicked() {
                    toggle_pip = true;
                }

                if tile.channel.is_some() {
                    let str_r = icons::media_overlay_circle_button(
                        &mut tr_ui,
                        btn_d,
                        if stats_open { EMERALD } else { dark_circle },
                        if tile.is_self {
                            "Sua transmissao (estatisticas)"
                        } else {
                            "Estatisticas da conexao"
                        },
                        |p, c, col| icons::paint_stats_bars(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= str_r.hovered() || str_r.clicked();
                    if str_r.clicked() {
                        toggle_stats = true;
                    }
                }

                #[cfg(feature = "mod_ferramentas")]
                if reactions_tid.is_some() {
                    let rx_r = icons::media_overlay_circle_button(
                        &mut tr_ui,
                        btn_d,
                        if rx_picker_open { EMERALD } else { dark_circle },
                        "Reagir a esta midia",
                        |p, c, col| icons::paint_reaction_smile(p, c, icon_sz, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= rx_r.hovered() || rx_r.clicked();
                    if rx_r.clicked() {
                        toggle_rx_picker = true;
                    }
                    if rx_picker_open {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("fs_rx_btn_rect"), rx_r.rect);
                        });
                    }
                }
            }

            #[cfg(feature = "mod_ferramentas")]
            if rx_picker_open && reactions_tid.is_some() {
                let picker_w = (rect.width() - 28.0).clamp(180.0, 356.0);
                let picker_rect = Rect::from_min_size(
                    pos2((rect.right() - picker_w - 14.0).max(rect.left() + 14.0), rect.top() + 52.0),
                    Vec2::new(picker_w, 36.0),
                );
                let mut p_ui = ui.new_child(egui::UiBuilder::new().max_rect(picker_rect));
                let pop_resp = egui::Frame::none()
                    .fill(dark_circle)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .rounding(Rounding::same(16.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                    .show(&mut p_ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                            for &em in crate::net::room_tools::REACTION_EMOJIS {
                                let (er, b) = ui.allocate_exact_size(
                                    Vec2::splat(24.0),
                                    Sense::click(),
                                );
                                if b.hovered() {
                                    ui.painter().rect_filled(
                                        er,
                                        Rounding::same(6.0),
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 28),
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
                                if b.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    send_rx_emoji = Some(em.to_string());
                                }
                            }
                        });
                    });
                let btn_rect_opt: Option<Rect> = ui.ctx().data(|d| d.get_temp(egui::Id::new("fs_rx_btn_rect")));
                if ui.ctx().input(|i| {
                    i.pointer.any_pressed()
                        && i.pointer.interact_pos().is_some_and(|p| {
                            !pop_resp.response.rect.contains(p)
                                && !btn_rect_opt.is_some_and(|br| br.contains(p))
                        })
                }) {
                    toggle_rx_picker = true;
                }
            }

            #[cfg(feature = "mod_ferramentas")]
            if reactions_tid.is_some() {
                let now = std::time::Instant::now();
                let rx_tuples: Vec<(&str, &str, f32, f32)> = app
                    .room_tools
                    .as_ref()
                    .map(|rt| {
                        rt.reactions
                            .iter()
                            .filter(|r| r.target == rx_target || r.target == tile.tile_id)
                            .map(|r| {
                                (
                                    r.emoji.as_str(),
                                    r.name.as_str(),
                                    now.saturating_duration_since(r.created_at).as_secs_f32(),
                                    r.x_frac,
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !rx_tuples.is_empty() {
                    crate::ui::video_tile::paint_floating_reactions(
                        ui.ctx(),
                        ui.painter(),
                        rect,
                        &rx_tuples,
                    );
                }
            }

            if show_bottom && tile.volume_key.is_some() {
                let pill_w = 290.0_f32.min(rect.width() - 32.0);
                let bottom_rect = Rect::from_center_size(
                    pos2(rect.center().x, rect.bottom() - 28.0),
                    Vec2::new(pill_w, 36.0),
                );
                let mut bt_ui = ui.new_child(egui::UiBuilder::new().max_rect(bottom_rect));
                egui::Frame::none()
                    .fill(dark_circle)
                    .stroke(Stroke::new(
                        1.0_f32,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 34),
                    ))
                    .rounding(Rounding::same(18.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(&mut bt_ui, |ui| {
                        ui.horizontal(|ui| {
                            let mr = icons::media_overlay_circle_button(
                                ui,
                                26.0,
                                if muted { RED } else { Color32::from_rgba_unmultiplied(255, 255, 255, 22) },
                                if muted { "Reativar audio" } else { "Silenciar transmissao" },
                                |p, c, col| icons::paint_speaker(p, c, 13.0, col, muted),
                                Color32::WHITE,
                            );
                            overlay_interacted |= mr.hovered() || mr.clicked();
                            if mr.clicked() {
                                toggle_mute = true;
                            }
                            let mut pct = vol * 100.0;
                            let sl = ui.add(
                                egui::Slider::new(&mut pct, 0.0..=200.0)
                                    .show_value(false)
                                    .step_by(1.0),
                            );
                            overlay_interacted |= sl.hovered() || sl.dragged();
                            if sl.double_clicked() {
                                new_volume = Some(1.0);
                            } else if sl.changed() {
                                new_volume = Some(pct / 100.0);
                            }
                            ui.label(
                                RichText::new(format!("{}%", pct.round() as i32))
                                    .size(11.5)
                                    .color(Color32::WHITE),
                            );
                        });
                    });
            }

            if resp.double_clicked() && !overlay_interacted {
                exit_fs = true;
            }
        });

    #[cfg(feature = "mod_ferramentas")]
    {
        if toggle_rx_picker {
            if let Some(ref mut rt) = app.room_tools {
                if rx_picker_open {
                    rt.open_reaction_picker_tile = None;
                } else {
                    rt.open_reaction_picker_tile = Some(tile.tile_id.clone());
                }
            }
        }
        if let (Some(emoji), Some(rtid)) = (send_rx_emoji, reactions_tid) {
            app.send_tool_json(crate::net::room_tools::cmd_reaction(
                &rtid,
                &emoji,
                &rx_target,
            ));
        }
    }
    #[cfg(not(feature = "mod_ferramentas"))]
    let _ = (&mut toggle_rx_picker, &mut send_rx_emoji, &rx_target, reactions_tid, rx_picker_open);

    if stats_open && video_rect != Rect::NOTHING {
        let stats_anchor = Rect::from_min_max(
            pos2(video_rect.left() + 8.0, video_rect.top() + 50.0),
            video_rect.max,
        );
        let copied_recently = app.stats_copied_until_ms > now_epoch_ms();
        let overlay_act = render_stats_overlay(
            ctx,
            &tile.tile_id,
            stats_anchor,
            tile.is_self,
            app.active_stats_snapshot.as_ref(),
            &app.peers,
            copied_recently,
        );
        if overlay_act.close {
            app.close_stats_overlay();
        }
        if overlay_act.copied {
            app.stats_copied_until_ms = now_epoch_ms() + 1500;
        }
    }

    if toggle_stats {
        app.toggle_tile_stats(tile);
    }
    if let Some(ref vkey) = tile.volume_key {
        if toggle_mute {
            let next = !muted;
            app.config.set_transmission_muted(vkey, next);
            app.config.save();
            if let (Some(ch), Some(pid)) = (&tile.channel, &tile.peer_id) {
                app.mixer.set_source_muted(&format!("{}:{}", ch, pid), next);
            }
        }
        if let Some(nv) = new_volume {
            app.config.set_transmission_volume(vkey, nv);
            app.config.save();
            if let (Some(ch), Some(pid)) = (&tile.channel, &tile.peer_id) {
                app.mixer.set_source_volume(&format!("{}:{}", ch, pid), nv);
            }
        }
    }
    if stop_self {
        app.exit_fullscreen(ctx);
        if tile.channel.as_deref() == Some("camera") {
            app.stop_camera();
        } else if let Some(ref ch) = tile.channel {
            app.stop_screen_slot(ch);
        }
    } else if stop_watching {
        app.exit_fullscreen(ctx);
        if let (Some(ch), Some(pid)) = (&tile.channel, &tile.peer_id) {
            app.stop_watching_peer(ch.clone(), pid.clone());
        }
    } else if toggle_pip {
        app.toggle_pip(tile.tile_id.clone(), ctx);
    } else if exit_fs {
        app.exit_fullscreen(ctx);
    }
}

fn render_pip_viewport(
    app: &mut GoLiveLiteApp,
    ctx: &egui::Context,
    tiles: &[MediaTileEntry],
) {
    let Some(pip_id) = app.pip_tile_id.clone() else {
        return;
    };
    let Some(pip_tile) = tiles
        .iter()
        .find(|t| t.tile_id == pip_id && !t.is_stopped_by_us)
        .cloned()
    else {
        app.close_pip();
        return;
    };

    let tex_info = app
        .texture_pool
        .get(&pip_tile.texture_key)
        .map(|t| (t.id(), t.size_vec2()));
    let allow_audio = pip_tile.volume_key.is_some();
    let muted = pip_tile
        .volume_key
        .as_ref()
        .map(|k| app.config.is_transmission_muted(k))
        .unwrap_or(true);
    let title = pip_tile.title.clone();
    let is_resuming = pip_tile.is_resuming;
    let is_ad_paused = pip_tile.is_ad_paused;
    let placeholder_note = pip_tile.placeholder_note.clone();

    let mut pip_close = false;
    let mut pip_return_to_app = false;
    let mut pip_toggle_mute = false;

    let vp_id = egui::ViewportId::from_hash_of(crate::app::PIP_VIEWPORT_ID_STR);
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("Go Live - Lite • PiP - {}", title))
        .with_inner_size([448.0, 252.0])
        .with_min_inner_size([240.0, 135.0])
        .with_resizable(true)
        .with_decorations(false)
        .with_always_on_top()
        .with_icon(std::sync::Arc::new(crate::ui::app_icon::build_window_icon()));

    ctx.show_viewport_immediate(vp_id, builder, |pip_ctx, class| {
        if pip_ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
            pip_close = true;
        }

        if class == egui::ViewportClass::Embedded {
            let mut open = true;
            centered_movable_window(
                format!("PiP - {}", title),
                "golive_pip_embedded_fallback",
                pip_ctx,
                Vec2::new(400.0, 225.0),
            )
            .open(&mut open)
            .default_size([400.0, 225.0])
                .frame(
                    egui::Frame::window(&pip_ctx.style())
                        .fill(Color32::BLACK)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(10.0)),
                )
                .show(pip_ctx, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    let (_, resp) = ui.allocate_exact_size(rect.size(), Sense::click());
                    paint_pip_video_contents(
                        ui.painter(),
                        rect,
                        tex_info,
                        is_resuming,
                        is_ad_paused,
                        placeholder_note.as_deref(),
                    );
                    if resp.double_clicked() {
                        pip_return_to_app = true;
                    }
                });
            if !open {
                pip_close = true;
            }
            return;
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::BLACK)
                    .stroke(Stroke::new(
                        1.0_f32,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 36),
                    ))
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(pip_ctx, |ui| {
                let avail = ui.available_size();
                let (rect, bg_resp) = ui.allocate_exact_size(avail, Sense::click());
                paint_pip_video_contents(
                    ui.painter(),
                    rect,
                    tex_info,
                    is_resuming,
                    is_ad_paused,
                    placeholder_note.as_deref(),
                );

                // Alcas de redimensionamento nas bordas e cantos (janela sem decoracao)
                let hover_pos = pip_ctx.input(|i| i.pointer.hover_pos());
                let resize_dir = hover_pos.and_then(|p| detect_pip_resize_dir(rect, p, 7.0));
                if let Some(dir) = resize_dir {
                    pip_ctx.set_cursor_icon(resize_cursor_for(dir));
                    if pip_ctx.input(|i| i.pointer.primary_pressed()) {
                        pip_ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                    }
                }

                let mut overlay_interacted = false;
                if hover_pos.is_some() {
                    let dark_circle = Color32::from_rgba_unmultiplied(12, 14, 19, 200);
                    let title_rect = Rect::from_min_max(
                        pos2(rect.left() + 8.0, rect.top() + 8.0),
                        pos2((rect.right() - 108.0).max(rect.left() + 80.0), rect.top() + 32.0),
                    );
                    let mut tl_ui = ui.new_child(egui::UiBuilder::new().max_rect(title_rect));
                    egui::Frame::none()
                        .fill(dark_circle)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                        .show(&mut tl_ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&title)
                                        .size(11.0)
                                        .semibold()
                                        .color(Color32::WHITE),
                                )
                                .truncate(),
                            );
                        });

                    let btn_d = 26.0;
                    let top_right = Rect::from_min_max(
                        pos2(rect.left() + 8.0, rect.top() + 8.0),
                        pos2(rect.right() - 8.0, rect.top() + 8.0 + btn_d),
                    );
                    let mut tr_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(top_right)
                            .layout(Layout::right_to_left(Align::Center)),
                    );
                    tr_ui.spacing_mut().item_spacing.x = 5.0;

                    let close_r = icons::media_overlay_circle_button(
                        &mut tr_ui,
                        btn_d,
                        Color32::from_rgba_unmultiplied(220, 38, 38, 210),
                        "Fechar Picture-in-Picture (PiP)",
                        |p, c, col| icons::paint_close_x(p, c, 12.5, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= close_r.hovered() || close_r.clicked();
                    if close_r.clicked() {
                        pip_close = true;
                    }

                    let ret_r = icons::media_overlay_circle_button(
                        &mut tr_ui,
                        btn_d,
                        dark_circle,
                        "Voltar e focar no app",
                        |p, c, col| icons::paint_focus(p, c, 12.5, col),
                        Color32::WHITE,
                    );
                    overlay_interacted |= ret_r.hovered() || ret_r.clicked();
                    if ret_r.clicked() {
                        pip_return_to_app = true;
                    }

                    if allow_audio {
                        let aud_r = icons::media_overlay_circle_button(
                            &mut tr_ui,
                            btn_d,
                            if muted { RED } else { dark_circle },
                            if muted { "Reativar audio" } else { "Silenciar transmissao" },
                            |p, c, col| icons::paint_speaker(p, c, 12.5, col, muted),
                            Color32::WHITE,
                        );
                        overlay_interacted |= aud_r.hovered() || aud_r.clicked();
                        if aud_r.clicked() {
                            pip_toggle_mute = true;
                        }
                    }

                    // Indicador visual discreto de redimensionamento no canto inferior direito
                    let grip_col = Color32::from_rgba_unmultiplied(255, 255, 255, 110);
                    let br = rect.max;
                    for d in [4.0_f32, 8.0, 12.0] {
                        ui.painter().line_segment(
                            [pos2(br.x - d, br.y - 2.0), pos2(br.x - 2.0, br.y - d)],
                            Stroke::new(1.1_f32, grip_col),
                        );
                    }
                }

                if bg_resp.double_clicked() && !overlay_interacted && resize_dir.is_none() {
                    pip_return_to_app = true;
                } else if !overlay_interacted
                    && resize_dir.is_none()
                    && pip_ctx.input(|i| i.pointer.primary_pressed())
                {
                    pip_ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
    });

    if pip_toggle_mute {
        if let Some(ref vkey) = pip_tile.volume_key {
            let next = !muted;
            app.config.set_transmission_muted(vkey, next);
            app.config.save();
            if let (Some(ch), Some(pid)) = (&pip_tile.channel, &pip_tile.peer_id) {
                app.mixer.set_source_muted(&format!("{}:{}", ch, pid), next);
            }
        }
    }

    if pip_return_to_app {
        app.pip_tile_id = None;
        app.focused_tile_id = Some(pip_tile.tile_id.clone());
        app.sync_video_render_policies();
        app.set_window_visible_from_ui(ctx);
    } else if pip_close {
        app.close_pip();
    }
}

fn paint_pip_video_contents(
    painter: &egui::Painter,
    rect: Rect,
    tex_info: Option<(egui::TextureId, Vec2)>,
    is_resuming: bool,
    is_ad_paused: bool,
    placeholder_note: Option<&str>,
) {
    painter.rect_filled(rect, Rounding::ZERO, Color32::BLACK);
    if is_resuming {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Retomando transmissao...",
            egui::FontId::proportional(12.5),
            TEXT_SECONDARY,
        );
    } else if is_ad_paused {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Transmissao pausada",
            egui::FontId::proportional(12.0),
            TEXT_SECONDARY,
        );
    } else if let Some((tid, tsz)) = tex_info {
        let fit = fit_aspect(tsz, rect.size());
        let img_rect = Rect::from_center_size(rect.center(), fit);
        painter.image(
            tid,
            img_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else if let Some(note) = placeholder_note {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            note,
            egui::FontId::proportional(11.5),
            TEXT_SECONDARY,
        );
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Conectando midia...",
            egui::FontId::proportional(12.0),
            TEXT_MUTED,
        );
    }
}

fn detect_pip_resize_dir(
    rect: Rect,
    pos: egui::Pos2,
    edge: f32,
) -> Option<egui::ResizeDirection> {
    let left = pos.x <= rect.left() + edge;
    let right = pos.x >= rect.right() - edge;
    let top = pos.y <= rect.top() + edge;
    let bottom = pos.y >= rect.bottom() - edge;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(egui::ResizeDirection::NorthWest),
        (_, true, true, _) => Some(egui::ResizeDirection::NorthEast),
        (true, _, _, true) => Some(egui::ResizeDirection::SouthWest),
        (_, true, _, true) => Some(egui::ResizeDirection::SouthEast),
        (true, _, _, _) => Some(egui::ResizeDirection::West),
        (_, true, _, _) => Some(egui::ResizeDirection::East),
        (_, _, true, _) => Some(egui::ResizeDirection::North),
        (_, _, _, true) => Some(egui::ResizeDirection::South),
        _ => None,
    }
}

fn resize_cursor_for(dir: egui::ResizeDirection) -> CursorIcon {
    match dir {
        egui::ResizeDirection::North => CursorIcon::ResizeNorth,
        egui::ResizeDirection::South => CursorIcon::ResizeSouth,
        egui::ResizeDirection::East => CursorIcon::ResizeEast,
        egui::ResizeDirection::West => CursorIcon::ResizeWest,
        egui::ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        egui::ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        egui::ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        egui::ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

#[cfg(feature = "mod_ferramentas")]
fn render_hands_queue_overlay(
    app: &mut GoLiveLiteApp,
    ctx: &egui::Context,
    stage_bounds: Rect,
    left_collapsed: bool,
) {
    let is_manager = app.is_room_manager();
    let mics_locked = !app.room_permissions.mic;
    if !is_manager || !mics_locked {
        return;
    }

    let Some(ref rt) = app.room_tools else {
        return;
    };
    if rt.hands.is_empty() && rt.speakers.is_empty() {
        return;
    }

    let mut ordered: Vec<(String, String, u64)> = rt
        .hands
        .iter()
        .map(|h| (h.id.clone(), h.name.clone(), h.at))
        .collect();
    ordered.sort_by_key(|h| h.2);
    let speakers = rt.speakers.clone();
    let collapsed = rt.hands_queue_collapsed;

    let name_of = |uid: &str| -> Option<String> {
        let self_uid = app
            .logged_account
            .as_ref()
            .map(|a| a.id.as_str())
            .or(app.self_user_id.as_deref());
        if self_uid == Some(uid) || app.self_id.as_deref() == Some(uid) {
            return Some(app.effective_name().to_string());
        }
        app.peers
            .iter()
            .find(|p| p.user_id.as_deref() == Some(uid) || p.id == uid)
            .map(|p| p.name.clone())
    };

    let mut toggle_collapse = false;
    let mut cmds: Vec<serde_json::Value> = Vec::new();
    let x_offset = if left_collapsed { 44.0 } else { 10.0 };
    let pos = pos2(stage_bounds.left() + x_offset, stage_bounds.top() + 10.0);
    let card_w = (stage_bounds.width() - x_offset - 12.0).clamp(180.0, 272.0);

    egui::Area::new(egui::Id::new("golive_hands_queue_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            let border_col = Color32::from_rgba_unmultiplied(245, 158, 11, 145);
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(20, 24, 33, 246))
                .stroke(Stroke::new(1.0_f32, border_col))
                .rounding(Rounding::same(11.0))
                .show(ui, |ui| {
                    ui.set_width(card_w);
                    let hdr_h = 32.0;
                    let (hdr_rect, hdr_resp) =
                        ui.allocate_exact_size(Vec2::new(card_w, hdr_h), Sense::click());
                    ui.painter().rect_filled(
                        hdr_rect,
                        Rounding {
                            nw: 11.0,
                            ne: 11.0,
                            sw: if collapsed { 11.0 } else { 0.0 },
                            se: if collapsed { 11.0 } else { 0.0 },
                        },
                        Color32::from_rgba_unmultiplied(120, 53, 15, 115),
                    );
                    icons::paint_hand_raise(
                        ui.painter(),
                        pos2(hdr_rect.left() + 16.0, hdr_rect.center().y),
                        14.0,
                        Color32::from_rgb(252, 211, 77),
                    );
                    ui.painter().text(
                        pos2(hdr_rect.left() + 30.0, hdr_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("Mãos levantadas ({})", ordered.len()),
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(254, 243, 199),
                    );
                    ui.painter().text(
                        pos2(hdr_rect.right() - 12.0, hdr_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        if collapsed { ">" } else { "v" },
                        egui::FontId::proportional(11.0),
                        Color32::from_rgb(253, 230, 138),
                    );
                    if hdr_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        toggle_collapse = true;
                    }

                    if !collapsed {
                        egui::Frame::none()
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.set_width(card_w - 16.0);
                                egui::ScrollArea::vertical().max_height(210.0).show(ui, |ui| {
                                    if !ordered.is_empty() {
                                        ui.label(
                                            RichText::new("Os microfones da sala estão bloqueados: liberar deixa só essa pessoa falar.")
                                                .size(10.5)
                                                .color(TEXT_MUTED),
                                        );
                                        ui.add_space(4.0);
                                    }
                                    for (idx, (uid, fallback_name, _)) in ordered.iter().enumerate() {
                                        let label = name_of(uid).unwrap_or_else(|| fallback_name.clone());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{}", idx + 1))
                                                    .size(10.5)
                                                    .color(TEXT_MUTED),
                                            );
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(label)
                                                        .size(12.0)
                                                        .color(TEXT_PRIMARY),
                                                )
                                                .truncate(),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let dismiss_btn = icons::icon_button(
                                                        ui,
                                                        Vec2::splat(22.0),
                                                        BG_ELEVATED,
                                                        Stroke::NONE,
                                                        Rounding::same(6.0),
                                                        "Dispensar mao",
                                                        |p, c, col| {
                                                            icons::paint_close_x(p, c, 9.5, col)
                                                        },
                                                        TEXT_SECONDARY,
                                                    );
                                                    if dismiss_btn.clicked() {
                                                        cmds.push(crate::net::room_tools::cmd_hand_action(
                                                            "hand-dismiss",
                                                            uid,
                                                        ));
                                                    }
                                                    let grant_btn = ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new("Liberar")
                                                                    .size(10.5)
                                                                    .color(Color32::WHITE),
                                                            )
                                                            .fill(EMERALD)
                                                            .rounding(Rounding::same(6.0))
                                                            .min_size(Vec2::new(54.0, 22.0)),
                                                        )
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .on_hover_text("Liberar para falar");
                                                    if grant_btn.clicked() {
                                                        cmds.push(crate::net::room_tools::cmd_hand_action(
                                                            "hand-grant",
                                                            uid,
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(3.0);
                                    }

                                    if !speakers.is_empty() {
                                        if !ordered.is_empty() {
                                            ui.add_space(4.0);
                                            ui.separator();
                                            ui.add_space(4.0);
                                        }
                                        ui.label(
                                            RichText::new("LIBERADOS PARA FALAR")
                                                .size(10.0)
                                                .semibold()
                                                .color(TEXT_MUTED),
                                        );
                                        ui.add_space(2.0);
                                        for sid in &speakers {
                                            let label = name_of(sid)
                                                .unwrap_or_else(|| "Alguém que saiu".to_string());
                                            ui.horizontal(|ui| {
                                                let (mr, _) = ui.allocate_exact_size(
                                                    Vec2::splat(14.0),
                                                    Sense::hover(),
                                                );
                                                icons::paint_mic(
                                                    ui.painter(),
                                                    mr.center(),
                                                    12.0,
                                                    EMERALD_BRIGHT,
                                                    false,
                                                );
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(label)
                                                            .size(12.0)
                                                            .color(TEXT_PRIMARY),
                                                    )
                                                    .truncate(),
                                                );
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        let revoke_btn = ui
                                                            .add(
                                                                egui::Button::new(
                                                                    RichText::new("Tirar")
                                                                        .size(10.5)
                                                                        .color(Color32::from_rgb(
                                                                            252, 165, 165,
                                                                        )),
                                                                )
                                                                .fill(Color32::from_rgb(
                                                                    65, 24, 29,
                                                                ))
                                                                .rounding(Rounding::same(6.0))
                                                                .min_size(Vec2::new(56.0, 22.0)),
                                                            )
                                                            .on_hover_cursor(
                                                                CursorIcon::PointingHand,
                                                            )
                                                            .on_hover_text(
                                                                "Tirar a permissão de falar",
                                                            );
                                                        if revoke_btn.clicked() {
                                                            cmds.push(
                                                                crate::net::room_tools::cmd_hand_action(
                                                                    "hand-revoke",
                                                                    sid,
                                                                ),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                            ui.add_space(2.0);
                                        }
                                    }
                                });
                            });
                    }
                });
        });

    if toggle_collapse {
        if let Some(ref mut rt) = app.room_tools {
            rt.hands_queue_collapsed = !rt.hands_queue_collapsed;
        }
    }
    for cmd in cmds {
        app.send_tool_json(cmd);
    }
}
