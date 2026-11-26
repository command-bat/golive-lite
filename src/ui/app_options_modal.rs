//! Menus flutuantes separados:
//! - `render_app_system_modal`: Opcoes do App (Sistema/Bandeja + Sobre/Updates), aberto pelos 3 pontos no topo.
//! - `render_user_settings_modal`: Configuracoes (Conta e Voz/Video), aberto pelo menu da conta ou engrenagem.

use crate::ui::theme::SemiBoldExt;
use egui::{Align, Color32, CursorIcon, Layout, RichText, Rounding, Stroke, Vec2};
use std::sync::atomic::Ordering;

use crate::app::{AppSystemTab, GoLiveLiteApp, UserSettingsTab};
#[cfg(feature = "mod_permissoes_chamada")]
use crate::config::MOD_PERMISSOES_CHAMADA;
use crate::config::vk_name;
use crate::media::audio::MicCapture;
use crate::media::audio_mixer::AudioMixer;
use crate::net::protocol::{
    format_points_br, resolve_name_color_rgb, room_category_label, verified_badge_tone, ShareFps,
    ShareResolution, ROOM_CATEGORIES,
};
use crate::ui::icons;
#[cfg(feature = "mod_permissoes_chamada")]
use crate::ui::manage_room_modal::ManageRoomView;
use crate::ui::theme::{
    centered_movable_window, BG_ELEVATED, BG_SURFACE, BG_TILE, BLUE, BORDER_SUBTLE, EMERALD,
    EMERALD_BRIGHT, GOLD, RED, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

fn render_segmented_tabs<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    items: &[(T, &str)],
) -> Option<T> {
    let mut switched = None;
    let total_w = ui.available_width();
    let count = items.len().max(1) as f32;
    let item_w = ((total_w - 8.0 - (count - 1.0) * 4.0) / count).max(80.0);

    egui::Frame::none()
        .fill(BG_TILE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(9.0))
        .inner_margin(egui::Margin::same(4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for &(val, label) in items {
                    let sel = *current == val;
                    let btn = egui::Button::new(
                        RichText::new(label)
                            .size(12.0)
                            .semibold()
                            .color(if sel { Color32::WHITE } else { TEXT_SECONDARY }),
                    )
                    .fill(if sel { BG_ELEVATED } else { Color32::TRANSPARENT })
                    .stroke(if sel {
                        Stroke::new(1.0_f32, BORDER_SUBTLE)
                    } else {
                        Stroke::NONE
                    })
                    .rounding(Rounding::same(7.0))
                    .min_size(Vec2::new(item_w, 28.0));
                    if ui.add(btn).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        *current = val;
                        switched = Some(val);
                    }
                }
            });
        });
    switched
}

/// Menu dos 3 pontos no topo: Sistema, Bandeja e Sobre / Atualizacoes.
pub fn render_app_system_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_app_system_modal {
        return;
    }

    let mut open = true;
    centered_movable_window(
        "Opcoes do App",
        "golive_app_system_modal",
        ctx,
        Vec2::new(372.0, 240.0),
    )
    .open(&mut open)
    .resizable(false)
    .default_width(372.0)
    .frame(
        egui::Frame::window(&ctx.style())
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(14.0))
            .inner_margin(egui::Margin::same(12.0)),
    )
    .show(ctx, |ui| {
        ui.set_width(348.0);
        ui.spacing_mut().item_spacing.y = 8.0;

        render_segmented_tabs(
            ui,
            &mut app.app_system_tab,
            &[
                (AppSystemTab::System, "Sistema e Bandeja"),
                (AppSystemTab::About, "Sobre / Updates"),
            ],
        );

        match app.app_system_tab {
            AppSystemTab::System => render_system_tab(app, ui, ctx),
            AppSystemTab::About => render_about_tab(app, ui),
        }
    });

    if !open {
        app.show_app_system_modal = false;
    }
}

