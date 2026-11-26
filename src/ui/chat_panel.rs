use crate::ui::theme::SemiBoldExt;
use egui::{Color32, CursorIcon, RichText, Rounding, Sense, Stroke, Vec2};

use crate::net::protocol::{resolve_name_color_rgb, verified_badge_tone, ChatMessage, ChatReplyTo};
#[cfg(feature = "mod_ferramentas")]
use crate::net::room_tools::{
    can_use_tool, cmd_close_tool, cmd_task_add_full, cmd_task_clear_done, cmd_task_remove,
    cmd_task_toggle, task_allows, RoomToolsState, ToolPayload,
};
use crate::ui::icons;
#[cfg(feature = "mod_ferramentas")]
use crate::ui::theme::{BG_TILE, EMERALD_BRIGHT, GOLD};
use crate::ui::theme::{
    BG_ELEVATED, BG_SURFACE, BLUE, BLUE_LIGHT, BORDER_SUBTLE, EMERALD, RED, TEXT_MUTED,
    TEXT_PRIMARY, TEXT_SECONDARY,
};

#[cfg(feature = "mod_ferramentas")]
pub type ChatToolsRef<'a> = &'a mut RoomToolsState;
#[cfg(not(feature = "mod_ferramentas"))]
pub type ChatToolsRef<'a> = &'a mut ();

#[derive(Default)]
pub struct ChatPanelAction {
    pub send_text: Option<String>,
    pub reply_to: Option<ChatReplyTo>,
    pub clear_reply: bool,
    pub typing_changed: Option<bool>,
    #[cfg(feature = "mod_ferramentas")]
    pub poll_vote: Option<(String, Vec<String>)>,
    #[cfg(feature = "mod_ferramentas")]
    pub poll_close: Option<String>,
    #[cfg(feature = "mod_ferramentas")]
    pub create_poll: Option<(String, Vec<String>, bool)>,
    #[cfg(feature = "mod_ferramentas")]
    pub tool_cmds: Vec<serde_json::Value>,
}

