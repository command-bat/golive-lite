use crate::ui::theme::SemiBoldExt;
use egui::{pos2, Align, Color32, CursorIcon, Layout, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::{GoLiveLiteApp, UserSettingsTab};
#[cfg(feature = "mod_perfil")]
use crate::config::MOD_PERFIL;
use crate::net::protocol::{verified_badge_tone, VerifiedTone};
use crate::ui::app_options_modal::{render_app_system_modal, render_user_settings_modal};
use crate::ui::icons;
#[cfg(feature = "mod_perfil")]
use crate::ui::profile_modal::render_profile_modal;
use crate::ui::theme::{
    BG_ELEVATED, BG_MAIN, BG_SURFACE, BG_TILE, BORDER_SUBTLE,
    TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

pub fn render_home_view(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    crate::ui::top_banners::render_top_warns(app, ctx);
    crate::ui::top_banners::render_alerts_center(app, ctx);
    render_app_system_modal(app, ctx);
    render_user_settings_modal(app, ctx);
    crate::ui::quality_modal::render_stream_quality_modal(app, ctx);
    #[cfg(feature = "mod_perfil")]
    render_profile_modal(app, ctx);

    if let Some(blocked_room) = app.protected_room_blocked.clone() {
        render_protected_room_blocker(app, ctx, &blocked_room);
        return;
    }

    if app.logged_account.is_none() {
        app.home_user_menu_open = false;
    } else if app.show_login_box {
        app.show_login_box = false;
    }

    render_home_login_modal(app, ctx);

    // Cabecalho, cartao, paineis e rodape: copia da home do site (ui/home_site.rs).
    crate::ui::home_site::render(app, ctx);
}

/// O menu da conta (usado pelo cabecalho da home).
pub(crate) fn render_home_user_dropdown_pub(
    app: &mut GoLiveLiteApp,
    ctx: &egui::Context,
    anchor: Rect,
    toggled_this_frame: bool,
) {
    render_home_user_dropdown(app, ctx, anchor, toggled_this_frame);
}

fn render_home_user_dropdown(
    app: &mut GoLiveLiteApp,
    ctx: &egui::Context,
    anchor: Rect,
    toggled_this_frame: bool,
) {
    let Some((username, badge_tone)) = app
        .logged_account
        .as_ref()
        .map(|a| (a.username.clone(), verified_badge_tone(&a.flags)))
    else {
        app.home_user_menu_open = false;
        return;
    };

    let area_pos = pos2(anchor.left().max(8.0), anchor.bottom() + 6.0);
    let area_resp = egui::Area::new(egui::Id::new("golive_home_user_dropdown"))
        .order(egui::Order::Foreground)
        .fixed_pos(area_pos)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgb(22, 27, 34))
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    ui.set_width(204.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;

                        egui::Frame::none()
                            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Conectado como")
                                        .size(11.5)
                                        .color(TEXT_MUTED),
                                );
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    ui.label(
                                        RichText::new(&username)
                                            .size(13.5)
                                            .semibold()
                                            .color(TEXT_PRIMARY),
                                    );
                                    if let Some(tone) = badge_tone {
                                        let (r, resp) = ui.allocate_exact_size(
                                            Vec2::new(13.0, 14.0),
                                            Sense::hover(),
                                        );
                                        icons::paint_verified_seal(
                                            ui.painter(),
                                            r.center(),
                                            5.4,
                                            tone,
                                        );
                                        let tip = match tone {
                                            VerifiedTone::Ruby => "Go Live Ultra",
                                            VerifiedTone::Gold => "Go Live Max",
                                            VerifiedTone::Blue => "Go Live Pro / Verificado",
                                        };
                                        resp.on_hover_text(tip);
                                    }
                                });
                            });

                        ui.separator();

                        #[cfg(feature = "mod_perfil")]
                        if app.has_modification(MOD_PERFIL)
                            && render_dropdown_item(
                                ui,
                                "Meu perfil",
                                Some(icons::paint_user_outline),
                            )
                        {
                            app.home_user_menu_open = false;
                            app.open_self_profile();
                        }

                        // "Amigos" logo depois do perfil, como no menu da conta do site.
                        #[cfg(feature = "mod_amigos")]
                        if app.has_modification(crate::config::MOD_AMIGOS) {
                            let pending = crate::ui::friends_view::incoming_badge(app);
                            let label = if pending > 0 {
                                format!("Amigos  ({})", pending)
                            } else {
                                "Amigos".to_string()
                            };
                            if render_dropdown_item(ui, &label, Some(icons::paint_user_plus)) {
                                app.home_user_menu_open = false;
                                if app.amigos.is_none() {
                                    app.start_amigos();
                                }
                                if let Some(st) = app.amigos.as_mut() {
                                    st.page_open = true;
                                }
                            }
                        }

                        if render_dropdown_item(ui, "Configurações", Some(icons::paint_gear)) {
                            app.home_user_menu_open = false;
                            app.user_settings_tab = UserSettingsTab::Account;
                            app.show_user_settings_modal = true;
                        }

                        if render_dropdown_item(
                            ui,
                            "Sair da conta",
                            None::<fn(&egui::Painter, egui::Pos2, f32, Color32)>,
                        ) {
                            app.home_user_menu_open = false;
                            app.logout_account();
                        }
                    });
                });
        });

    if !toggled_this_frame {
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let outside_click = ctx.input(|i| {
            i.pointer.primary_clicked()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !area_resp.response.rect.contains(p) && !anchor.contains(p)
                })
        });
        if esc || outside_click {
            app.home_user_menu_open = false;
        }
    }
}