/// Menu de Configuracoes (Conta e Audio/Video): compacto, sem sub-menus extras nem espacos vazios.
pub fn render_user_settings_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_user_settings_modal {
        return;
    }

    if app.user_settings_tab == UserSettingsTab::Room {
        app.user_settings_tab = UserSettingsTab::Account;
    }

    let mut open = true;
    centered_movable_window(
        "Configuracoes",
        "golive_user_settings_modal",
        ctx,
        Vec2::new(396.0, 270.0),
    )
    .open(&mut open)
    .resizable(false)
    .default_width(396.0)
    .frame(
        egui::Frame::window(&ctx.style())
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(14.0))
            .inner_margin(egui::Margin::same(12.0)),
    )
    .show(ctx, |ui| {
        ui.set_width(372.0);
        ui.spacing_mut().item_spacing.y = 8.0;

        let switched = render_segmented_tabs(
            ui,
            &mut app.user_settings_tab,
            &[
                (UserSettingsTab::Account, "Conta e Pontos"),
                (UserSettingsTab::Audio, "Voz e Video"),
            ],
        );
        if switched == Some(UserSettingsTab::Audio) {
            app.cached_mic_devices = MicCapture::list_input_devices();
            app.cached_speaker_devices = AudioMixer::list_output_devices();
        }

        match app.user_settings_tab {
            UserSettingsTab::Account => render_account_tab(app, ui),
            UserSettingsTab::Audio | UserSettingsTab::Room => render_audio_tab(app, ui),
        }
    });

    if !open {
        app.show_user_settings_modal = false;
    }
}

fn render_account_tab(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    let pts = app.effective_points();
    let tier = app.current_plan_tier();
    let flags = app.effective_self_flags().to_vec();
    let name_col = resolve_name_color_rgb(app.effective_self_name_color())
        .map(|[r, g, b]| Color32::from_rgb(r, g, b))
        .unwrap_or(TEXT_PRIMARY);

    egui::Frame::none()
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(app.effective_name())
                        .size(14.0)
                        .semibold()
                        .color(name_col),
                );
                if let Some(tone) = verified_badge_tone(&flags) {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                    icons::paint_verified_seal(ui.painter(), r.center(), 6.0, tone);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format_points_br(pts))
                            .size(13.5)
                            .semibold()
                            .color(GOLD),
                    );
                    let (cr, _) = ui.allocate_exact_size(Vec2::splat(15.0), egui::Sense::hover());
                    icons::paint_coin(ui.painter(), cr.center(), 13.0, GOLD);
                });
            });
            ui.add_space(2.0);
            let sub = if let Some(ref acc) = app.logged_account {
                format!(
                    "@{} • Plano: {} (ate {} telas)",
                    acc.username,
                    tier.label(),
                    tier.max_screens()
                )
            } else {
                format!("Convidado • Plano: {}", tier.label())
            };
            ui.label(RichText::new(sub).size(11.5).color(TEXT_SECONDARY));
        });

    if app.logged_account.is_some() {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Sair da conta").size(12.0).color(Color32::WHITE))
                        .fill(RED)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(112.0, 28.0)),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                app.logout_account();
            }
        });
    } else {
        egui::Frame::none()
            .fill(BG_TILE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Apelido:").size(12.0).color(TEXT_SECONDARY));
                    let w = ui.available_width();
                    if ui
                        .add_sized(
                            Vec2::new(w, 26.0),
                            egui::TextEdit::singleline(&mut app.config.display_name),
                        )
                        .changed()
                    {
                        app.config.save();
                    }
                });
                ui.label(
                    RichText::new("Entrar na conta Go Live (sincroniza Pro, Pontos e Selo)")
                        .size(11.5)
                        .color(TEXT_MUTED),
                );
                ui.horizontal(|ui| {
                    let field_w = ((ui.available_width() - 78.0) * 0.5).max(80.0);
                    ui.add_sized(
                        Vec2::new(field_w, 26.0),
                        egui::TextEdit::singleline(&mut app.login_username).hint_text("Usuario"),
                    );
                    ui.add_sized(
                        Vec2::new(field_w, 26.0),
                        egui::TextEdit::singleline(&mut app.login_password)
                            .password(true)
                            .hint_text("Senha"),
                    );
                    let can_login = !app.login_in_flight
                        && !app.login_username.trim().is_empty()
                        && !app.login_password.is_empty();
                    if ui
                        .add_enabled(
                            can_login,
                            egui::Button::new(RichText::new("Entrar").size(12.0).color(Color32::WHITE))
                                .fill(BLUE)
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(66.0, 26.0)),
                        )
                        .clicked()
                    {
                        app.start_account_login();
                    }
                });
                if let Some(ref lerr) = app.login_error {
                    ui.label(
                        RichText::new(lerr)
                            .size(11.0)
                            .color(Color32::from_rgb(252, 165, 165)),
                    );
                }
            });
    }
}