pub fn render_chat_panel(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    typing_names: &[String],
    blocked_notice: Option<&str>,
    draft: &mut String,
    reply_target: &mut Option<ChatReplyTo>,
    was_typing: &mut bool,
    self_id: Option<&str>,
    mut tools: Option<ChatToolsRef<'_>>,
    self_user_id: Option<&str>,
    is_manager: bool,
    can_create_tools: bool,
) -> ChatPanelAction {
    let mut act = ChatPanelAction::default();
    #[cfg(not(feature = "mod_ferramentas"))]
    let _ = (&mut tools, self_user_id, is_manager, can_create_tools);

    egui::Frame::none()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(20.0)) // data-rr-card rounded-xl
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            if let Some(err) = blocked_notice {
                egui::Frame::none()
                    .fill(RED.linear_multiply(0.18))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(6.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(err)
                                .size(11.5)
                                .color(Color32::from_rgb(252, 165, 165)),
                        );
                    });
                ui.add_space(4.0);
            }

            #[cfg(feature = "mod_ferramentas")]
            let mut inline_tasks_h = 0.0_f32;
            #[cfg(not(feature = "mod_ferramentas"))]
            let inline_tasks_h = 0.0_f32;

            #[cfg(feature = "mod_ferramentas")]
            if let Some(ref mut st) = tools {
                let task_snap = st.tools.iter().find_map(|t| {
                    if let ToolPayload::Tasks { ref items, ref rules } = t.payload {
                        Some((
                            t.id.clone(),
                            t.title.clone(),
                            t.created_by_id.clone(),
                            items.clone(),
                            rules.clone(),
                        ))
                    } else {
                        None
                    }
                });
                if let Some((tid, title, created_by_id, items, rules)) = task_snap {
                    let done = items.iter().filter(|i| i.done).count();
                    let total = items.len();
                    let frac = if total == 0 {
                        0.0
                    } else {
                        (done as f32 / total as f32).clamp(0.0, 1.0)
                    };
                    let bar_w = ui.available_width();
                    let (bar_rect, bar_resp) =
                        ui.allocate_exact_size(Vec2::new(bar_w, 28.0), egui::Sense::click());
                    let bar_bg = if bar_resp.hovered() {
                        Color32::from_rgb(17, 56, 42)
                    } else {
                        Color32::from_rgb(13, 44, 34)
                    };
                    ui.painter().rect(
                        bar_rect,
                        Rounding::same(8.0),
                        bar_bg,
                        Stroke::new(1.0_f32, Color32::from_rgb(20, 83, 56)),
                    );
                    icons::paint_tool_kind_icon(
                        ui.painter(),
                        egui::pos2(bar_rect.left() + 13.0, bar_rect.center().y - 1.0),
                        12.5,
                        "tasks",
                        EMERALD_BRIGHT,
                    );
                    let hdr_lbl = if total == 0 {
                        title.clone()
                    } else {
                        format!("{} • {}/{} ({:.0}%)", title, done, total, frac * 100.0)
                    };
                    ui.painter().text(
                        egui::pos2(bar_rect.left() + 26.0, bar_rect.center().y - 1.5),
                        egui::Align2::LEFT_CENTER,
                        hdr_lbl,
                        egui::FontId::proportional(11.5),
                        EMERALD_BRIGHT,
                    );
                    icons::paint_chevron_down(
                        ui.painter(),
                        egui::pos2(bar_rect.right() - 12.0, bar_rect.center().y - 1.0),
                        9.0,
                        TEXT_SECONDARY,
                    );
                    let prog_track = egui::Rect::from_min_max(
                        egui::pos2(bar_rect.left() + 6.0, bar_rect.bottom() - 4.0),
                        egui::pos2(bar_rect.right() - 6.0, bar_rect.bottom() - 1.5),
                    );
                    ui.painter().rect_filled(
                        prog_track,
                        Rounding::same(1.5),
                        Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                    );
                    if frac > 0.0 {
                        let prog_fill = egui::Rect::from_min_max(
                            prog_track.min,
                            egui::pos2(
                                prog_track.left() + prog_track.width() * frac,
                                prog_track.max.y,
                            ),
                        );
                        ui.painter()
                            .rect_filled(prog_fill, Rounding::same(1.5), EMERALD_BRIGHT);
                    }
                    if bar_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Clique para ver ou editar tarefas aqui no chat")
                        .clicked()
                    {
                        st.chat_tasks_open = !st.chat_tasks_open;
                    }
                    ui.add_space(4.0);

                    if st.chat_tasks_open {
                        let can_add = task_allows(&rules, "add", self_user_id, is_manager, None);
                        let can_edit_any =
                            task_allows(&rules, "edit", self_user_id, is_manager, None);
                        let can_close = is_manager
                            || (!created_by_id.is_empty()
                                && self_user_id == Some(created_by_id.as_str()));

                        let card_resp = egui::Frame::none()
                            .fill(BG_TILE)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .rounding(Rounding::same(9.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{}/{} concluidas", done, total))
                                            .size(10.5)
                                            .color(TEXT_MUTED),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if can_close
                                                && ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Fechar")
                                                                .size(10.0)
                                                                .color(Color32::from_rgb(
                                                                    252, 165, 165,
                                                                )),
                                                        )
                                                        .fill(Color32::TRANSPARENT),
                                                    )
                                                    .on_hover_text("Encerrar tarefas da sala")
                                                    .clicked()
                                            {
                                                act.tool_cmds.push(cmd_close_tool(&tid));
                                                st.chat_tasks_open = false;
                                            }
                                            if can_edit_any
                                                && done > 0
                                                && ui
                                                    .small_button("Limpar feitas")
                                                    .clicked()
                                            {
                                                act.tool_cmds.push(cmd_task_clear_done(&tid));
                                            }
                                        },
                                    );
                                });
                                ui.add_space(3.0);

                                if items.is_empty() {
                                    ui.label(
                                        RichText::new("Nenhuma tarefa ainda.")
                                            .size(11.0)
                                            .color(TEXT_MUTED),
                                    );
                                } else {
                                    egui::ScrollArea::vertical()
                                        .id_salt(("chat_inline_tasks", &tid))
                                        .max_height(132.0)
                                        .show(ui, |ui| {
                                            for item in &items {
                                                let can_toggle = task_allows(
                                                    &rules,
                                                    "complete",
                                                    self_user_id,
                                                    is_manager,
                                                    Some(item),
                                                );
                                                let can_remove = task_allows(
                                                    &rules,
                                                    "edit",
                                                    self_user_id,
                                                    is_manager,
                                                    Some(item),
                                                );
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    let (cb_r, cb_resp) = ui.allocate_exact_size(
                                                        Vec2::splat(15.0),
                                                        if can_toggle {
                                                            egui::Sense::click()
                                                        } else {
                                                            egui::Sense::hover()
                                                        },
                                                    );
                                                    ui.painter().rect(
                                                        cb_r,
                                                        Rounding::same(3.5),
                                                        if item.done {
                                                            EMERALD
                                                        } else {
                                                            BG_ELEVATED
                                                        },
                                                        Stroke::new(
                                                            1.0_f32,
                                                            if item.done {
                                                                EMERALD_BRIGHT
                                                            } else {
                                                                BORDER_SUBTLE
                                                            },
                                                        ),
                                                    );
                                                    if item.done {
                                                        icons::paint_checkmark(
                                                            ui.painter(),
                                                            cb_r.center(),
                                                            9.0,
                                                            Color32::WHITE,
                                                        );
                                                    }
                                                    if can_toggle
                                                        && cb_resp
                                                            .on_hover_cursor(
                                                                CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                    {
                                                        act.tool_cmds.push(cmd_task_toggle(
                                                            &tid, &item.id,
                                                        ));
                                                    }

                                                    let mut rt = RichText::new(&item.text)
                                                        .size(11.5)
                                                        .color(if item.done {
                                                            TEXT_MUTED
                                                        } else {
                                                            TEXT_PRIMARY
                                                        });
                                                    if item.done {
                                                        rt = rt.strikethrough();
                                                    }
                                                    let avail_lbl_w = (ui.available_width()
                                                        - if can_remove { 24.0 } else { 4.0 })
                                                    .max(40.0);
                                                    ui.add_sized(
                                                        Vec2::new(avail_lbl_w, 16.0),
                                                        egui::Label::new(rt).truncate(),
                                                    );
                                                    if item.priority == "high"
                                                        || item.priority == "urgent"
                                                    {
                                                        ui.painter().circle_filled(
                                                            egui::pos2(
                                                                cb_r.left() - 2.0,
                                                                cb_r.center().y,
                                                            ),
                                                            2.0,
                                                            GOLD,
                                                        );
                                                    }
                                                    if can_remove && ui.small_button("×").clicked()
                                                    {
                                                        act.tool_cmds.push(cmd_task_remove(
                                                            &tid, &item.id,
                                                        ));
                                                    }
                                                });
                                                ui.add_space(2.0);
                                            }
                                        });
                                }

                                if can_add {
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        let ew = (ui.available_width() - 32.0).max(70.0);
                                        let ed = ui.add_sized(
                                            Vec2::new(ew, 22.0),
                                            egui::TextEdit::singleline(&mut st.task_input_draft)
                                                .hint_text("+ Nova tarefa..."),
                                        );
                                        let enter = ed.lost_focus()
                                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        let clicked = ui
                                            .add_enabled(
                                                !st.task_input_draft.trim().is_empty(),
                                                egui::Button::new(
                                                    RichText::new("+")
                                                        .size(12.0)
                                                        .semibold()
                                                        .color(Color32::WHITE),
                                                )
                                                .fill(EMERALD)
                                                .rounding(Rounding::same(5.0))
                                                .min_size(Vec2::new(24.0, 22.0)),
                                            )
                                            .clicked();
                                        if (enter || clicked)
                                            && !st.task_input_draft.trim().is_empty()
                                        {
                                            let txt = st.task_input_draft.trim().to_string();
                                            act.tool_cmds.push(cmd_task_add_full(
                                                &tid,
                                                &txt,
                                                None,
                                                "normal",
                                                None,
                                            ));
                                            st.task_input_draft.clear();
                                        }
                                    });
                                }
                            });
                        inline_tasks_h = card_resp.response.rect.height() + 6.0;
                        ui.add_space(4.0);
                    }
                }
            }

            #[cfg(feature = "mod_ferramentas")]
            let poll_creator_open = tools.as_ref().is_some_and(|st| st.chat_poll_open);
            #[cfg(not(feature = "mod_ferramentas"))]
            let poll_creator_open = false;
            let input_reserve = 48.0
                + if reply_target.is_some() { 28.0 } else { 0.0 }
                + if poll_creator_open { 188.0 } else { 0.0 };
            let _ = inline_tasks_h;
            let scroll_h = (ui.available_height() - input_reserve).max(90.0);

            egui::ScrollArea::vertical()
                .max_height(scroll_h)
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    #[cfg(feature = "mod_ferramentas")]
                    let has_polls = tools
                        .as_ref()
                        .is_some_and(|st| st.tools.iter().any(|t| matches!(t.payload, ToolPayload::Poll { .. })));
                    #[cfg(not(feature = "mod_ferramentas"))]
                    let has_polls = false;
                    if messages.is_empty() && !has_polls {
                        let center_pad = ((scroll_h - 24.0) * 0.45).max(16.0);
                        ui.add_space(center_pad);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("Nenhuma mensagem ainda.")
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                            );
                        });
                    }

                    for m in messages {
                        let is_mine = self_id.is_some() && self_id == Some(m.from.as_str());
                        ui.add_space(4.0);
                        egui::Frame::none()
                            .fill(if is_mine {
                                BLUE.linear_multiply(0.10)
                            } else {
                                BG_ELEVATED.linear_multiply(0.42)
                            })
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                            .show(ui, |ui| {
                                if let Some(ref rt) = m.reply_to {
                                    ui.label(
                                        RichText::new(format!(
                                            "↳ {}: {}",
                                            rt.name,
                                            rt.text.as_deref().unwrap_or("[midia]")
                                        ))
                                        .size(10.5)
                                        .color(TEXT_MUTED),
                                    );
                                }
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let author = if m.is_guest == Some(true) {
                                        format!("{} (guest)", m.name)
                                    } else {
                                        m.name.clone()
                                    };
                                    let author_col = if let Some([r, g, b]) =
                                        resolve_name_color_rgb(m.name_color.as_deref())
                                    {
                                        Color32::from_rgb(r, g, b)
                                    } else if is_mine {
                                        BLUE_LIGHT
                                    } else {
                                        EMERALD
                                    };
                                    ui.label(
                                        RichText::new(author)
                                            .size(12.0)
                                            .semibold()
                                            .color(author_col),
                                    );
                                    if let Some(tone) =
                                        m.flags.as_deref().and_then(verified_badge_tone)
                                    {
                                        let (r, _) = ui.allocate_exact_size(
                                            Vec2::new(11.0, 12.0),
                                            egui::Sense::hover(),
                                        );
                                        icons::paint_verified_seal(
                                            ui.painter(),
                                            r.center(),
                                            4.8,
                                            tone,
                                        );
                                    }
                                    if m.ts > 0 {
                                        ui.label(
                                            RichText::new(format_hhmm(m.ts))
                                                .size(10.0)
                                                .color(TEXT_MUTED),
                                        );
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("Responder").clicked() {
                                                *reply_target = Some(ChatReplyTo {
                                                    id: m.id.clone(),
                                                    name: m.name.clone(),
                                                    text: Some(m.text.chars().take(80).collect()),
                                                    kind: m.kind.clone(),
                                                });
                                            }
                                        },
                                    );
                                });

                                if !m.text.is_empty() {
                                    ui.label(
                                        RichText::new(&m.text)
                                            .size(12.5)
                                            .color(TEXT_PRIMARY),
                                    );
                                }
                                if m.kind.as_deref() == Some("gif") {
                                    if let Some(ref url) = m.url {
                                        ui.label(
                                            RichText::new(format!("[GIF] {}", url))
                                                .size(11.0)
                                                .color(TEXT_SECONDARY),
                                        );
                                    }
                                }
                                if let Some(ref imgs) = m.images {
                                    if !imgs.is_empty() {
                                        ui.label(
                                            RichText::new(format!(
                                                "[{} imagem(ns) anexada(s)]",
                                                imgs.len()
                                            ))
                                            .size(11.0)
                                            .color(TEXT_SECONDARY),
                                        );
                                    }
                                }
                            });
                    }

                    // Enquetes ativas da sala renderizadas dentro do chat
                    #[cfg(feature = "mod_ferramentas")]
                    if let Some(ref st) = tools {
                        for tool in &st.tools {
                            let ToolPayload::Poll {
                                ref question,
                                ref options,
                                multi,
                                closed,
                                ref counts,
                                total_voters,
                                ref mine,
                            } = tool.payload
                            else {
                                continue;
                            };
                            let can_vote = !closed && can_use_tool(tool, self_user_id, is_manager);
                            let can_close = !closed
                                && (is_manager
                                    || (!tool.created_by_id.is_empty()
                                        && self_user_id == Some(tool.created_by_id.as_str())));
                            let total_votes: u64 = counts.iter().sum::<u64>().max(1);

                            ui.add_space(6.0);
                            egui::Frame::none()
                                .fill(BG_TILE)
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(20, 83, 56)))
                                .rounding(Rounding::same(10.0))
                                .inner_margin(egui::Margin::same(9.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(if closed {
                                                "ENQUETE ENCERRADA"
                                            } else {
                                                "ENQUETE"
                                            })
                                            .size(9.5)
                                            .semibold()
                                            .color(if closed { TEXT_MUTED } else { EMERALD_BRIGHT }),
                                        );
                                        if multi && !closed {
                                            ui.label(
                                                RichText::new("• Multipla escolha")
                                                    .size(9.5)
                                                    .color(TEXT_MUTED),
                                            );
                                        }
                                        if can_close {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.small_button("Encerrar").clicked() {
                                                        act.poll_close = Some(tool.id.clone());
                                                    }
                                                },
                                            );
                                        }
                                    });
                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(question)
                                            .size(12.5)
                                            .semibold()
                                            .color(TEXT_PRIMARY),
                                    );
                                    ui.add_space(4.0);

                                    for (idx, opt) in options.iter().enumerate() {
                                        let count = counts.get(idx).copied().unwrap_or(0);
                                        let pct = ((count as f32 / total_votes as f32) * 100.0).round() as u32;
                                        let selected = mine.contains(&opt.id);
                                        let row_w = ui.available_width();
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(row_w, 26.0),
                                            if can_vote {
                                                egui::Sense::click()
                                            } else {
                                                egui::Sense::hover()
                                            },
                                        );
                                        ui.painter().rect(
                                            rect,
                                            Rounding::same(6.0),
                                            BG_ELEVATED,
                                            Stroke::new(
                                                1.0_f32,
                                                if selected { EMERALD } else { BORDER_SUBTLE },
                                            ),
                                        );
                                        if pct > 0 {
                                            let fill_w = (rect.width() * (pct as f32 / 100.0)).max(6.0);
                                            let fill_rect = egui::Rect::from_min_size(
                                                rect.min,
                                                Vec2::new(fill_w, rect.height()),
                                            );
                                            ui.painter().rect_filled(
                                                fill_rect,
                                                Rounding::same(6.0),
                                                if selected {
                                                    Color32::from_rgba_unmultiplied(16, 185, 129, 72)
                                                } else {
                                                    Color32::from_rgba_unmultiplied(255, 255, 255, 20)
                                                },
                                            );
                                        }
                                        let ind_center = egui::pos2(rect.left() + 14.0, rect.center().y);
                                        if multi {
                                            let ind_rect = egui::Rect::from_center_size(ind_center, Vec2::splat(13.0));
                                            if selected {
                                                ui.painter().rect_filled(ind_rect, Rounding::same(3.0), Color32::from_rgb(5, 150, 105));
                                                crate::ui::icons::paint_checkmark(ui.painter(), ind_center, 8.5, Color32::WHITE);
                                            } else {
                                                ui.painter().rect_stroke(ind_rect, Rounding::same(3.0), Stroke::new(1.4_f32, Color32::from_rgb(110, 113, 124)));
                                            }
                                        } else if selected {
                                            ui.painter().circle_filled(ind_center, 6.5, Color32::from_rgb(5, 150, 105));
                                            crate::ui::icons::paint_checkmark(ui.painter(), ind_center, 8.5, Color32::WHITE);
                                        } else {
                                            ui.painter().circle_stroke(ind_center, 6.5, Stroke::new(1.4_f32, Color32::from_rgb(110, 113, 124)));
                                        }
                                        ui.painter().text(
                                            egui::pos2(rect.left() + 26.0, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            &opt.text,
                                            egui::FontId::proportional(11.5),
                                            TEXT_PRIMARY,
                                        );
                                        ui.painter().text(
                                            egui::pos2(rect.right() - 8.0, rect.center().y),
                                            egui::Align2::RIGHT_CENTER,
                                            format!("{}% ({})", pct, count),
                                            egui::FontId::proportional(10.5),
                                            TEXT_SECONDARY,
                                        );
                                        if can_vote
                                            && resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .clicked()
                                        {
                                            let next_ids = if multi {
                                                let mut next = mine.clone();
                                                if let Some(p) = next.iter().position(|x| x == &opt.id) {
                                                    next.remove(p);
                                                } else {
                                                    next.push(opt.id.clone());
                                                }
                                                next
                                            } else if selected {
                                                Vec::new()
                                            } else {
                                                vec![opt.id.clone()]
                                            };
                                            act.poll_vote = Some((tool.id.clone(), next_ids));
                                        }
                                        ui.add_space(2.0);
                                    }

                                    ui.label(
                                        RichText::new(format!("{} voto(s)", total_voters))
                                            .size(10.5)
                                            .color(TEXT_MUTED),
                                    );
                                });
                        }
                    }
                });

            #[cfg(feature = "mod_ferramentas")]
            if let Some(ref mut st) = tools {
                if st.chat_poll_open {
                    ui.add_space(4.0);
                    egui::Frame::none()
                        .fill(BG_TILE)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Nova enquete no chat")
                                        .size(11.5)
                                        .semibold()
                                        .color(TEXT_PRIMARY),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("×").clicked() {
                                        st.chat_poll_open = false;
                                    }
                                });
                            });
                            ui.add(
                                egui::TextEdit::singleline(&mut st.chat_poll_question)
                                    .desired_width(ui.available_width())
                                    .hint_text("Pergunta da enquete..."),
                            );
                            ui.add_space(3.0);
                            let opt_count = st.chat_poll_options.len();
                            let mut remove_idx: Option<usize> = None;
                            for idx in 0..opt_count {
                                ui.horizontal(|ui| {
                                    let w = (ui.available_width() - if opt_count > 2 { 26.0 } else { 0.0 }).max(80.0);
                                    ui.add_sized(
                                        Vec2::new(w, 22.0),
                                        egui::TextEdit::singleline(&mut st.chat_poll_options[idx])
                                            .hint_text(format!("Opcao {}", idx + 1)),
                                    );
                                    if opt_count > 2 && ui.small_button("×").clicked() {
                                        remove_idx = Some(idx);
                                    }
                                });
                            }
                            if let Some(rm) = remove_idx {
                                st.chat_poll_options.remove(rm);
                            }
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut st.chat_poll_multi, "Multipla");
                                if st.chat_poll_options.len() < 6 && ui.small_button("+ Opcao").clicked() {
                                    st.chat_poll_options.push(String::new());
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let valid_opts: Vec<String> = st
                                        .chat_poll_options
                                        .iter()
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                    let can_submit = !st.chat_poll_question.trim().is_empty() && valid_opts.len() >= 2;
                                    if ui
                                        .add_enabled(
                                            can_submit,
                                            egui::Button::new(RichText::new("Criar").size(11.0).color(Color32::WHITE))
                                                .fill(EMERALD)
                                                .rounding(Rounding::same(6.0)),
                                        )
                                        .clicked()
                                    {
                                        act.create_poll = Some((
                                            st.chat_poll_question.trim().to_string(),
                                            valid_opts,
                                            st.chat_poll_multi,
                                        ));
                                        st.chat_poll_question.clear();
                                        st.chat_poll_options = vec![String::new(), String::new()];
                                        st.chat_poll_multi = false;
                                        st.chat_poll_open = false;
                                    }
                                });
                            });
                        });
                }
            }

            if !typing_names.is_empty() {
                let lbl = if typing_names.len() == 1 {
                    format!("{} esta digitando...", typing_names[0])
                } else {
                    format!("{} pessoas digitando...", typing_names.len())
                };
                ui.label(RichText::new(lbl).size(10.5).italics().color(TEXT_MUTED));
            }

            if let Some(ref rt) = reply_target.clone() {
                egui::Frame::none()
                    .fill(BG_ELEVATED)
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Respondendo a {}", rt.name))
                                    .size(11.0)
                                    .color(BLUE_LIGHT),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("x").clicked() {
                                        act.clear_reply = true;
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(3.0);
            }

            // Composer do site (desk): rounded-[1rem] border-zinc-700 bg-zinc-900 p-1, botoes h-9.
            egui::Frame::none()
                .fill(BG_ELEVATED)
                .stroke(Stroke::new(1.0_f32, crate::ui::theme::BORDER_STRONG))
                .rounding(Rounding::same(16.0))
                .inner_margin(egui::Margin::same(4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        #[cfg(feature = "mod_ferramentas")]
                        if let Some(ref mut st) = tools {
                            if can_create_tools {
                                let plus_btn = icons::icon_button(
                                    ui,
                                    Vec2::splat(22.0),
                                    if st.chat_poll_open {
                                        EMERALD
                                    } else {
                                        Color32::from_rgb(42, 49, 60)
                                    },
                                    Stroke::NONE,
                                    Rounding::same(11.0),
                                    "Criar enquete no chat",
                                    |p, c, col| icons::paint_plus(p, c, 10.0, col),
                                    if st.chat_poll_open {
                                        Color32::WHITE
                                    } else {
                                        TEXT_SECONDARY
                                    },
                                );
                                if plus_btn.clicked() {
                                    st.chat_poll_open = !st.chat_poll_open;
                                }
                            }
                        }

                        let right_reserve = 84.0; // GIF (h-9) + enviar (h-9 w-9)
                        let edit_w = (ui.available_width() - right_reserve).max(60.0);
                        let resp = ui.add_sized(
                            Vec2::new(edit_w, 36.0),
                            egui::TextEdit::singleline(draft)
                                .frame(false)
                                .font(egui::FontId::proportional(14.0))
                                .vertical_align(egui::Align::Center)
                                .hint_text("Digite uma mensagem..."),
                        );

                        let now_typing = !draft.trim().is_empty();
                        if now_typing != *was_typing {
                            *was_typing = now_typing;
                            act.typing_changed = Some(now_typing);
                        }

                        // GIF: h-9 rounded-[0.75rem] px-2 text-xs font-bold text-zinc-400.
                        let (gif_rect, gif_resp) =
                            ui.allocate_exact_size(Vec2::new(36.0, 36.0), Sense::hover());
                        if gif_resp.hovered() {
                            ui.painter().rect_filled(gif_rect, Rounding::same(12.0), BORDER_SUBTLE);
                        }
                        ui.painter().text(
                            gif_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "GIF",
                            egui::FontId::proportional(12.0),
                            TEXT_SECONDARY,
                        );
                        gif_resp.on_hover_text("Adicionar GIF");

                        let enter =
                            resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let has_text = !draft.trim().is_empty();
                        // Enviar: h-9 w-9 rounded-[0.75rem]; claro (zinc-50) quando da para enviar.
                        let send_resp = icons::icon_button(
                            ui,
                            Vec2::splat(36.0),
                            if has_text { Color32::from_rgb(250, 250, 251) } else { Color32::TRANSPARENT },
                            Stroke::NONE,
                            Rounding::same(12.0),
                            "Enviar",
                            |p, c, col| icons::paint_send(p, c, 16.0, col),
                            if has_text { BG_SURFACE } else { Color32::from_rgb(80, 83, 95) },
                        )
                        .on_hover_cursor(CursorIcon::PointingHand);

                        if (send_resp.clicked() || enter) && has_text {
                            act.send_text = Some(draft.trim().to_string());
                            act.reply_to = reply_target.clone();
                            draft.clear();
                            if *was_typing {
                                *was_typing = false;
                                act.typing_changed = Some(false);
                            }
                            resp.request_focus();
                        }
                    });
                });
        });

    act
}

fn format_hhmm(ts_ms: u64) -> String {
    let secs = (ts_ms / 1000) % 86_400;
    let local_secs = (secs + 86_400 - 3 * 3600) % 86_400;
    let h = local_secs / 3600;
    let m = (local_secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}