fn render_dropdown_item<F>(ui: &mut egui::Ui, label: &str, icon_fn: Option<F>) -> bool
where
    F: FnOnce(&egui::Painter, egui::Pos2, f32, Color32),
{
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, Rounding::same(8.0), Color32::from_rgb(42, 49, 60));
    }
    if let Some(paint) = icon_fn {
        paint(
            ui.painter(),
            pos2(rect.left() + 16.0, rect.center().y),
            13.5,
            TEXT_SECONDARY,
        );
        ui.painter().text(
            pos2(rect.left() + 32.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            TEXT_PRIMARY,
        );
    } else {
        ui.painter().text(
            pos2(rect.left() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            TEXT_PRIMARY,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// "Entrar na conta": copia do `AccountModal` + `LoginForm` do site (fundo `bg-black/50`,
/// cartao `max-w-md rounded-2xl p-8 bg-zinc-950`, campos `h-11 rounded-lg`, botao claro).
/// Criar conta e "Esqueci minha senha" abrem o site (com o aviso de link externo).
fn render_home_login_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.show_login_box || app.logged_account.is_some() {
        return;
    }
    const ZINC_50: Color32 = Color32::from_rgb(250, 250, 250);
    const ZINC_300: Color32 = Color32::from_rgb(212, 212, 216);
    const ZINC_900_FIELD: Color32 = Color32::from_rgb(28, 34, 43);
    const ZINC_700: Color32 = Color32::from_rgb(58, 66, 78);
    let screen = ctx.screen_rect();
    let mut close = false;

    // Fundo: clique fora fecha.
    let backdrop = egui::Area::new(egui::Id::new("golive_login_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, Rounding::ZERO, Color32::from_black_alpha(128));
            resp.clicked()
        })
        .inner;
    if backdrop || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }

    let w = (screen.width() - 32.0).min(448.0);
    let show_pw_id = egui::Id::new("golive_login_show_password");
    let mut show_pw = ctx.data(|d| d.get_temp::<bool>(show_pw_id)).unwrap_or(false);
    let mut submit = false;
    let mut open_url: Option<&'static str> = None;

    let area = egui::Area::new(egui::Id::new("golive_login_modal"))
        .order(egui::Order::Tooltip)
        .pivot(egui::Align2::CENTER_CENTER)
        .fixed_pos(screen.center())
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(26)))
                .rounding(Rounding::same(16.0))
                .inner_margin(egui::Margin::same(32.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 12.0),
                    blur: 32.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(110),
                })
                .show(ui, |ui| {
                    ui.set_width(w - 64.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("Entrar na conta").size(20.0).semibold().color(ZINC_50));
                    ui.add_space(24.0);

                    // Campo: rotulo `text-sm font-medium text-zinc-300` + `h-11 rounded-lg`
                    let field = |ui: &mut egui::Ui, text: &mut String, password: bool, id: &str, invalid: bool| {
                        let rect = ui.allocate_space(Vec2::new(ui.available_width(), 44.0)).1;
                        let has_focus = ui.memory(|m| m.has_focus(egui::Id::new(id)));
                        let border = if invalid {
                            Color32::from_rgb(153, 27, 27)
                        } else if has_focus {
                            Color32::from_rgb(113, 113, 122)
                        } else {
                            ZINC_700
                        };
                        ui.painter().rect(rect, Rounding::same(8.0), ZINC_900_FIELD, Stroke::new(1.0_f32, border));
                        let right_pad = if password { 44.0 } else { 14.0 };
                        let edit_rect = Rect::from_min_max(
                            pos2(rect.left() + 14.0, rect.top() + 4.0),
                            pos2(rect.right() - right_pad, rect.bottom() - 4.0),
                        );
                        let resp = ui.put(
                            edit_rect,
                            egui::TextEdit::singleline(text)
                                .id(egui::Id::new(id))
                                .frame(false)
                                .password(password)
                                .font(egui::FontId::proportional(16.0))
                                .text_color(ZINC_50)
                                .vertical_align(Align::Center),
                        );
                        (rect, resp)
                    };

                    ui.label(RichText::new("Usuário ou e-mail").size(14.0).semibold().color(ZINC_300));
                    ui.add_space(6.0);
                    let invalid = app.login_error.is_some();
                    let (_, user_resp) = field(ui, &mut app.login_username, false, "golive_login_user", invalid);
                    if user_resp.changed() {
                        app.login_error = None;
                    }
                    if !user_resp.has_focus()
                        && app.login_username.is_empty()
                        && !ui.memory(|m| m.has_focus(egui::Id::new("golive_login_pass")))
                    {
                        user_resp.request_focus();
                    }
                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Senha").size(14.0).semibold().color(ZINC_300));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let link = ui.add(
                                egui::Label::new(RichText::new("Esqueci minha senha").size(14.0).semibold().color(TEXT_PRIMARY))
                                    .sense(Sense::click()),
                            );
                            if link.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                open_url = Some("https://golive.nemtudo.me/login");
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let (pw_rect, pass_resp) = field(ui, &mut app.login_password, !show_pw, "golive_login_pass", invalid);
                    if pass_resp.changed() {
                        app.login_error = None;
                    }
                    // Olho de mostrar/esconder (PasswordInput do site)
                    let eye = Rect::from_center_size(pos2(pw_rect.right() - 22.0, pw_rect.center().y), Vec2::splat(28.0));
                    let eye_resp = ui.interact(eye, egui::Id::new("golive_login_eye"), Sense::click());
                    icons::paint_eye(ui.painter(), eye.center(), 16.0, TEXT_MUTED, show_pw);
                    if eye_resp
                        .on_hover_text(if show_pw { "Esconder senha" } else { "Mostrar senha" })
                        .clicked()
                    {
                        show_pw = !show_pw;
                    }
                    if let Some(ref err) = app.login_error {
                        ui.add_space(6.0);
                        ui.label(RichText::new(err).size(14.0).color(Color32::from_rgb(248, 113, 113)));
                    }
                    let enter = (pass_resp.lost_focus() || user_resp.lost_focus())
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.add_space(20.0);

                    // Botao `h-11 rounded-lg bg-zinc-50 text-zinc-950`
                    let can = !app.login_in_flight && !app.login_username.trim().is_empty() && !app.login_password.is_empty();
                    let (br, bresp) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 44.0),
                        if can { Sense::click() } else { Sense::hover() },
                    );
                    let fill = if !can {
                        ZINC_50.gamma_multiply(0.5)
                    } else if bresp.hovered() {
                        Color32::from_rgb(228, 228, 231)
                    } else {
                        ZINC_50
                    };
                    ui.painter().rect_filled(br, Rounding::same(8.0), fill);
                    ui.painter().text(
                        br.center(),
                        egui::Align2::CENTER_CENTER,
                        if app.login_in_flight { "Entrando..." } else { "Entrar" },
                        egui::FontId::new(16.0, egui::FontFamily::Name(crate::ui::theme::SEMIBOLD.into())),
                        Color32::from_rgb(9, 9, 11),
                    );
                    if can && (bresp.on_hover_cursor(CursorIcon::PointingHand).clicked() || enter) {
                        submit = true;
                    }
                    ui.add_space(16.0);

                    // "Não tem uma conta? Criar conta"
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            let prompt = "Não tem uma conta?";
                            let action = "Criar conta";
                            let pw = ui.fonts(|f| {
                                f.layout_no_wrap(prompt.into(), egui::FontId::proportional(14.0), TEXT_MUTED).size().x
                                    + f.layout_no_wrap(action.into(), egui::FontId::proportional(14.0), TEXT_MUTED).size().x
                            }) + 4.0;
                            ui.add_space(((ui.available_width() - pw) / 2.0).max(0.0));
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(prompt).size(14.0).color(TEXT_MUTED));
                            let link = ui.add(
                                egui::Label::new(RichText::new(action).size(14.0).semibold().color(TEXT_PRIMARY))
                                    .sense(Sense::click()),
                            );
                            if link.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                open_url = Some("https://golive.nemtudo.me/login");
                            }
                        });
                        ui.add_space(12.0);
                        let back = ui.add(
                            egui::Label::new(RichText::new("Voltar").size(14.0).color(TEXT_MUTED)).sense(Sense::click()),
                        );
                        if back.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            close = true;
                        }
                    });
                });
        });
    // O cartao "engole" o clique (o fundo atras nao fecha quando se clica nele).
    if backdrop && area.response.rect.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())) {
        close = false;
    }

    ctx.data_mut(|d| d.insert_temp(show_pw_id, show_pw));
    if let Some(url) = open_url {
        crate::ui::external_link::request(url);
    }
    if submit {
        app.start_account_login();
    }
    if close {
        app.show_login_box = false;
        app.login_error = None;
    }
}