fn render_audio_tab(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;

            egui::Grid::new("settings_audio_video_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Microfone:").size(12.0).color(TEXT_SECONDARY));
                    let cur_mic = app
                        .config
                        .mic_device_name
                        .clone()
                        .unwrap_or_else(|| "Padrao do Sistema".to_string());
                    egui::ComboBox::from_id_salt("app_opt_mic_combo")
                        .selected_text(cur_mic)
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(app.config.mic_device_name.is_none(), "Padrao do Sistema")
                                .clicked()
                            {
                                app.config.mic_device_name = None;
                                app.config.save();
                                app.restart_mic_if_active();
                            }
                            let list = app.cached_mic_devices.clone();
                            for d in list {
                                let sel = app.config.mic_device_name.as_deref() == Some(d.as_str());
                                if ui.selectable_label(sel, &d).clicked() {
                                    app.config.mic_device_name = Some(d);
                                    app.config.save();
                                    app.restart_mic_if_active();
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(RichText::new("Saida (Fones):").size(12.0).color(TEXT_SECONDARY));
                    let cur_spk = app
                        .config
                        .speaker_device_name
                        .clone()
                        .unwrap_or_else(|| "Padrao do Sistema".to_string());
                    egui::ComboBox::from_id_salt("app_opt_spk_combo")
                        .selected_text(cur_spk)
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(app.config.speaker_device_name.is_none(), "Padrao do Sistema")
                                .clicked()
                            {
                                app.config.speaker_device_name = None;
                                app.config.save();
                                let _ = app.mixer.start(None);
                            }
                            let list = app.cached_speaker_devices.clone();
                            for d in list {
                                let sel = app.config.speaker_device_name.as_deref() == Some(d.as_str());
                                if ui.selectable_label(sel, &d).clicked() {
                                    app.config.speaker_device_name = Some(d.clone());
                                    app.config.save();
                                    let _ = app.mixer.start(Some(&d));
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(RichText::new("Resolucao:").size(12.0).color(TEXT_SECONDARY));
                    egui::ComboBox::from_id_salt("app_opt_res_inline")
                        .selected_text(app.config.share_resolution.label())
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            for res in ShareResolution::ALL {
                                let unlocked = app.can_use_resolution(res);
                                ui.add_enabled_ui(unlocked, |ui| {
                                    if ui
                                        .selectable_value(
                                            &mut app.config.share_resolution,
                                            res,
                                            res.label(),
                                        )
                                        .changed()
                                    {
                                        app.config.save();
                                    }
                                });
                            }
                        });
                    ui.end_row();

                    ui.label(RichText::new("Taxa de quadros:").size(12.0).color(TEXT_SECONDARY));
                    egui::ComboBox::from_id_salt("app_opt_fps_inline")
                        .selected_text(app.config.share_fps.label())
                        .width(230.0)
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
                });

            let mut gain_pct = app.config.mic_gain * 100.0;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Ganho mic: {}%", gain_pct.round() as i32))
                        .size(12.0)
                        .color(TEXT_SECONDARY),
                );
                let gs = ui.add(
                    egui::Slider::new(&mut gain_pct, 1.0..=200.0)
                        .show_value(false),
                );
                if gs.double_clicked() {
                    app.config.mic_gain = 1.0;
                    app.mic.state.set_gain(1.0);
                    app.config.save();
                } else if gs.changed() {
                    app.config.mic_gain = gain_pct / 100.0;
                    app.mic.state.set_gain(app.config.mic_gain);
                    app.config.save();
                }
            });

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

            if ui
                .checkbox(
                    &mut app.config.share_audio,
                    "Compartilhar som do sistema por padrao",
                )
                .changed()
            {
                app.config.save();
            }
        });
}

pub fn render_room_settings_panel(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    if app.current_room.is_none() {
        ui.label(RichText::new("Entre em uma sala para configurar.").color(TEXT_MUTED));
        return;
    }
    let is_mgr = app.is_room_manager();
    let has_media = !app.active_screen_slots.is_empty()
        || app.camera_active
        || app.has_active_media_tools()
        || app
            .peers
            .iter()
            .any(|p| p.sharing || p.camera.unwrap_or(false) || p.screen.unwrap_or(false) || !p.files.is_empty());

    ui.label(
        RichText::new("Descricao e Categoria")
            .size(12.5)
            .semibold()
            .color(TEXT_PRIMARY),
    );
    if is_mgr {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Categoria:").size(12.0).color(TEXT_SECONDARY));
            let selected_lbl =
                room_category_label(app.room_category.as_deref()).unwrap_or("Sem categoria");
            egui::ComboBox::from_id_salt("usr_room_cat_combo")
                .selected_text(selected_lbl)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(app.room_category.is_none(), "Sem categoria")
                        .clicked()
                    {
                        app.room_category = None;
                        app.save_room_info(None, Some(None));
                    }
                    for &(cid, clabel) in ROOM_CATEGORIES {
                        let sel = app.room_category.as_deref() == Some(cid);
                        if ui.selectable_label(sel, clabel).clicked() {
                            app.room_category = Some(cid.to_string());
                            app.save_room_info(None, Some(Some(cid.to_string())));
                        }
                    }
                });
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 78.0).max(160.0);
            ui.add_sized(
                Vec2::new(w, 28.0),
                egui::TextEdit::singleline(&mut app.room_description_draft)
                    .char_limit(120)
                    .hint_text("Descricao da sala (ate 120 chars)"),
            );
            if ui
                .add(
                    egui::Button::new(RichText::new("Salvar").size(12.0).color(Color32::WHITE))
                        .fill(BLUE)
                        .rounding(Rounding::same(7.0))
                        .min_size(Vec2::new(68.0, 28.0)),
                )
                .clicked()
            {
                let trimmed = app.room_description_draft.trim().to_string();
                app.room_description = trimmed.clone();
                app.save_room_info(Some(trimmed), None);
            }
        });
    } else {
        let cat_lbl =
            room_category_label(app.room_category.as_deref()).unwrap_or("Sem categoria");
        let desc_lbl = if app.room_description.trim().is_empty() {
            "Sem descricao definida pelo dono."
        } else {
            app.room_description.trim()
        };
        ui.label(
            RichText::new(format!("Categoria: {}", cat_lbl))
                .size(11.5)
                .color(TEXT_SECONDARY),
        );
        ui.label(RichText::new(desc_lbl).size(11.5).color(TEXT_MUTED));
    }

    ui.add_space(6.0);
    ui.label(
        RichText::new("Exibicao e Otimizacao na Sala")
            .size(12.5)
            .semibold()
            .color(TEXT_PRIMARY),
    );
    if ui
        .checkbox(
            &mut app.config.grid_video_preview,
            "Ativar previa de video na grade (desligado = Modo Eco)",
        )
        .changed()
    {
        if !app.config.grid_video_preview {
            app.enabled_grid_preview_tiles.clear();
        }
        app.config.save();
    }

    if has_media {
        let mut show_ppl = !app.left_sidebar_collapsed;
        if ui
            .checkbox(&mut show_ppl, "Mostrar painel de Participantes e Parceiro")
            .changed()
        {
            app.left_sidebar_collapsed = !show_ppl;
        }

        let mut show_chat = !app.right_sidebar_collapsed;
        if ui
            .checkbox(&mut show_chat, "Mostrar painel lateral de Chat / Sala")
            .changed()
        {
            app.right_sidebar_collapsed = !show_chat;
        }
    }

    ui.checkbox(
        &mut app.chat_notifications_muted,
        "Silenciar avisos visuais de mensagens novas no chat",
    );

    #[cfg(feature = "mod_permissoes_chamada")]
    if app.has_modification(MOD_PERMISSOES_CHAMADA) && is_mgr {
        ui.add_space(6.0);
        if ui
            .add(
                egui::Button::new(
                    RichText::new("Gerenciar sala e permissoes")
                        .size(12.5)
                        .semibold()
                        .color(Color32::WHITE),
                )
                .fill(EMERALD)
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(ui.available_width(), 32.0)),
            )
            .clicked()
        {
            if let Some(ref mut m) = app.manage_room_mod {
                m.manage_room_view = ManageRoomView::Menu;
                m.show_manage_room_modal = true;
            }
        }

        let manageable: Vec<_> = app
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
        if !manageable.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Moderacao rapida")
                    .size(12.0)
                    .semibold()
                    .color(TEXT_PRIMARY),
            );
            for p in manageable {
                let Some(uid) = p.user_id.clone() else {
                    continue;
                };
                let silenced = app.is_peer_silenced(Some(&uid));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&p.name).size(12.0).color(TEXT_PRIMARY));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Banir").size(10.5).color(Color32::WHITE),
                                )
                                .fill(RED)
                                .rounding(Rounding::same(5.0)),
                            )
                            .clicked()
                        {
                            app.room_ban_peer(&uid);
                        }
                        if ui.small_button("Expulsar").clicked() {
                            app.room_kick_peer(&uid);
                        }
                        if ui
                            .small_button(if silenced { "Dessilenciar" } else { "Silenciar" })
                            .clicked()
                        {
                            app.room_silence_peer(&uid, !silenced);
                        }
                    });
                });
            }
        }
    }
}