fn render_protected_room_blocker(
    app: &mut GoLiveLiteApp,
    ctx: &egui::Context,
    blocked_room: &str,
) {
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG_MAIN))
        .show(ctx, |ui| {
            let avail = ui.available_size();
            let card_w = 410.0_f32.min((avail.x - 32.0).max(280.0));
            let top_pad = ((avail.y - 340.0) * 0.44).max(24.0);
            ui.add_space(top_pad);
            ui.horizontal(|ui| {
                ui.add_space(((avail.x - card_w) * 0.5).max(8.0));
                egui::Frame::none()
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .rounding(Rounding::same(16.0))
                    .inner_margin(egui::Margin::symmetric(24.0, 26.0))
                    .show(ui, |ui| {
                        ui.set_width(card_w - 48.0);
                        ui.vertical_centered(|ui| {
                            let (lr, _) =
                                ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                            ui.painter().circle_filled(
                                lr.center(),
                                26.0,
                                Color32::from_rgb(28, 34, 43),
                            );
                            ui.painter().circle_stroke(
                                lr.center(),
                                26.0,
                                Stroke::new(1.0_f32, Color32::from_rgb(67, 74, 128)),
                            );
                            icons::paint_lock(
                                ui.painter(),
                                lr.center(),
                                22.0,
                                Color32::from_rgb(165, 180, 252),
                            );

                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("Sala protegida")
                                    .size(21.0)
                                    .semibold()
                                    .color(TEXT_PRIMARY),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Esta sala exige o aplicativo oficial do Go Live com protecao contra captura.\nO Go Live - Lite nao entra em salas protegidas.",
                                )
                                .size(12.5)
                                .color(TEXT_SECONDARY),
                            );

                            ui.add_space(16.0);
                            let open_w = ui.available_width();
                            let (open_rect, open_resp) = ui.allocate_exact_size(
                                Vec2::new(open_w, 40.0),
                                Sense::click(),
                            );
                            let open_bg = if open_resp.hovered() {
                                Color32::from_rgb(232, 236, 244)
                            } else {
                                Color32::WHITE
                            };
                            ui.painter()
                                .rect_filled(open_rect, Rounding::same(10.0), open_bg);
                            icons::paint_windows_logo(
                                ui.painter(),
                                pos2(open_rect.center().x - 52.0, open_rect.center().y),
                                15.0,
                                Color32::from_rgb(22, 27, 34),
                            );
                            ui.painter().text(
                                pos2(open_rect.center().x + 8.0, open_rect.center().y),
                                egui::Align2::CENTER_CENTER,
                                "Abrir no app",
                                egui::FontId::proportional(13.5),
                                Color32::from_rgb(22, 27, 34),
                            );
                            if open_resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                let slug = crate::net::auth::encode_query_component(blocked_room);
                                crate::ui::external_link::request(&format!(
                                    "https://golive.nemtudo.me/watch/{}",
                                    slug
                                ));
                            }

                            ui.add_space(8.0);
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Baixar para Windows")
                                            .size(13.0)
                                            .semibold()
                                            .color(TEXT_PRIMARY),
                                    )
                                    .fill(BG_ELEVATED)
                                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                    .rounding(Rounding::same(10.0))
                                    .min_size(Vec2::new(open_w, 38.0)),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                crate::ui::external_link::request("https://golive.nemtudo.me/download");
                            }

                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(10.0);

                            ui.horizontal(|ui| {
                                let half = (ui.available_width() - 8.0) * 0.5;
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Inicio").size(12.5).color(TEXT_PRIMARY),
                                        )
                                        .fill(BG_TILE)
                                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(half, 34.0)),
                                    )
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    app.protected_room_blocked = None;
                                    app.home_error = None;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Outras salas")
                                                .size(12.5)
                                                .color(TEXT_PRIMARY),
                                        )
                                        .fill(BG_TILE)
                                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(half, 34.0)),
                                    )
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    crate::ui::external_link::request("https://golive.nemtudo.me/rooms");
                                }
                            });

                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                ui.add_space(((ui.available_width() - 250.0) * 0.5).max(0.0));
                                ui.label(
                                    RichText::new("Nao consegue abrir?")
                                        .size(11.5)
                                        .color(TEXT_MUTED),
                                );
                                if ui
                                    .link(
                                        RichText::new("Fale no Discord")
                                            .size(11.5)
                                            .color(Color32::from_rgb(129, 140, 248)),
                                    )
                                    .clicked()
                                {
                                    crate::ui::external_link::request("https://discord.gg/golive");
                                }
                            });
                        });
                    });
            });
        });
}