fn render_system_tab(app: &mut GoLiveLiteApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    egui::Frame::none()
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;

            let mut autostart = app.config.start_with_windows;
            if ui
                .checkbox(&mut autostart, "Iniciar com o Windows (escondido na bandeja)")
                .changed()
            {
                app.set_start_with_windows(autostart);
            }

            let mut close_to_tray = app.config.minimize_to_tray_on_close;
            if ui
                .checkbox(&mut close_to_tray, "Botao fechar (X) minimiza para a bandeja")
                .on_hover_text("Desmarque para encerrar completamente o aplicativo ao clicar no X.")
                .changed()
            {
                app.set_minimize_to_tray_on_close(close_to_tray);
            }
        });

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add(
                egui::Button::new(
                    RichText::new("Sair do Go Live - Lite")
                        .size(12.0)
                        .color(Color32::WHITE),
                )
                .fill(RED)
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(150.0, 28.0)),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            app.request_real_quit(ctx);
        }
    });
}

fn render_about_tab(app: &mut GoLiveLiteApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Go Live - Lite")
                        .size(14.5)
                        .semibold()
                        .color(TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(format!("v{}", crate::net::updater::APP_VERSION))
                        .size(12.0)
                        .color(EMERALD_BRIGHT),
                );
            });
            ui.label(
                RichText::new("Compativel com Windows 11, Windows 10 e Windows 8/8.1.")
                    .size(11.5)
                    .color(TEXT_SECONDARY),
            );

            let has_any_mod_feature = cfg!(feature = "mod_ferramentas")
                || cfg!(feature = "mod_permissoes_chamada")
                || cfg!(feature = "mod_perfil");
            let mods_summary = if !has_any_mod_feature {
                "Nucleo puro Ultra-Leve (0 modificacoes no disco e na memoria)".to_string()
            } else if app.config.enabled_modifications.is_empty() {
                "Modo Ultra-Leve (nenhuma modificacao extra ativa)".to_string()
            } else {
                format!("Modificacoes: {}", app.config.enabled_modifications.join(", "))
            };
            ui.label(RichText::new(mods_summary).size(11.5).color(EMERALD_BRIGHT));
        });

    ui.horizontal(|ui| {
        let checking = matches!(
            app.update_state,
            crate::net::updater::UpdateDownloadState::Checking
        );
        if ui
            .add_enabled(
                !checking,
                egui::Button::new(
                    RichText::new(if checking {
                        "Verificando..."
                    } else {
                        "Verificar atualizacoes"
                    })
                    .size(12.0)
                    .color(Color32::WHITE),
                )
                .fill(EMERALD)
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(150.0, 28.0)),
            )
            .clicked()
        {
            app.trigger_manual_update_check();
        }
        match &app.update_state {
            crate::net::updater::UpdateDownloadState::UpToDate => {
                ui.label(RichText::new("Atualizado!").size(11.5).color(EMERALD_BRIGHT));
            }
            crate::net::updater::UpdateDownloadState::Error(err) => {
                ui.label(
                    RichText::new(err)
                        .size(11.0)
                        .color(Color32::from_rgb(252, 165, 165)),
                );
            }
            _ => {}
        }
    });
}
