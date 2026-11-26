//! Ferramentas da Sala (modificacao `ferramentas`):
//! - `render_room_tools_modal`: lancador compacto de ferramentas, maos levantadas e Tarefas.
//! - `render_tool_media_tile`: renderiza ferramentas de midia diretamente na grade/palco/tela cheia.

use crate::ui::theme::SemiBoldExt;
use egui::{
    pos2, Align, Color32, CursorIcon, FontId, Layout, Rect, RichText, Rounding, Sense, Stroke,
    Vec2,
};
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use crate::app::GoLiveLiteApp;
use crate::config::MOD_FERRAMENTAS;
use crate::net::doc_html::{
    doc_to_word_html, parse_css_hex_color, parse_doc_html,
    runs_plain_text, save_export_to_downloads, serialize_doc_html, toggle_runs_style,
    update_runs_with_typing_style, DocAlign, DocBlock,
    DocBlockKind, DocDocument, DocFocus, DocRun, InlineStyle, DOC_NO_ITEM,
};
use crate::net::room_tools::{
    can_open_kind, can_use_tool, cmd_add_stroke, cmd_clear_strokes, kind_plan_name, cmd_close_tool, cmd_code_language,
    cmd_create_tool, cmd_image_stroke, cmd_move_stroke, cmd_reaction, cmd_remove_strokes,
    cmd_task_add_full, cmd_task_clear_done, cmd_task_edit, cmd_task_remove, cmd_task_set_rules,
    cmd_task_toggle, encode_png_data_url, eval_calc_preview, eval_sheet_display,
    parse_pasted_tsv_grid, press_calc_key, sheet_cell_key, sheet_cell_name, sheet_col_name,
    sheet_to_csv_semicolon, sheet_to_excel_xls, task_allows, wheel_angle_at, CalcEntry, SheetCell,
    TaskItem, TaskRules, ToolPayload, ToolStroke, WbDragMode, WbMoveDrag, WheelEntry, WheelResult,
    WheelSpinState, CODE_LANGUAGES, PANEL_TOOL_KINDS, REACTION_EMOJIS, SHEET_DEFAULT_COL_WIDTH,
};
use crate::ui::icons;
use crate::ui::theme::{
    BG_ELEVATED, BG_TILE, BLUE, BLUE_LIGHT, BORDER_SUBTLE,
    EMERALD, EMERALD_BRIGHT, GOLD, RED, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use crate::ui::video_tile::paint_floating_reactions;

const WB_COLORS: &[(&str, Color32)] = &[
    ("#111827", Color32::from_rgb(22, 27, 34)),
    ("#ef4444", Color32::from_rgb(239, 68, 68)),
    ("#f59e0b", Color32::from_rgb(245, 158, 11)),
    ("#22c55e", Color32::from_rgb(34, 197, 94)),
    ("#3b82f6", Color32::from_rgb(59, 130, 246)),
    ("#a855f7", Color32::from_rgb(168, 85, 247)),
    ("#ec4899", Color32::from_rgb(236, 72, 153)),
    ("#ffffff", Color32::WHITE),
];

const WB_WIDTHS: &[f32] = &[2.0, 4.0, 8.0, 14.0];

const WHEEL_COLORS: &[&str] = &[
    "#ef4444", "#f97316", "#eab308", "#10b981", "#3b82f6", "#8b5cf6", "#ec4899", "#06b6d4",
];

const SHEET_FILL_PALETTE: &[(&str, Color32)] = &[
    ("#fef08a", Color32::from_rgb(254, 240, 138)),
    ("#bbf7d0", Color32::from_rgb(187, 247, 208)),
    ("#bfdbfe", Color32::from_rgb(191, 219, 254)),
    ("#fecaca", Color32::from_rgb(254, 202, 202)),
    ("#e9d5ff", Color32::from_rgb(233, 213, 255)),
    ("#fed7aa", Color32::from_rgb(254, 215, 170)),
    ("#e4e4e7", Color32::from_rgb(228, 228, 231)),
];

const SHEET_TEXT_PALETTE: &[(&str, Color32)] = &[
    ("#dc2626", Color32::from_rgb(220, 38, 38)),
    ("#16a34a", Color32::from_rgb(22, 163, 74)),
    ("#2563eb", Color32::from_rgb(37, 99, 235)),
    ("#9333ea", Color32::from_rgb(147, 51, 234)),
    ("#ea580c", Color32::from_rgb(234, 88, 12)),
    ("#71717a", Color32::from_rgb(110, 113, 124)),
];

const DOC_TEXT_COLORS: &[(&str, &str, Color32)] = &[
    ("Padrao", "", Color32::from_rgb(228, 228, 231)),
    ("Vermelho", "#dc2626", Color32::from_rgb(220, 38, 38)),
    ("Verde", "#16a34a", Color32::from_rgb(22, 163, 74)),
    ("Azul", "#2563eb", Color32::from_rgb(37, 99, 235)),
    ("Roxo", "#9333ea", Color32::from_rgb(147, 51, 234)),
    ("Laranja", "#ea580c", Color32::from_rgb(234, 88, 12)),
    ("Cinza", "#71717a", Color32::from_rgb(110, 113, 124)),
];

pub fn kind_badge_upper(kind: &str) -> &'static str {
    match kind {
        "whiteboard" => "LOUSA",
        "notepad" => "BLOCO DE NOTAS",
        "code" => "CODIGO",
        "wheel" => "ROLETA",
        "calc" => "CALCULADORA",
        "sheet" => "PLANILHA",
        "doc" => "DOCUMENTO",
        "tasks" => "TAREFAS",
        "poll" => "ENQUETE",
        "reactions" => "REACOES",
        _ => "FERRAMENTA",
    }
}

fn kind_display_name(kind: &str) -> &'static str {
    match kind {
        "whiteboard" => "Lousa",
        "notepad" => "Bloco de notas",
        "code" => "Codigo",
        "tasks" => "Tarefas",
        "poll" => "Enquete",
        "wheel" => "Roleta",
        "calc" => "Calculadora",
        "sheet" => "Planilha",
        "doc" => "Documento",
        "reactions" => "Reacoes",
        _ => "Ferramenta",
    }
}

pub fn can_close_tool(
    created_by_id: &str,
    is_mgr: bool,
    self_uid: Option<&str>,
) -> bool {
    is_mgr || (!created_by_id.is_empty() && self_uid == Some(created_by_id))
}

fn parse_hex_color32(hex: &str) -> Color32 {
    if let Some([r, g, b]) = parse_css_hex_color(hex) {
        return Color32::from_rgb(r, g, b);
    }
    Color32::from_rgb(22, 27, 34)
}

fn collect_room_people(app: &GoLiveLiteApp) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(ref uid) = app.self_user_id {
        out.push((uid.clone(), "Você".to_string()));
    }
    for p in &app.peers {
        if !p.is_obs() && !p.is_silent_moderator() {
            if let Some(ref uid) = p.user_id {
                out.push((uid.clone(), p.name.clone()));
            }
        }
    }
    out
}

/// O texto que a pessoa ve para um `error` do servidor sobre ferramentas; `None` = nao e de ferramenta.
/// A API ja manda o texto traduzido (pt/en/es, `apiMessages.ts`): ele e mostrado como veio.
pub fn tool_error_text(message: &str) -> Option<String> {
    let m = message.to_lowercase();
    let about_tools = ["ferramenta", "tool", "herramienta"].iter().any(|w| m.contains(w));
    about_tools.then(|| message.to_string())
}

/// Erro de plano (mostra "Ver planos"), em qualquer das tres linguas.
fn is_plan_error(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("plano") || m.contains("plan ")
}

// Cores do painel (classes do site no tema escuro da sala).
const P_ZINC_100: Color32 = Color32::from_rgb(244, 244, 245);
const P_ZINC_300: Color32 = Color32::from_rgb(212, 212, 216);
const P_EMERALD_600: Color32 = Color32::from_rgb(5, 150, 105);
const P_EMERALD_500: Color32 = Color32::from_rgb(16, 185, 129);
const P_EMERALD_400: Color32 = Color32::from_rgb(52, 211, 153);
const P_ROSE_500: Color32 = Color32::from_rgb(244, 63, 94);

/// Painel "Ferramentas" (copia do `RoomToolsPanel` do site): uma coluna na borda direita do
/// palco (`sm:w-[min(24rem,45%)]`, `top-2 right-2 bottom-2`), ou o palco inteiro ampliado.
pub fn render_room_tools_modal(app: &mut GoLiveLiteApp, ctx: &egui::Context) {
    if !app.has_modification(MOD_FERRAMENTAS)
        || !app.room_tools.as_ref().is_some_and(|rt| rt.panel_open)
        || app.current_room.is_none()
    {
        return;
    }

    let is_mgr = app.is_room_manager();
    let can_open_tools = is_mgr || app.room_permissions.tools;
    let self_uid = app.self_user_id.clone();
    let features: Vec<String> = app.logged_account.as_ref().map(|a| a.features.clone()).unwrap_or_default();
    let peer_names: Vec<String> = std::iter::once(app.effective_name().to_string())
        .chain(
            app.peers
                .iter()
                .filter(|p| !p.is_obs() && !p.is_silent_moderator())
                .map(|p| p.name.clone()),
        )
        .collect();
    let assignable = collect_room_people(app);
    let stage = app.stage_rect.unwrap_or_else(|| ctx.screen_rect().shrink(80.0));

    let Some(mut tools_box) = app.room_tools.take() else {
        return;
    };
    let was_tasks_open = tools_box.chat_tasks_open;
    let mut cmds: Vec<Value> = Vec::new();
    let mut focus_tile: Option<String> = None;
    let mut open_pro = false;

    let panel_rect = if tools_box.panel_expanded {
        stage
    } else {
        let w = (stage.width() * 0.45).min(384.0).max(280.0_f32.min(stage.width() - 16.0));
        Rect::from_min_max(
            pos2(stage.right() - 8.0 - w, stage.top() + 8.0),
            pos2(stage.right() - 8.0, stage.bottom() - 8.0),
        )
    };

    egui::Area::new(egui::Id::new("golive_room_tools_panel"))
        .order(egui::Order::Foreground)
        .fixed_pos(panel_rect.min)
        .show(ctx, |ui| {
            let st = &mut *tools_box;
            // `border-zinc-800 bg-zinc-900 shadow-xl rounded-xl`
            egui::Frame::none()
                .fill(BG_ELEVATED)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(20.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 10.0),
                    blur: 28.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(90),
                })
                .show(ui, |ui| {
                    ui.set_width(panel_rect.width());
                    ui.set_height(panel_rect.height());
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                    let active = st.active_tool_id.as_ref().and_then(|aid| {
                        st.tools
                            .iter()
                            .find(|t| &t.id == aid && t.kind != "reactions")
                            .map(|t| (t.id.clone(), t.kind.clone(), t.title.clone(), t.created_by_id.clone()))
                    });

                    // Cabecalho: `px-3 py-2 border-b`, voltar | titulo | ampliar | fechar.
                    let header = egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.set_width(ui.available_width());
                                if st.creating_kind.is_some() || active.is_some() {
                                    if panel_icon_button(ui, "Voltar", |p, c, col| icons::paint_arrow_back(p, c, 18.0, col)) {
                                        st.creating_kind = None;
                                        st.active_tool_id = None;
                                    }
                                }
                                if let (Some((_, kind, title, _)), None) = (active.as_ref(), st.creating_kind.as_ref()) {
                                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                                    icons::paint_tool_kind_icon(ui.painter(), ir.center(), 17.0, kind, P_EMERALD_600);
                                    ui.add(egui::Label::new(RichText::new(title).size(14.0).semibold().color(P_ZINC_100)).truncate());
                                } else {
                                    ui.label(RichText::new("Ferramentas").size(14.0).semibold().color(P_ZINC_100));
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if panel_icon_button(ui, "Fechar painel", |p, c, col| icons::paint_close_x(p, c, 20.0, col)) {
                                        st.panel_open = false;
                                    }
                                    let expanded = st.panel_expanded;
                                    if panel_icon_button(ui, if expanded { "Reduzir" } else { "Ampliar" }, move |p, c, col| {
                                        icons::paint_fullscreen(p, c, 16.0, col, expanded)
                                    }) {
                                        st.panel_expanded = !st.panel_expanded;
                                    }
                                    // Encerrar a ferramenta aberta (dono ou administrador).
                                    if let Some((ref tid, _, _, ref owner)) = active {
                                        if st.creating_kind.is_none() && can_close_tool(owner, is_mgr, self_uid.as_deref()) {
                                            if panel_icon_button(ui, "Desativar ferramenta", |p, c, col| icons::paint_trash(p, c, 17.0, col)) {
                                                cmds.push(cmd_close_tool(tid));
                                                st.active_tool_id = None;
                                            }
                                        }
                                    }
                                });
                            });
                        });
                    let hr = header.response.rect;
                    ui.painter().hline(hr.x_range(), hr.bottom(), Stroke::new(1.0_f32, BORDER_SUBTLE));

                    // Corpo: `p-3 overflow-y-auto`
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if let Some(err) = st.server_error.clone() {
                                    let mut dismiss = false;
                                    egui::Frame::none()
                                        .fill(Color32::from_rgb(69, 10, 10))
                                        .rounding(Rounding::same(14.0))
                                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(RichText::new(&err).size(13.0).color(Color32::from_rgb(252, 165, 165)));
                                            ui.horizontal(|ui| {
                                                if is_plan_error(&err) && ui.link(RichText::new("Ver planos").size(12.5).semibold().color(P_ZINC_100)).clicked() {
                                                    open_pro = true;
                                                }
                                                if ui.link(RichText::new("Fechar").size(12.5).color(TEXT_MUTED)).clicked() {
                                                    dismiss = true;
                                                }
                                            });
                                        });
                                    if dismiss {
                                        st.server_error = None;
                                    }
                                    ui.add_space(8.0);
                                }

                                if let Some(ref ck) = st.creating_kind.clone() {
                                    render_create_tool_form(st, ui, ck, &peer_names, &mut cmds);
                                } else if let Some((tid, kind, _, _)) = active.clone() {
                                    let Some(tool) = st.tools.iter().find(|t| t.id == tid).cloned() else {
                                        st.active_tool_id = None;
                                        return;
                                    };
                                    let can_use = can_use_tool(&tool, self_uid.as_deref(), is_mgr);
                                    match (&tool.payload, kind.as_str()) {
                                        (ToolPayload::Tasks { items, rules }, _) => render_tasks_view(
                                            st,
                                            ui,
                                            &tool.id,
                                            items,
                                            rules,
                                            self_uid.as_deref(),
                                            can_use,
                                            is_mgr,
                                            &assignable,
                                            &mut cmds,
                                        ),
                                        (ToolPayload::Annotate { strokes }, _) => {
                                            render_annotate_view(st, ui, &tool.id, strokes, self_uid.as_deref(), can_use, is_mgr, &mut cmds)
                                        }
                                        _ => {
                                            // As de midia ficam na grade: o painel leva ate o tile.
                                            focus_tile = Some(format!("tool:{}", tool.id));
                                            st.active_tool_id = None;
                                            st.panel_open = false;
                                        }
                                    }
                                } else {
                                    render_tools_home(
                                        st,
                                        ui,
                                        self_uid.as_deref(),
                                        is_mgr,
                                        can_open_tools,
                                        &features,
                                        &mut focus_tile,
                                        &mut open_pro,
                                        &mut cmds,
                                    );
                                }
                            });
                        });
                });
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && tools_box.creating_kind.is_none() {
        tools_box.panel_open = false;
    }
    let open_chat_tasks = !was_tasks_open && tools_box.chat_tasks_open;
    app.room_tools = Some(tools_box);

    if open_chat_tasks {
        app.right_sidebar_collapsed = false;
        app.right_tab_room = false;
    }
    if open_pro {
        crate::ui::external_link::request("https://golive.nemtudo.me/pro");
    }
    if let Some(ft) = focus_tile {
        app.focused_tile_id = Some(ft);
        app.sync_video_render_policies();
    }
    for cmd in cmds {
        app.send_tool_json(cmd);
    }
}

/// `rounded-md p-1 text-zinc-300 hover:bg-zinc-800`, icone 20 px.
fn panel_icon_button(ui: &mut egui::Ui, tip: &str, paint: impl FnOnce(&egui::Painter, egui::Pos2, Color32)) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, Rounding::same(8.0), BORDER_SUBTLE);
    }
    paint(ui.painter(), r.center(), P_ZINC_300);
    resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn access_label(access: &str) -> &'static str {
    match access {
        "managers" => "Só administradores",
        "selected" => "Pessoas escolhidas",
        _ => "Todos na sala",
    }
}

/// `ToolsHome` do site: as abertas, "Nova ferramenta" em 3 colunas e "Reações nas mídias".
#[allow(clippy::too_many_arguments)]
fn render_tools_home(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    self_uid: Option<&str>,
    is_mgr: bool,
    can_open_tools: bool,
    features: &[String],
    focus_tile: &mut Option<String>,
    open_pro: &mut bool,
    cmds: &mut Vec<Value>,
) {
    let listed: Vec<_> = st.tools.iter().filter(|t| t.kind != "reactions").cloned().collect();
    if listed.is_empty() {
        ui.label(
            RichText::new(if can_open_tools {
                "Nenhuma ferramenta aberta. Abra uma abaixo. Você decide quem pode usar cada uma."
            } else {
                "Nenhuma ferramenta aberta ainda. Os administradores da sala podem abrir lousa, bloco de notas, tarefas e mais."
            })
            .size(14.0)
            .color(TEXT_SECONDARY),
        );
        ui.add_space(10.0);
    } else {
        ui.spacing_mut().item_spacing.y = 6.0;
        for tool in &listed {
            let media = crate::net::room_tools::is_media_tool_kind(&tool.kind);
            let sub = format!(
                "{}{} · aberta por {}",
                if media { "na tela · " } else { "" },
                access_label(&tool.access),
                tool.created_by_name
            );
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::click());
            let hov = resp.hovered();
            ui.painter().rect_stroke(r, Rounding::same(14.0), Stroke::new(1.0_f32, if hov { P_EMERALD_500 } else { BORDER_SUBTLE }));
            icons::paint_tool_kind_icon(ui.painter(), pos2(r.left() + 22.0, r.center().y), 19.0, &tool.kind, P_EMERALD_600);
            let text_w = r.width() - 44.0 - 30.0;
            // Uma linha cada, com "…" no fim (`truncate` do site).
            let one_line = |text: String, font: FontId, color: Color32| {
                let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
                ui.fonts(|f| f.layout_job(job))
            };
            let title = one_line(
                tool.title.clone(),
                FontId::new(14.0, egui::FontFamily::Name(crate::ui::theme::SEMIBOLD.into())),
                P_ZINC_100,
            );
            ui.painter().galley(pos2(r.left() + 44.0, r.top() + 7.0), title, P_ZINC_100);
            let sub_g = one_line(sub, FontId::proportional(12.0), TEXT_MUTED);
            ui.painter().galley(pos2(r.left() + 44.0, r.top() + 26.0), sub_g, TEXT_MUTED);
            if !can_use_tool(tool, self_uid, is_mgr) {
                icons::paint_lock(ui.painter(), pos2(r.right() - 18.0, r.center().y), 14.0, TEXT_MUTED);
            }
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                if media {
                    *focus_tile = Some(format!("tool:{}", tool.id));
                    st.panel_open = false;
                } else if tool.kind == "tasks" {
                    st.chat_tasks_open = true;
                    st.panel_open = false;
                } else {
                    st.active_tool_id = Some(tool.id.clone());
                }
            }
        }
        ui.add_space(12.0);
    }

    if !can_open_tools {
        return;
    }

    // "NOVA FERRAMENTA": `text-xs font-semibold uppercase tracking-wide text-zinc-500`
    ui.label(RichText::new("NOVA FERRAMENTA").size(12.0).semibold().color(TEXT_MUTED));
    ui.add_space(2.0);
    let single_taken: Vec<String> = st
        .tools
        .iter()
        .filter(|t| matches!(t.kind.as_str(), "reactions" | "annotate" | "tasks"))
        .map(|t| t.kind.clone())
        .collect();
    let gap = 8.0;
    let cols = if ui.available_width() >= 330.0 { 3 } else { 2 };
    let card_w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    for row in PANEL_TOOL_KINDS.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &(kind, name, hint) in row {
                let allowed = can_open_kind(kind, features);
                let plan = kind_plan_name(kind);
                let taken = single_taken.iter().any(|k| k == kind);
                let (r, resp) = ui.allocate_exact_size(Vec2::new(card_w, 104.0), if taken { Sense::hover() } else { Sense::click() });
                let hov = resp.hovered() && !taken;
                let alpha = if taken { 0.4 } else { 1.0 };
                let p = ui.painter();
                p.rect_stroke(r, Rounding::same(14.0), Stroke::new(1.0_f32, if hov { P_EMERALD_500 } else { BORDER_SUBTLE }));
                // Linha do titulo: icone 18 + nome + cadeado
                let top = r.top() + 10.0;
                icons::paint_tool_kind_icon(p, pos2(r.left() + 19.0, top + 9.0), 17.0, kind, P_EMERALD_600.gamma_multiply(alpha));
                let name_w = card_w - 38.0 - if allowed { 0.0 } else { 18.0 };
                let mut job = egui::text::LayoutJob::simple_singleline(
                    name.to_string(),
                    FontId::new(14.0, egui::FontFamily::Name(crate::ui::theme::SEMIBOLD.into())),
                    P_ZINC_100.gamma_multiply(alpha),
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(name_w);
                let g = ui.fonts(|f| f.layout_job(job));
                let p = ui.painter();
                p.galley(pos2(r.left() + 32.0, top), g, P_ZINC_100);
                if !allowed {
                    icons::paint_lock(p, pos2(r.right() - 14.0, top + 9.0), 12.5, TEXT_MUTED);
                }
                // Dica: `text-[11px] leading-snug text-zinc-500`
                let hint_g = ui.fonts(|f| f.layout(hint.to_string(), FontId::proportional(11.0), TEXT_MUTED.gamma_multiply(alpha), card_w - 20.0));
                let hint_h = hint_g.size().y;
                ui.painter().galley(pos2(r.left() + 10.0, top + 24.0), hint_g, TEXT_MUTED);
                // Plano: pilula `rounded-full px-1.5 py-0.5 text-[10px] font-semibold`
                if let Some(plan) = plan {
                    let (bg, fg) = if allowed {
                        (P_EMERALD_600.gamma_multiply(0.12), P_EMERALD_400)
                    } else {
                        (P_ROSE_500.gamma_multiply(0.12), P_ROSE_500)
                    };
                    let pg = ui.fonts(|f| f.layout_no_wrap(plan.to_string(), FontId::new(10.0, egui::FontFamily::Name(crate::ui::theme::SEMIBOLD.into())), fg));
                    let pr = Rect::from_min_size(pos2(r.left() + 10.0, (top + 28.0 + hint_h).min(r.bottom() - 22.0)), pg.size() + egui::vec2(12.0, 4.0));
                    let p = ui.painter();
                    p.rect_filled(pr, Rounding::same(8.0), bg);
                    p.galley(pr.min + egui::vec2(6.0, 2.0), pg, fg);
                }
                let resp = if taken { resp.on_hover_text("Já está aberta nesta sala") } else { resp.on_hover_cursor(CursorIcon::PointingHand) };
                if resp.clicked() && !taken {
                    if allowed {
                        st.creating_kind = Some(kind.to_string());
                        st.create_title_draft = name.to_string();
                        st.server_error = None;
                    } else {
                        // Como o site (abre os planos), mas sem trancar: um experimento pode
                        // liberar a ferramenta, e o servidor e quem decide de verdade.
                        st.creating_kind = Some(kind.to_string());
                        st.create_title_draft = name.to_string();
                    }
                }
            }
        });
        ui.add_space(gap - 6.0);
    }
    let _ = open_pro;

    // "Reações nas mídias": `rounded-lg border px-3 py-2` com caixa de marcar.
    ui.add_space(4.0);
    let reactions_id = st.reactions_tool_id().map(str::to_string);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    let p = ui.painter();
    p.rect_stroke(r, Rounding::same(14.0), Stroke::new(1.0_f32, BORDER_SUBTLE));
    icons::paint_reaction_smile(p, pos2(r.left() + 22.0, r.center().y), 18.0, P_EMERALD_600);
    p.text(pos2(r.left() + 40.0, r.center().y), egui::Align2::LEFT_CENTER, "Reações nas mídias", FontId::proportional(14.0), P_ZINC_100);
    let on = reactions_id.is_some();
    let locked = on && !is_mgr;
    let cb = Rect::from_center_size(pos2(r.right() - 20.0, r.center().y), Vec2::splat(16.0));
    if on {
        p.rect_filled(cb, Rounding::same(4.0), if locked { TEXT_MUTED } else { P_EMERALD_600 });
        p.add(egui::Shape::line(
            vec![pos2(cb.left() + 3.5, cb.center().y), pos2(cb.left() + 6.5, cb.bottom() - 4.0), pos2(cb.right() - 3.0, cb.top() + 4.0)],
            Stroke::new(1.8_f32, Color32::WHITE),
        ));
    } else {
        p.rect_stroke(cb, Rounding::same(4.0), Stroke::new(1.2_f32, TEXT_MUTED));
    }
    if !locked && resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        if let Some(rid) = reactions_id {
            cmds.push(cmd_close_tool(&rid));
        } else {
            cmds.push(cmd_create_tool("reactions", json!({ "access": "everyone" })));
        }
    }
}

/// "Anotar na tela" no painel (`AnnotateView` do site): dica, caneta, desfazer, limpar, contagem.
#[allow(clippy::too_many_arguments)]
fn render_annotate_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    strokes: &[ToolStroke],
    self_uid: Option<&str>,
    can_use: bool,
    is_mgr: bool,
    cmds: &mut Vec<Value>,
) {
    ui.label(
        RichText::new("Com a ferramenta aberta você já desenha em todas as telas — todos veem no mesmo ponto. O lápis em cada mídia desliga o desenho nela, e o pincel fica no botão \"Anotar na tela\" embaixo.")
            .size(14.0)
            .color(TEXT_SECONDARY),
    );
    ui.add_space(8.0);
    if can_use {
        // Cores e espessura (PenToolbar do site)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (hex, col) in WB_COLORS.iter() {
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                ui.painter().circle_filled(r.center(), 9.0, *col);
                if st.pen_color.eq_ignore_ascii_case(hex) {
                    ui.painter().circle_stroke(r.center(), 11.5, Stroke::new(2.0_f32, P_EMERALD_500));
                }
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    st.pen_color = (*hex).to_string();
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (w, label) in [(2.0_f32, "Fina"), (4.0, "Média"), (8.0, "Grossa")] {
                let sel = (st.pen_width - w).abs() < 0.1;
                if ui
                    .add(egui::Button::new(RichText::new(label).size(12.5).color(if sel { Color32::WHITE } else { TEXT_SECONDARY }))
                        .fill(if sel { P_EMERALD_600 } else { Color32::TRANSPARENT })
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(8.0)))
                    .clicked()
                {
                    st.pen_width = w;
                }
            }
        });
        ui.add_space(6.0);
        let mine: Vec<&ToolStroke> = strokes.iter().filter(|s| Some(s.by.as_str()) == self_uid).collect();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!mine.is_empty(), egui::Button::new(RichText::new("Desfazer").size(12.5)).rounding(Rounding::same(8.0)))
                .clicked()
            {
                if let Some(last) = mine.last() {
                    cmds.push(cmd_remove_strokes(tool_id, &[last.id.clone()]));
                }
            }
            let clear_label = if is_mgr { "Limpar tudo" } else { "Limpar o que eu fiz" };
            if ui.add(egui::Button::new(RichText::new(clear_label).size(12.5)).rounding(Rounding::same(8.0))).clicked() {
                if is_mgr {
                    cmds.push(cmd_clear_strokes(tool_id));
                } else {
                    let ids: Vec<String> = mine.iter().map(|s| s.id.clone()).collect();
                    if !ids.is_empty() {
                        cmds.push(cmd_remove_strokes(tool_id, &ids));
                    }
                }
            }
        });
    } else {
        ui.label(RichText::new("Somente visualização — um administrador decide quem pode usar.").size(12.0).color(TEXT_MUTED));
    }
    ui.add_space(6.0);
    ui.label(RichText::new(format!("{} anotações nas telas", strokes.len())).size(12.0).color(TEXT_MUTED));
}

/// A camada de "Anotar na tela" sobre uma midia (`AnnotationLayer` do site). `content` e a
/// area onde o video aparece (sem as faixas pretas): os pontos sao 0..1 dela, iguais para todos.
/// Devolve o comando do traco terminado, quando houver.
pub fn render_annotation_layer(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    media_key: &str,
    content: Rect,
    self_uid: Option<&str>,
    is_mgr: bool,
) -> Option<Value> {
    let tool = st.tools.iter().find(|t| t.kind == "annotate")?.clone();
    let ToolPayload::Annotate { strokes } = &tool.payload else { return None };
    let can_draw = can_use_tool(&tool, self_uid, is_mgr) && !st.annotate_off.contains(media_key);
    let to_screen = |x: f32, y: f32| pos2(content.left() + x * content.width(), content.top() + y * content.height());
    let painter = ui.painter_at(content);
    let paint = |pts: &[f32], color: &str, width: f32| {
        if pts.len() < 4 {
            return;
        }
        let line: Vec<egui::Pos2> = pts.chunks_exact(2).map(|c| to_screen(c[0], c[1])).collect();
        // Largura em "pixels do quadro" (`BOARD_WIDTH` = 1000 no site), escalada para esta midia.
        let w = (width * content.width() / 1000.0).max(1.2);
        painter.add(egui::Shape::line(line, Stroke::new(w, parse_hex_color32(color))));
    };
    for s in strokes.iter().filter(|s| s.target.as_deref() == Some(media_key)) {
        paint(&s.points, &s.color, s.width);
    }
    if let Some(live) = st.wb_live_strokes.get(&tool.id) {
        for s in live.values().filter(|s| s.target.as_deref() == Some(media_key)) {
            paint(&s.points, &s.color, s.width);
        }
    }

    // Lapis desta midia (AnnotateButton): liga/desliga o desenho so nela.
    if can_use_tool(&tool, self_uid, is_mgr) {
        let br = Rect::from_min_size(pos2(content.left() + 8.0, content.top() + 8.0), Vec2::splat(28.0));
        let on = !st.annotate_off.contains(media_key);
        let resp = ui.interact(br, ui.id().with(("annotate_toggle", media_key)), Sense::click());
        painter.circle_filled(br.center(), 14.0, if on { P_EMERALD_600 } else { Color32::from_black_alpha(150) });
        icons::paint_brush(&painter, br.center(), 15.0, Color32::WHITE);
        if resp
            .on_hover_text(if on { "Parar de desenhar nesta tela" } else { "Desenhar nesta tela" })
            .clicked()
        {
            if on {
                st.annotate_off.insert(media_key.to_string());
            } else {
                st.annotate_off.remove(media_key);
            }
        }
    }
    if !can_draw {
        return None;
    }
    // Borda verde enquanto da para desenhar (`ring-2 ring-emerald-500/70`)
    painter.rect_stroke(content.shrink(1.0), Rounding::ZERO, Stroke::new(2.0_f32, P_EMERALD_500.gamma_multiply(0.7)));
    let draw_rect = Rect::from_min_max(pos2(content.left(), content.top() + 40.0), content.max);
    let resp = ui.interact(draw_rect, ui.id().with(("annotate_draw", media_key)), Sense::drag());
    let mut out = None;
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let n = [((pos.x - content.left()) / content.width()).clamp(0.0, 1.0), ((pos.y - content.top()) / content.height()).clamp(0.0, 1.0)];
            match st.annotate_drawing.as_mut() {
                Some((key, pts)) if key == media_key => {
                    pts.push(n[0]);
                    pts.push(n[1]);
                }
                _ => st.annotate_drawing = Some((media_key.to_string(), vec![n[0], n[1]])),
            }
        }
    }
    if let Some((key, pts)) = st.annotate_drawing.as_ref() {
        if key == media_key {
            paint(pts, &st.pen_color.clone(), st.pen_width);
        }
    }
    if resp.drag_stopped() {
        if let Some((key, pts)) = st.annotate_drawing.take() {
            if key == media_key && pts.len() >= 4 {
                let cid = st.next_stroke_client_id();
                out = Some(crate::net::room_tools::cmd_add_stroke_on(
                    &tool.id,
                    "pen",
                    &st.pen_color,
                    st.pen_width,
                    &pts,
                    &cid,
                    Some(media_key),
                ));
            }
        }
    }
    out
}


fn render_preset_pill(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    paint_icon: impl FnOnce(&egui::Painter, egui::Pos2, Color32),
) -> bool {
    let text_w = ui.fonts(|f| {
        f.layout_no_wrap(label.to_string(), FontId::proportional(11.0), TEXT_PRIMARY)
            .size()
            .x
    });
    let w = text_w + 30.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
    let bg = if active {
        Color32::from_rgb(18, 60, 48)
    } else if resp.hovered() {
        Color32::from_rgb(42, 49, 60)
    } else {
        BG_ELEVATED
    };
    let border = if active { EMERALD } else { BORDER_SUBTLE };
    ui.painter()
        .rect(rect, Rounding::same(12.0), bg, Stroke::new(1.0_f32, border));
    paint_icon(ui.painter(), pos2(rect.left() + 13.0, rect.center().y), EMERALD_BRIGHT);
    ui.painter().text(
        pos2(rect.left() + 23.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.0),
        TEXT_PRIMARY,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn render_create_tool_form(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    kind: &str,
    peer_names: &[String],
    cmds: &mut Vec<Value>,
) {
    ui.label(RichText::new("Titulo").size(12.0).color(TEXT_SECONDARY));
    ui.add(
        egui::TextEdit::singleline(&mut st.create_title_draft)
            .desired_width(ui.available_width())
            .hint_text(kind_display_name(kind)),
    );
    ui.add_space(6.0);
    ui.checkbox(&mut st.create_access_everyone, "Todos na sala podem usar/editar");
    ui.add_space(6.0);

    if kind == "code" {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Linguagem:").size(12.0).color(TEXT_SECONDARY));
            egui::ComboBox::from_id_salt("tool_create_lang")
                .selected_text(&st.create_code_lang)
                .show_ui(ui, |ui| {
                    for &l in CODE_LANGUAGES {
                        ui.selectable_value(&mut st.create_code_lang, l.to_string(), l);
                    }
                });
        });
    } else if kind == "poll" {
        ui.label(RichText::new("Pergunta").size(12.0).color(TEXT_SECONDARY));
        ui.add(
            egui::TextEdit::singleline(&mut st.poll_question_draft)
                .desired_width(ui.available_width()),
        );
        ui.checkbox(&mut st.poll_multi_draft, "Permitir multipla escolha");
        ui.label(RichText::new("Opcoes").size(11.5).color(TEXT_SECONDARY));
        for i in 0..st.poll_options_draft.len() {
            ui.add(
                egui::TextEdit::singleline(&mut st.poll_options_draft[i])
                    .desired_width(ui.available_width())
                    .hint_text(format!("Opcao {}", i + 1)),
            );
        }
        if st.poll_options_draft.len() < 6 && ui.small_button("+ Opcao").clicked() {
            st.poll_options_draft.push(String::new());
        }
    } else if kind == "wheel" {
        ui.label(
            RichText::new("Uma por linha. Da para mudar depois.")
                .size(11.5)
                .color(TEXT_SECONDARY),
        );
        ui.add(
            egui::TextEdit::multiline(&mut st.wheel_bulk_draft)
                .desired_rows(4)
                .desired_width(ui.available_width()),
        );
        ui.add_space(6.0);
        // Presets exatos da captura 1: Pessoas da sala, Sim / Nao, Cara ou coroa, Dado, Numeros, Letras
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
            if render_preset_pill(ui, "Pessoas da sala", false, |p, c, col| {
                icons::paint_groups(p, c, 12.0, col);
            }) {
                st.wheel_bulk_draft = peer_names.join("\n");
            }
            if render_preset_pill(ui, "Sim / Nao", false, |p, c, col| {
                icons::paint_checkmark(p, c, 11.0, col);
            }) {
                st.wheel_bulk_draft = "Sim\nNao".to_string();
            }
            if render_preset_pill(ui, "Cara ou coroa", false, |p, c, col| {
                icons::paint_coin(p, c, 12.0, col);
            }) {
                st.wheel_bulk_draft = "Cara\nCoroa".to_string();
            }
            if render_preset_pill(ui, "Dado", false, |p, c, col| {
                icons::paint_dice(p, c, 12.0, col);
            }) {
                st.wheel_bulk_draft = "1\n2\n3\n4\n5\n6".to_string();
            }
            if render_preset_pill(ui, "Numeros", st.wheel_create_num_open, |p, c, col| {
                p.text(c, egui::Align2::CENTER_CENTER, "12", FontId::proportional(9.5), col);
            }) {
                st.wheel_create_num_open = !st.wheel_create_num_open;
            }
            if render_preset_pill(ui, "Letras", false, |p, c, col| {
                p.text(c, egui::Align2::CENTER_CENTER, "Ab", FontId::proportional(9.5), col);
            }) {
                st.wheel_bulk_draft = ('A'..='Z')
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        });
        if st.wheel_create_num_open {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("De").size(11.0).color(TEXT_MUTED));
                ui.add_sized(
                    Vec2::new(44.0, 22.0),
                    egui::TextEdit::singleline(&mut st.wheel_num_from),
                );
                ui.label(RichText::new("Ate").size(11.0).color(TEXT_MUTED));
                ui.add_sized(
                    Vec2::new(44.0, 22.0),
                    egui::TextEdit::singleline(&mut st.wheel_num_to),
                );
                if ui
                    .add(
                        egui::Button::new(RichText::new("OK").size(11.0).color(Color32::WHITE))
                            .fill(EMERALD)
                            .rounding(Rounding::same(6.0)),
                    )
                    .clicked()
                {
                    let a = st.wheel_num_from.trim().parse::<i32>().unwrap_or(1);
                    let b = st.wheel_num_to.trim().parse::<i32>().unwrap_or(10);
                    let (lo, hi) = (a.min(b), a.max(b));
                    let list: Vec<String> = (lo..=lo + (hi - lo).min(59))
                        .map(|n| n.to_string())
                        .collect();
                    st.wheel_bulk_draft = list.join("\n");
                    st.wheel_create_num_open = false;
                }
            });
        }
    }

    ui.add_space(10.0);
    if ui
        .add(
            egui::Button::new(
                RichText::new("Criar na chamada")
                    .size(13.0)
                    .semibold()
                    .color(Color32::WHITE),
            )
            .fill(EMERALD)
            .rounding(Rounding::same(8.0))
            .min_size(Vec2::new(170.0, 32.0)),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked()
    {
        let access = if st.create_access_everyone {
            "everyone"
        } else {
            "managers"
        };
        let title_str = {
            let t = st.create_title_draft.trim();
            if t.is_empty() {
                kind_display_name(kind)
            } else {
                t
            }
        };
        let mut opts = json!({
            "title": title_str,
            "access": access,
        });
        if kind == "code" {
            opts["language"] = Value::String(st.create_code_lang.clone());
        } else if kind == "poll" {
            let poll_opts: Vec<String> = st
                .poll_options_draft
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if poll_opts.len() < 2 || st.poll_question_draft.trim().is_empty() {
                return;
            }
            opts["poll"] = json!({
                "question": st.poll_question_draft.trim(),
                "options": poll_opts,
                "multi": st.poll_multi_draft,
                "anonymous": false,
                "announce": true,
                "durationMinutes": Value::Null,
            });
            st.poll_question_draft.clear();
            st.poll_options_draft = vec![String::new(), String::new()];
            st.creating_kind = None;
            st.panel_open = false;
        } else if kind == "tasks" {
            opts["rules"] = json!({
                "add": "everyone",
                "complete": "everyone",
                "edit": "everyone",
            });
            st.chat_tasks_open = true;
            st.creating_kind = None;
            st.panel_open = false;
        } else if kind == "wheel" {
            let entries: Vec<Value> = st
                .wheel_bulk_draft
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| json!({ "text": l }))
                .collect();
            if !entries.is_empty() {
                opts["entries"] = Value::Array(entries);
            }
        }
        cmds.push(cmd_create_tool(kind, opts));
    }
}

/// Miniatura compacta de ferramenta para a faixa lateral no modo foco.
pub fn render_tool_thumbnail_tile(
    app: &mut GoLiveLiteApp,
    ui: &mut egui::Ui,
    tool_id: &str,
    size: Vec2,
) {
    let Some(ref tools_box) = app.room_tools else {
        return;
    };
    let Some(tool) = tools_box.tools.iter().find(|t| t.id == tool_id).cloned() else {
        return;
    };
    let is_mgr = app.is_room_manager();
    let self_uid = app.self_user_id.clone();
    let can_close = can_close_tool(&tool.created_by_id, is_mgr, self_uid.as_deref());

    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let clip = ui.clip_rect().intersect(rect);
    let painter = ui.painter().with_clip_rect(clip);
    let bg = if resp.hovered() {
        Color32::from_rgb(20, 25, 34)
    } else {
        Color32::from_rgb(22, 27, 34)
    };
    painter.rect(
        rect,
        Rounding::same(10.0),
        bg,
        Stroke::new(1.0_f32, if resp.hovered() { EMERALD } else { BORDER_SUBTLE }),
    );
    icons::paint_tool_kind_icon(
        &painter,
        pos2(rect.center().x, rect.center().y - 14.0),
        22.0,
        &tool.kind,
        EMERALD_BRIGHT,
    );
    painter.text(
        pos2(rect.center().x, rect.center().y + 12.0),
        egui::Align2::CENTER_CENTER,
        &tool.title,
        FontId::proportional(12.5),
        TEXT_PRIMARY,
    );
    painter.text(
        pos2(rect.center().x, rect.center().y + 29.0),
        egui::Align2::CENTER_CENTER,
        kind_badge_upper(&tool.kind),
        FontId::proportional(9.5),
        EMERALD_BRIGHT,
    );

    let mut close_clicked = false;
    if can_close {
        let xr_rect = Rect::from_min_size(pos2(rect.right() - 26.0, rect.top() + 6.0), Vec2::splat(20.0));
        let mut xui = ui.new_child(egui::UiBuilder::new().max_rect(xr_rect));
        xui.set_clip_rect(clip.intersect(xr_rect));
        if icons::media_overlay_circle_button(
            &mut xui,
            20.0,
            Color32::from_rgba_unmultiplied(30, 35, 46, 220),
            "Fechar ferramenta",
            |p, c, col| icons::paint_close_x(p, c, 9.5, col),
            Color32::from_rgb(248, 113, 113),
        )
        .clicked()
        {
            close_clicked = true;
        }
    }

    if close_clicked {
        app.send_tool_json(cmd_close_tool(&tool.id));
    } else if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        app.focused_tile_id = Some(format!("tool:{}", tool.id));
    }
}

/// Renderiza qualquer ferramenta colaborativa de midia diretamente dentro de um tile 16:9 / palco / tela cheia.
pub fn render_tool_media_tile(
    app: &mut GoLiveLiteApp,
    ui: &mut egui::Ui,
    tool_id: &str,
    size: Vec2,
    is_focused: bool,
    is_fullscreen: bool,
) {
    let Some(mut tools_box) = app.room_tools.take() else {
        return;
    };
    let Some(idx) = tools_box.tools.iter().position(|t| t.id == tool_id) else {
        app.room_tools = Some(tools_box);
        return;
    };
    let tool = tools_box.tools[idx].clone();
    let is_mgr = app.is_room_manager();
    let self_uid = app.self_user_id.clone();
    let can_use = can_use_tool(&tool, self_uid.as_deref(), is_mgr);
    let can_close = can_close_tool(&tool.created_by_id, is_mgr, self_uid.as_deref());
    let tile_id = format!("tool:{}", tool.id);
    let reactions_tid = tools_box.reactions_tool_id().map(str::to_string);
    let picker_open =
        tools_box.open_reaction_picker_tile.as_deref() == Some(tile_id.as_str());
    let peer_names: Vec<String> = std::iter::once(app.effective_name().to_string())
        .chain(
            app.peers
                .iter()
                .filter(|p| !p.is_obs() && !p.is_silent_moderator())
                .map(|p| p.name.clone()),
        )
        .collect();
    let assignable = collect_room_people(app);

    let mut decoded_imgs: std::collections::HashMap<String, egui::TextureId> =
        std::collections::HashMap::new();
    if let ToolPayload::Whiteboard { ref strokes } | ToolPayload::Annotate { ref strokes } =
        tool.payload
    {
        for s in strokes {
            if s.shape == "image" {
                if let Some(ref src) = s.src {
                    if let Some(tid) = app.get_or_decode_wb_image(&s.id, src) {
                        decoded_imgs.insert(s.id.clone(), tid);
                    }
                }
            }
        }
    }

    let st = &mut *tools_box;
    let mut cmds: Vec<Value> = Vec::new();
    let mut toggle_focus = false;
    let mut toggle_fullscreen = false;
    let mut close_tool = false;
    let mut rx_toggle_rect = Rect::NOTHING;

    let pad = if size.x < 420.0 { 6.0 } else { 9.0 };
    let (tile_rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let tile_clip = ui.clip_rect().intersect(tile_rect);
    let rounding = Rounding::same(if is_fullscreen { 0.0 } else { 12.0 });
    ui.painter().with_clip_rect(tile_clip).rect(
        tile_rect,
        rounding,
        Color32::from_rgb(22, 27, 34),
        Stroke::new(
            if is_focused { 1.5_f32 } else { 1.0_f32 },
            if is_focused { BLUE } else { BORDER_SUBTLE },
        ),
    );

    let inner_rect = tile_rect.shrink(pad);
    let inner_w = inner_rect.width().max(140.0);
    let compact_hdr = inner_w < 430.0;
    let hdr_h = if compact_hdr { 24.0 } else { 28.0 };
    let hdr_rect = Rect::from_min_size(inner_rect.min, Vec2::new(inner_w, hdr_h));
    let body_rect = Rect::from_min_max(
        pos2(inner_rect.left(), (hdr_rect.bottom() + 4.0).min(inner_rect.bottom())),
        inner_rect.max,
    );

    let mut hdr_ui = ui.new_child(egui::UiBuilder::new().max_rect(hdr_rect));
    hdr_ui.set_clip_rect(tile_clip.intersect(hdr_rect));
    hdr_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = if compact_hdr { 4.0 } else { 6.0 };
        let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        icons::paint_tool_kind_icon(
            ui.painter(),
            ir.center(),
            14.0,
            &tool.kind,
            EMERALD_BRIGHT,
        );
        let max_title_w = (inner_w - if compact_hdr { 115.0 } else { 230.0 }).max(40.0);
        ui.allocate_ui_with_layout(
            Vec2::new(max_title_w, hdr_h),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&tool.title)
                            .size(if compact_hdr { 12.0 } else { 13.5 })
                            .semibold()
                            .color(TEXT_PRIMARY),
                    )
                    .truncate(),
                );
                if !compact_hdr {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(5, 120, 82))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(7.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(kind_badge_upper(&tool.kind))
                                    .size(9.5)
                                    .semibold()
                                    .color(Color32::WHITE),
                            );
                        });
                }
                if !can_use && !compact_hdr {
                    ui.label(RichText::new("• Somente gerentes").size(10.0).color(GOLD));
                }
            },
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let btn_d = if compact_hdr { 22.0 } else { 26.0 };
            let dark_c = Color32::from_rgba_unmultiplied(30, 35, 46, 220);

            if can_close {
                let xr = icons::media_overlay_circle_button(
                    ui,
                    btn_d,
                    dark_c,
                    "Fechar ferramenta para todos",
                    |p, c, col| icons::paint_close_x(p, c, 11.5, col),
                    Color32::from_rgb(248, 113, 113),
                );
                if xr.clicked() {
                    close_tool = true;
                }
            }

            let fsr = icons::media_overlay_circle_button(
                ui,
                btn_d,
                if is_fullscreen { BLUE } else { dark_c },
                if is_fullscreen {
                    "Sair da tela cheia (Esc)"
                } else {
                    "Tela cheia"
                },
                |p, c, col| icons::paint_fullscreen(p, c, 12.0, col, is_fullscreen),
                Color32::WHITE,
            );
            if fsr.clicked() {
                toggle_fullscreen = true;
            }

            if !is_fullscreen {
                let fcr = icons::media_overlay_circle_button(
                    ui,
                    btn_d,
                    if is_focused { EMERALD } else { dark_c },
                    if is_focused {
                        "Sair do foco"
                    } else {
                        "Focar ferramenta"
                    },
                    |p, c, col| icons::paint_focus(p, c, 12.0, col),
                    Color32::WHITE,
                );
                if fcr.clicked() {
                    toggle_focus = true;
                }
            }

            if reactions_tid.is_some() {
                let rxr = icons::media_overlay_circle_button(
                    ui,
                    btn_d,
                    if picker_open { EMERALD } else { dark_c },
                    "Reagir",
                    |p, c, col| icons::paint_reaction_smile(p, c, 12.5, col),
                    Color32::WHITE,
                );
                rx_toggle_rect = rxr.rect;
                if rxr.clicked() {
                    if picker_open {
                        st.open_reaction_picker_tile = None;
                    } else {
                        st.open_reaction_picker_tile = Some(tile_id.clone());
                    }
                }
            }
        });
    });

    if body_rect.height() >= 40.0 {
        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        body_ui.set_clip_rect(tile_clip.intersect(body_rect));
        let ui = &mut body_ui;
        match &tool.payload {
            ToolPayload::Whiteboard { strokes } | ToolPayload::Annotate { strokes } => {
                render_whiteboard_view(
                    st,
                    ui,
                    &tool.id,
                    strokes,
                    &decoded_imgs,
                    self_uid.as_deref(),
                    can_use,
                    &mut cmds,
                );
            }
            ToolPayload::Text {
                text, language, ..
            } => {
                if tool.kind == "doc" {
                    render_doc_view(
                        st,
                        ui,
                        &tool.id,
                        &tool.title,
                        text,
                        can_use,
                        &mut cmds,
                    );
                } else {
                    render_text_tool_view(
                        st,
                        ui,
                        &tool.id,
                        &tool.kind,
                        language,
                        can_use,
                        is_mgr,
                        &mut cmds,
                    );
                }
            }
            ToolPayload::Wheel {
                entries,
                spin,
                history,
                remove_winner,
                announce,
                duration,
                ..
            } => {
                render_wheel_view(
                    st,
                    ui,
                    &tool.id,
                    entries,
                    spin.as_ref(),
                    history,
                    *remove_winner,
                    *announce,
                    *duration,
                    &peer_names,
                    is_mgr,
                    can_use,
                    &mut cmds,
                );
            }
            ToolPayload::Calc {
                expr,
                history,
                memory,
                angle,
            } => {
                render_calc_view(
                    st,
                    ui,
                    &tool.id,
                    expr,
                    history,
                    *memory,
                    angle,
                    can_use,
                    &mut cmds,
                );
            }
            ToolPayload::Sheet {
                rows,
                cols,
                cells,
                col_widths,
            } => {
                render_sheet_view(
                    st,
                    ui,
                    &tool.id,
                    &tool.title,
                    *rows,
                    *cols,
                    cells,
                    col_widths,
                    can_use,
                    &mut cmds,
                );
            }
            ToolPayload::Tasks { items, rules } => {
                render_tasks_view(
                    st,
                    ui,
                    &tool.id,
                    items,
                    rules,
                    self_uid.as_deref(),
                    can_use,
                    is_mgr,
                    &assignable,
                    &mut cmds,
                );
            }
            ToolPayload::Poll { .. } | ToolPayload::Reactions => {}
        }
    }

    // Picker + reacoes flutuantes por cima do tile da ferramenta
    if let Some(ref rtid) = reactions_tid {
        if picker_open && tile_rect != Rect::NOTHING {
            let pw = (tile_rect.width() - 24.0).clamp(180.0, 344.0);
            let prect = Rect::from_min_size(
                pos2(
                    (tile_rect.right() - pw - 10.0).max(tile_rect.left() + 8.0),
                    tile_rect.top() + 36.0,
                ),
                Vec2::new(pw, 36.0),
            );
            let area_resp = egui::Area::new(egui::Id::new(("tool_rx_picker", &tool.id)))
                .fixed_pos(prect.min)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(15, 18, 25, 244))
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(16.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                        .show(ui, |ui| {
                            ui.set_max_width(pw);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                                for &em in REACTION_EMOJIS {
                                    let (er, eresp) = ui.allocate_exact_size(
                                        Vec2::splat(24.0),
                                        Sense::click(),
                                    );
                                    if eresp.hovered() {
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
                                    if eresp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                        cmds.push(cmd_reaction(rtid, em, &tile_id));
                                    }
                                }
                            });
                        });
                });
            let outside_click = ui.ctx().input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| {
                        !area_resp.response.rect.contains(p) && !rx_toggle_rect.contains(p)
                    })
            });
            if outside_click || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                st.open_reaction_picker_tile = None;
            }
        }
        let now = Instant::now();
        let active: Vec<(&str, &str, f32, f32)> = st
            .reactions
            .iter()
            .filter(|r| r.target == tile_id)
            .map(|r| {
                (
                    r.emoji.as_str(),
                    r.name.as_str(),
                    now.saturating_duration_since(r.created_at).as_secs_f32(),
                    r.x_frac,
                )
            })
            .collect();
        if !active.is_empty() && tile_rect != Rect::NOTHING {
            paint_floating_reactions(ui.ctx(), ui.painter(), tile_rect, &active);
        }
    }

    if close_tool {
        cmds.push(cmd_close_tool(&tool.id));
    }
    for c in &cmds {
        if c.get("type").and_then(|v| v.as_str()) == Some("tool-sheet") {
            if let Some(ch) = c.get("change") {
                st.apply_sheet_change_local(&tool.id, ch);
            }
        }
    }
    app.room_tools = Some(tools_box);

    if toggle_focus {
        if is_focused {
            app.focused_tile_id = None;
        } else {
            app.focused_tile_id = Some(tile_id.clone());
        }
    }
    if toggle_fullscreen {
        app.toggle_fullscreen(tile_id, ui.ctx());
    }
    for c in cmds {
        app.send_tool_json(c);
    }
}

fn stroke_bounding_box_norm(s: &ToolStroke) -> Option<([f32; 2], [f32; 2])> {
    if s.points.len() < 2 {
        return None;
    }
    if s.shape == "image" && s.points.len() >= 4 {
        let x0 = s.points[0].min(s.points[2]);
        let y0 = s.points[1].min(s.points[3]);
        let x1 = s.points[0].max(s.points[2]);
        let y1 = s.points[1].max(s.points[3]);
        return Some(([x0, y0], [x1, y1]));
    }
    if s.shape == "text" {
        let txt_len = s.text.as_deref().unwrap_or("T").chars().count().max(2) as f32;
        let w = (txt_len * 0.014).clamp(0.06, 0.42);
        let h = (s.width * 0.008).clamp(0.04, 0.14);
        let x = s.points[0];
        let y = s.points[1];
        return Some(([x, y - h * 0.5], [x + w, y + h * 0.5]));
    }
    let mut min_x = 1.0_f32;
    let mut min_y = 1.0_f32;
    let mut max_x = 0.0_f32;
    let mut max_y = 0.0_f32;
    for ch in s.points.chunks_exact(2) {
        min_x = min_x.min(ch[0]);
        min_y = min_y.min(ch[1]);
        max_x = max_x.max(ch[0]);
        max_y = max_y.max(ch[1]);
    }
    let pad = 0.012;
    Some((
        [(min_x - pad).max(0.0), (min_y - pad).max(0.0)],
        [(max_x + pad).min(1.0), (max_y + pad).min(1.0)],
    ))
}

#[allow(clippy::too_many_arguments)]
fn render_whiteboard_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    strokes: &[ToolStroke],
    decoded_imgs: &std::collections::HashMap<String, egui::TextureId>,
    self_uid: Option<&str>,
    can_use: bool,
    cmds: &mut Vec<Value>,
) {
    let paste_image_clipboard =
        |st: &mut crate::net::room_tools::RoomToolsState, cmds: &mut Vec<Value>| {
            if let Some((iw, ih, rgba)) = crate::media::image_decode::read_clipboard_image_rgba() {
                if let Some(data_url) = encode_png_data_url(iw, ih, &rgba) {
                    let aspect = (iw as f32 / ih.max(1) as f32).clamp(0.25, 4.0);
                    let (mut bw, mut bh) = if aspect >= 1.0 {
                        (0.38_f32, 0.38_f32 * (16.0 / 9.0) / aspect)
                    } else {
                        (0.36_f32 * aspect * (9.0 / 16.0), 0.36_f32)
                    };
                    if bw > 0.72 {
                        let k = 0.72 / bw;
                        bw *= k;
                        bh *= k;
                    }
                    if bh > 0.72 {
                        let k = 0.72 / bh;
                        bw *= k;
                        bh *= k;
                    }
                    let x1 = 0.5 - bw * 0.5;
                    let y1 = 0.5 - bh * 0.5;
                    let x2 = 0.5 + bw * 0.5;
                    let y2 = 0.5 + bh * 0.5;
                    let cid = st.next_stroke_client_id();
                    cmds.push(cmd_image_stroke(tool_id, [x1, y1, x2, y2], &data_url, &cid));
                    st.pen_shape = "select".to_string();
                    st.wb_clipboard_error = None;
                    return;
                }
            }
            st.wb_clipboard_error =
                Some("Nenhuma imagem na area de transferencia (copie um print ou imagem e tente colar).".to_string());
        };

    // Barra compacta fiel ao GoLive oficial (em tiles menores rola horizontalmente para preservar area da lousa)
    let compact_wb_tb = ui.available_width() < 540.0;
    egui::Frame::none()
        .fill(BG_TILE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(14.0))
        .inner_margin(egui::Margin::symmetric(6.0, 3.0))
        .show(ui, |ui| {
            let draw_wb_toolbar = |ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                let tools = [
                    ("select", "Selecionar / Mover / Redimensionar"),
                    ("pen", "Caneta"),
                    ("highlighter", "Marca-texto"),
                    ("line", "Linha"),
                    ("arrow", "Seta"),
                    ("rect", "Retangulo"),
                    ("ellipse", "Circulo"),
                    ("text", "Texto"),
                    ("eraser", "Borracha"),
                ];
                for (id, tip) in tools {
                    let active = st.pen_shape == id;
                    let r = icons::icon_button(
                        ui,
                        Vec2::splat(22.0),
                        if active { EMERALD } else { BG_ELEVATED },
                        Stroke::NONE,
                        Rounding::same(11.0),
                        tip,
                        |p, c, col| match id {
                            "select" => icons::paint_pointer_select(p, c, 12.0, col),
                            "pen" => icons::paint_brush(p, c, 12.0, col),
                            "highlighter" => icons::paint_highlighter(p, c, 12.0, col),
                            "line" => icons::paint_line(p, c, 12.0, col),
                            "arrow" => icons::paint_arrow_ne(p, c, 12.0, col),
                            "rect" => icons::paint_rect_shape(p, c, 12.0, col),
                            "ellipse" => icons::paint_circle_shape(p, c, 12.0, col),
                            "text" => icons::paint_text_tt(p, c, 12.0, col),
                            _ => icons::paint_eraser(p, c, 12.0, col),
                        },
                        Color32::WHITE,
                    );
                    if r.clicked() {
                        st.pen_shape = id.to_string();
                    }
                }

                ui.separator();
                for &(hex, col) in WB_COLORS {
                    let selected = st.pen_color.eq_ignore_ascii_case(hex);
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 6.0, col);
                    ui.painter().circle_stroke(
                        rect.center(),
                        6.0,
                        Stroke::new(0.8_f32, Color32::from_rgb(100, 116, 139)),
                    );
                    if selected {
                        ui.painter().circle_stroke(
                            rect.center(),
                            7.4,
                            Stroke::new(1.5_f32, EMERALD_BRIGHT),
                        );
                    }
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        st.pen_color = hex.to_string();
                    }
                }

                ui.separator();
                for &w in WB_WIDTHS {
                    let selected = (st.pen_width - w).abs() < 0.2;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                    let r = (w * 0.38).clamp(2.0, 5.8);
                    ui.painter().circle_filled(
                        rect.center(),
                        r,
                        if selected {
                            EMERALD_BRIGHT
                        } else {
                            TEXT_SECONDARY
                        },
                    );
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        st.pen_width = w;
                    }
                }

                ui.separator();
                if icons::icon_button(
                    ui,
                    Vec2::splat(22.0),
                    BG_ELEVATED,
                    Stroke::NONE,
                    Rounding::same(11.0),
                    "Colar imagem da area de transferencia (Ctrl+V)",
                    |p, c, col| icons::paint_image_frame(p, c, 12.0, col),
                    TEXT_PRIMARY,
                )
                .clicked()
                    && can_use
                {
                    paste_image_clipboard(st, cmds);
                }
                if icons::icon_button(
                    ui,
                    Vec2::splat(22.0),
                    BG_ELEVATED,
                    Stroke::NONE,
                    Rounding::same(11.0),
                    "Desfazer ultimo traco",
                    |p, c, col| icons::paint_undo(p, c, 12.0, col),
                    TEXT_PRIMARY,
                )
                .clicked()
                    && can_use
                {
                    if let Some(last) = strokes
                        .iter()
                        .rev()
                        .find(|s| self_uid.is_none() || self_uid == Some(s.by.as_str()))
                        .or_else(|| strokes.last())
                    {
                        cmds.push(cmd_remove_strokes(tool_id, &[last.id.clone()]));
                    }
                }
                if icons::icon_button(
                    ui,
                    Vec2::splat(22.0),
                    BG_ELEVATED,
                    Stroke::NONE,
                    Rounding::same(11.0),
                    "Limpar lousa",
                    |p, c, col| icons::paint_trash(p, c, 12.0, col),
                    Color32::from_rgb(248, 113, 113),
                )
                .clicked()
                    && can_use
                {
                    st.wb_selected_stroke.remove(tool_id);
                    cmds.push(cmd_clear_strokes(tool_id));
                }
            };
            if compact_wb_tb {
                egui::ScrollArea::horizontal()
                    .id_salt(("wb_tb_scroll", tool_id))
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                    )
                    .show(ui, |ui| {
                        ui.horizontal(draw_wb_toolbar);
                    });
            } else {
                ui.horizontal_wrapped(draw_wb_toolbar);
            }
        });

    if let Some(ref err) = st.wb_clipboard_error.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(err).size(10.5).color(GOLD));
            if ui.small_button("×").clicked() {
                st.wb_clipboard_error = None;
            }
        });
    }

    if st.pen_shape == "text" && can_use {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Texto:").size(11.0).color(TEXT_SECONDARY));
            ui.add_sized(
                Vec2::new((ui.available_width() - 56.0).min(220.0), 22.0),
                egui::TextEdit::singleline(&mut st.wb_text_draft)
                    .hint_text("Digite e clique no quadro branco..."),
            );
        });
    }

    ui.add_space(3.0);
    let avail = ui.available_size();
    let (canvas_rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
    // Letterbox preto fora da folha 16:9 + quadro BRANCO igual ao GoLive Web
    ui.painter()
        .rect_filled(canvas_rect, Rounding::same(8.0), Color32::from_rgb(13, 17, 23));
    let bw = canvas_rect
        .width()
        .min(canvas_rect.height() * (16.0 / 9.0))
        .max(40.0);
    let bh = (bw * (9.0 / 16.0)).max(24.0);
    let board_rect = Rect::from_center_size(canvas_rect.center(), Vec2::new(bw, bh));

    let painter = ui.painter_at(board_rect);
    painter.rect(
        board_rect,
        Rounding::same(6.0),
        Color32::WHITE,
        Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225)),
    );

    let scale = (board_rect.width() / 1000.0).max(0.15);
    let to_screen = |nx: f32, ny: f32| -> egui::Pos2 {
        pos2(
            board_rect.left() + nx * board_rect.width(),
            board_rect.top() + ny * board_rect.height(),
        )
    };
    let to_norm = |pos: egui::Pos2| -> [f32; 2] {
        [
            ((pos.x - board_rect.left()) / board_rect.width().max(1.0)).clamp(0.0, 1.0),
            ((pos.y - board_rect.top()) / board_rect.height().max(1.0)).clamp(0.0, 1.0),
        ]
    };

    let draw_shape = |p: &egui::Painter,
                      stroke_id: Option<&str>,
                      shape: &str,
                      col_hex: &str,
                      width: f32,
                      pts: &[f32],
                      text_opt: Option<&str>| {
        if shape == "image" && pts.len() >= 4 {
            let p0 = to_screen(pts[0], pts[1]);
            let p1 = to_screen(pts[2], pts[3]);
            let r = Rect::from_two_pos(p0, p1);
            if let Some(tid) = stroke_id.and_then(|sid| decoded_imgs.get(sid).copied()) {
                p.image(
                    tid,
                    r,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                p.rect(
                    r,
                    Rounding::same(3.0),
                    Color32::from_rgb(241, 245, 249),
                    Stroke::new(1.0_f32, Color32::from_rgb(148, 163, 184)),
                );
                p.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "Imagem",
                    FontId::proportional(11.0),
                    Color32::from_rgb(100, 116, 139),
                );
            }
            return;
        }

        let mut c = parse_hex_color32(col_hex);
        let mut sw = width * scale;
        if shape == "highlighter" {
            c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 100);
            sw = (width * 2.5 * scale).max(6.0 * scale);
        }
        let st_stroke = Stroke::new(sw.max(1.0), c);
        match shape {
            "line" | "arrow" if pts.len() >= 4 => {
                let p0 = to_screen(pts[0], pts[1]);
                let p1 = to_screen(pts[pts.len() - 2], pts[pts.len() - 1]);
                p.line_segment([p0, p1], st_stroke);
                if shape == "arrow" && p1.distance(p0) > 1.0 {
                    // Ponta triangular preenchida exata igual `strokes.ts:116-125` (`angle +- PI/7`, `head = max(10, sw*3.5)`)
                    let angle = (p1.y - p0.y).atan2(p1.x - p0.x);
                    let head = (sw * 3.5).max(10.0 * scale);
                    let spread = std::f32::consts::PI / 7.0;
                    let a1 = angle - spread;
                    let a2 = angle + spread;
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            p1,
                            pos2(p1.x - head * a1.cos(), p1.y - head * a1.sin()),
                            pos2(p1.x - head * a2.cos(), p1.y - head * a2.sin()),
                        ],
                        c,
                        Stroke::NONE,
                    ));
                }
            }
            "rect" if pts.len() >= 4 => {
                let p0 = to_screen(pts[0], pts[1]);
                let p1 = to_screen(pts[pts.len() - 2], pts[pts.len() - 1]);
                p.rect_stroke(Rect::from_two_pos(p0, p1), Rounding::same(2.0), st_stroke);
            }
            "ellipse" if pts.len() >= 4 => {
                let p0 = to_screen(pts[0], pts[1]);
                let p1 = to_screen(pts[pts.len() - 2], pts[pts.len() - 1]);
                let r = Rect::from_two_pos(p0, p1);
                let mut poly = Vec::with_capacity(32);
                for i in 0..32 {
                    let a = (i as f32) * std::f32::consts::TAU / 32.0;
                    poly.push(pos2(
                        r.center().x + a.cos() * (r.width() * 0.5),
                        r.center().y + a.sin() * (r.height() * 0.5),
                    ));
                }
                p.add(egui::Shape::closed_line(poly, st_stroke));
            }
            "text" if pts.len() >= 2 => {
                let p0 = to_screen(pts[0], pts[1]);
                if let Some(txt) = text_opt {
                    p.text(
                        p0,
                        egui::Align2::LEFT_CENTER,
                        txt,
                        FontId::proportional((sw * 3.8).clamp(11.0, 28.0)),
                        c,
                    );
                }
            }
            _ if pts.len() >= 4 => {
                let screen_pts: Vec<egui::Pos2> =
                    pts.chunks_exact(2).map(|ch| to_screen(ch[0], ch[1])).collect();
                p.add(egui::Shape::line(screen_pts, st_stroke));
            }
            _ => {}
        }
    };

    // Aplica preview ao vivo durante arrasto de mover/redimensionar
    let active_drag_id = st
        .wb_move_drag
        .as_ref()
        .filter(|d| d.tool_id == tool_id)
        .map(|d| (d.stroke_id.clone(), d.orig_points.clone(), d.mode, d.start_norm));

    for s in strokes {
        draw_shape(
            &painter,
            Some(&s.id),
            &s.shape,
            &s.color,
            s.width,
            &s.points,
            s.text.as_deref(),
        );
    }

    if let Some(live_map) = st.wb_live_strokes.get(tool_id) {
        for ls in live_map.values() {
            draw_shape(
                &painter,
                None,
                &ls.shape,
                &ls.color,
                ls.width,
                &ls.points,
                ls.text.as_deref(),
            );
        }
    }

    if !st.active_draw_points.is_empty() {
        draw_shape(
            &painter,
            None,
            &st.pen_shape,
            &st.pen_color,
            st.pen_width,
            &st.active_draw_points,
            None,
        );
    }

    // Ctrl+V cola imagem da area de transferencia diretamente na Lousa
    if can_use && resp.hovered() {
        let paste_pressed = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::V));
        if paste_pressed {
            paste_image_clipboard(st, cmds);
        }
    }

    if !can_use {
        return;
    }

    // Modo Selecionar / Mover / Redimensionar (como `PasteMoveLayer.tsx`)
    if st.pen_shape == "select" {
        let sel_id = st.wb_selected_stroke.get(tool_id).cloned();
        for s in strokes {
            let is_sel = sel_id.as_deref() == Some(s.id.as_str());
            if !is_sel && s.shape != "image" && s.shape != "text" {
                continue;
            }
            if let Some((b0, b1)) = stroke_bounding_box_norm(s) {
                let br = Rect::from_two_pos(to_screen(b0[0], b0[1]), to_screen(b1[0], b1[1]));
                let col = if is_sel {
                    EMERALD
                } else {
                    Color32::from_rgba_unmultiplied(16, 185, 129, 110)
                };
                painter.rect_stroke(br, Rounding::same(2.0), Stroke::new(1.5_f32, col));
                if is_sel && s.shape == "image" {
                    let handle = Rect::from_center_size(br.right_bottom(), Vec2::splat(10.0));
                    painter.rect_filled(handle, Rounding::same(2.0), EMERALD);
                }
            }
        }

        // Delete/Backspace remove traco selecionado
        if let Some(ref sid) = sel_id {
            if resp.hovered()
                && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            {
                cmds.push(cmd_remove_strokes(tool_id, &[sid.clone()]));
                st.wb_selected_stroke.remove(tool_id);
            }
        }

        // Inicia clique/arrasto para mover ou redimensionar
        if resp.drag_started() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let norm = to_norm(pos);
                let mut matched: Option<(String, WbDragMode, Vec<f32>)> = None;

                if let Some(ref sid) = sel_id {
                    if let Some(s) = strokes.iter().find(|x| &x.id == sid) {
                        if s.shape == "image" && s.points.len() >= 4 {
                            let rb = to_screen(
                                s.points[0].max(s.points[2]),
                                s.points[1].max(s.points[3]),
                            );
                            if rb.distance(pos) <= 14.0 {
                                matched = Some((s.id.clone(), WbDragMode::Resize, s.points.clone()));
                            }
                        }
                    }
                }
                if matched.is_none() {
                    for s in strokes.iter().rev() {
                        if let Some((b0, b1)) = stroke_bounding_box_norm(s) {
                            let br = Rect::from_two_pos(
                                to_screen(b0[0], b0[1]),
                                to_screen(b1[0], b1[1]),
                            )
                            .expand(6.0);
                            if br.contains(pos) {
                                matched = Some((s.id.clone(), WbDragMode::Move, s.points.clone()));
                                break;
                            }
                        }
                    }
                }
                if let Some((sid, mode, orig_pts)) = matched {
                    st.wb_selected_stroke.insert(tool_id.to_string(), sid.clone());
                    if resp.drag_started() {
                        st.wb_move_drag = Some(WbMoveDrag {
                            tool_id: tool_id.to_string(),
                            stroke_id: sid,
                            mode,
                            start_norm: norm,
                            orig_points: orig_pts,
                        });
                    }
                } else if resp.clicked() {
                    st.wb_selected_stroke.remove(tool_id);
                }
            }
        }

        if resp.dragged() {
            if let (Some((sid, orig_pts, mode, start_norm)), Some(pos)) =
                (active_drag_id, resp.interact_pointer_pos())
            {
                let cur_n = to_norm(pos);
                let dx = cur_n[0] - start_norm[0];
                let dy = cur_n[1] - start_norm[1];
                let mut next_pts = orig_pts.clone();
                match mode {
                    WbDragMode::Move => {
                        for ch in next_pts.chunks_exact_mut(2) {
                            ch[0] = (ch[0] + dx).clamp(0.0, 1.0);
                            ch[1] = (ch[1] + dy).clamp(0.0, 1.0);
                        }
                    }
                    WbDragMode::Resize if next_pts.len() >= 4 => {
                        let ow = (orig_pts[2] - orig_pts[0]).max(0.04);
                        let oh = (orig_pts[3] - orig_pts[1]).max(0.04);
                        let nw = (ow + dx).max(0.04).min(1.0 - orig_pts[0]);
                        let nh = (nw * (oh / ow)).max(0.04).min(1.0 - orig_pts[1]);
                        next_pts[2] = orig_pts[0] + nw;
                        next_pts[3] = orig_pts[1] + nh;
                    }
                    _ => {}
                }
                // Atualiza preview local em tempo real
                if let Some(tool) = st.tools.iter_mut().find(|t| t.id == tool_id) {
                    if let ToolPayload::Whiteboard { ref mut strokes }
                    | ToolPayload::Annotate { ref mut strokes } = tool.payload
                    {
                        if let Some(s) = strokes.iter_mut().find(|x| x.id == sid) {
                            s.points = next_pts;
                        }
                    }
                }
            }
        }

        if resp.drag_stopped() {
            if let Some(drag) = st.wb_move_drag.take() {
                if let Some(cur_pts) = st
                    .tools
                    .iter()
                    .find(|t| t.id == drag.tool_id)
                    .and_then(|t| match &t.payload {
                        ToolPayload::Whiteboard { strokes } | ToolPayload::Annotate { strokes } => {
                            strokes.iter().find(|s| s.id == drag.stroke_id).map(|s| s.points.clone())
                        }
                        _ => None,
                    })
                {
                    if cur_pts != drag.orig_points {
                        cmds.push(cmd_move_stroke(&drag.tool_id, &drag.stroke_id, &cur_pts));
                    }
                }
            }
        }
        return;
    }

    if st.pen_shape == "eraser" {
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.clicked() || resp.dragged() {
                let hit_ids: Vec<String> = strokes
                    .iter()
                    .filter(|s| {
                        if let Some((b0, b1)) = stroke_bounding_box_norm(s) {
                            let br = Rect::from_two_pos(
                                to_screen(b0[0], b0[1]),
                                to_screen(b1[0], b1[1]),
                            )
                            .expand(8.0);
                            if s.shape == "image" || s.shape == "text" {
                                return br.contains(pos);
                            }
                        }
                        s.points
                            .chunks_exact(2)
                            .any(|pt| to_screen(pt[0], pt[1]).distance(pos) <= 16.0)
                    })
                    .map(|s| s.id.clone())
                    .collect();
                if !hit_ids.is_empty() {
                    cmds.push(cmd_remove_strokes(tool_id, &hit_ids));
                }
            }
        }
    } else if st.pen_shape == "text" {
        if resp.clicked() && !st.wb_text_draft.trim().is_empty() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let n = to_norm(pos);
                let cid = st.next_stroke_client_id();
                cmds.push(json!({
                    "type": "tool-stroke",
                    "id": tool_id,
                    "clientId": cid,
                    "stroke": {
                        "shape": "text",
                        "color": st.pen_color,
                        "width": st.pen_width,
                        "points": [n[0], n[1]],
                        "text": st.wb_text_draft.trim(),
                    }
                }));
            }
        }
    } else {
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let n = to_norm(pos);
                if st.active_draw_points.is_empty() {
                    st.active_draw_points.push(n[0]);
                    st.active_draw_points.push(n[1]);
                } else if matches!(st.pen_shape.as_str(), "line" | "arrow" | "rect" | "ellipse") {
                    if st.active_draw_points.len() == 2 {
                        st.active_draw_points.push(n[0]);
                        st.active_draw_points.push(n[1]);
                    } else {
                        let len = st.active_draw_points.len();
                        st.active_draw_points[len - 2] = n[0];
                        st.active_draw_points[len - 1] = n[1];
                    }
                } else {
                    st.active_draw_points.push(n[0]);
                    st.active_draw_points.push(n[1]);
                }
            }
        }
        if resp.drag_stopped() && st.active_draw_points.len() >= 4 {
            let pts = std::mem::take(&mut st.active_draw_points);
            let cid = st.next_stroke_client_id();
            cmds.push(cmd_add_stroke(
                tool_id,
                &st.pen_shape,
                &st.pen_color,
                st.pen_width,
                &pts,
                &cid,
            ));
        }
    }
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn doc_runs_to_layout_job(
    runs: &[DocRun],
    font_size: f32,
    default_col: Color32,
    halign: Align,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob {
        halign,
        wrap: egui::text::TextWrapping {
            max_width: wrap_width.max(40.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = runs_plain_text(runs);
    if text.is_empty() {
        job.append("", 0.0, egui::TextFormat::simple(FontId::proportional(font_size), default_col));
        return job;
    }
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        let col = if r.style.link.is_some() {
            BLUE_LIGHT
        } else if let Some(ref c_hex) = r.style.color {
            parse_css_hex_color(c_hex)
                .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                .unwrap_or(default_col)
        } else if r.style.bold {
            Color32::WHITE
        } else {
            default_col
        };
        let eff_size = if r.style.bold { font_size + 0.6 } else { font_size };
        let fmt = egui::TextFormat {
            font_id: FontId::proportional(eff_size),
            color: col,
            italics: r.style.italic,
            underline: if r.style.underline || r.style.link.is_some() {
                Stroke::new(1.1_f32, col)
            } else {
                Stroke::NONE
            },
            strikethrough: if r.style.strike {
                Stroke::new(1.3_f32, col)
            } else {
                Stroke::NONE
            },
            background: if r.style.bold {
                Color32::from_rgba_unmultiplied(255, 255, 255, 14)
            } else {
                Color32::TRANSPARENT
            },
            extra_letter_spacing: if r.style.bold { 0.35 } else { 0.0 },
            ..Default::default()
        };
        job.append(&r.text, 0.0, fmt);
    }
    job
}

fn block_align_to_egui(al: DocAlign) -> Align {
    match al {
        DocAlign::Left => Align::LEFT,
        DocAlign::Center => Align::Center,
        DocAlign::Right => Align::RIGHT,
    }
}

/// Teclas que mudam a estrutura do documento (lidas antes do TextEdit consumir).
#[derive(Clone, Copy)]
enum DocKey {
    None,
    Enter(usize),
    BackAtStart,
    Up,
    Down,
}

/// Edicao estrutural aplicada depois de desenhar todos os blocos (os indices mudam).
#[derive(Clone, Copy)]
enum DocStructOp {
    SplitBlock(usize, usize),
    MergeBack(usize),
    SplitItem(usize, usize, usize),
    ItemBack(usize, usize),
    Focus(DocFocus),
    FocusEnd,
}

/// Como o contentEditable do site: Enter abre bloco novo, Backspace no inicio junta com o de
/// cima e, nas listas, setas sobem/descem de item. Shift+Enter continua sendo quebra de linha.
fn doc_take_key(ui: &egui::Ui, id: egui::Id, can_use: bool, multiline: bool) -> DocKey {
    if !can_use || !ui.memory(|m| m.has_focus(id)) {
        return DocKey::None;
    }
    let Some(range) = egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range())
    else {
        return DocKey::None;
    };
    let (a, b) = (range.primary.index, range.secondary.index);
    let (shift, other_mod) = ui.input(|i| {
        (i.modifiers.shift, i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt)
    });
    if a != b || shift || other_mod {
        return DocKey::None;
    }
    let take = |k: egui::Key| ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
    if take(egui::Key::Enter) {
        return DocKey::Enter(a);
    }
    if a == 0 && take(egui::Key::Backspace) {
        return DocKey::BackAtStart;
    }
    if !multiline {
        if take(egui::Key::ArrowUp) {
            return DocKey::Up;
        }
        if take(egui::Key::ArrowDown) {
            return DocKey::Down;
        }
    }
    DocKey::None
}

/// Um trecho editavel com estilos (paragrafo, titulo, citacao ou item de lista).
#[allow(clippy::too_many_arguments)]
fn doc_runs_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    runs: &mut Vec<DocRun>,
    font_sz: f32,
    col: Color32,
    halign: Align,
    typing: Option<&InlineStyle>,
    can_use: bool,
    multiline: bool,
    hint: &str,
    focus_at: Option<usize>,
) -> (egui::text_edit::TextEditOutput, bool) {
    if let Some(pos) = focus_at {
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let pos = pos.min(crate::net::doc_html::runs_char_len(runs));
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(pos),
        )));
        state.store(ui.ctx(), id);
        ui.memory_mut(|m| m.request_focus(id));
    }
    let mut plain = runs_plain_text(runs);
    let old = plain.clone();
    let runs_for_layout = runs.clone();
    let typing_owned = typing.cloned();
    let mut layouter = move |ui: &egui::Ui, text: &str, wrap_w: f32| {
        let dyn_runs = update_runs_with_typing_style(&runs_for_layout, text, typing_owned.as_ref());
        // Sempre montado alinhado a esquerda e depois deslocado linha a linha: o TextEdit do
        // egui 0.29 desenha a partir da borda esquerda e ignora o `halign` do texto (centralizado
        // ia para a esquerda e o alinhado a direita sumia para fora da caixa).
        let galley = ui.fonts(|f| f.layout_job(doc_runs_to_layout_job(&dyn_runs, font_sz, col, Align::LEFT, wrap_w)));
        align_galley_rows(galley, halign, wrap_w)
    };
    let edit = if multiline {
        egui::TextEdit::multiline(&mut plain).desired_rows(1)
    } else {
        egui::TextEdit::singleline(&mut plain)
    };
    let out = edit
        .id(id)
        .frame(false)
        .desired_width(ui.available_width())
        .hint_text(hint)
        .interactive(can_use)
        .layouter(&mut layouter)
        .show(ui);
    let changed = can_use && out.response.changed() && plain != old;
    if changed {
        *runs = update_runs_with_typing_style(runs, &plain, typing);
    }
    (out, changed)
}

/// Guarda qual bloco esta ativo e a selecao dele (a barra de ferramentas age sobre ela).
fn note_doc_focus(
    ui: &egui::Ui,
    tool_id: &str,
    bidx: usize,
    out: &egui::text_edit::TextEditOutput,
    active: &mut Option<usize>,
) {
    if !(out.response.has_focus() || out.response.clicked()) {
        return;
    }
    *active = Some(bidx);
    if let Some(cr) = out.cursor_range {
        let (a, b) = (cr.primary.ccursor.index, cr.secondary.ccursor.index);
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new(("doc_last_sel", tool_id, bidx)), (a.min(b), a.max(b)))
        });
    }
}

fn render_doc_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    title: &str,
    _server_text: &str,
    can_use: bool,
    cmds: &mut Vec<Value>,
) {
    let cur = st
        .text_syncs
        .get(tool_id)
        .map(|s| s.text.clone())
        .unwrap_or_default();

    let cur_hash = hash_str(&cur);
    let mut doc = match st.doc_parsed_cache.get(tool_id) {
        Some((h, cached)) if *h == cur_hash => cached.clone(),
        _ => {
            let parsed = parse_doc_html(&cur);
            st.doc_parsed_cache
                .insert(tool_id.to_string(), (cur_hash, parsed.clone()));
            parsed
        }
    };

    let active_idx = st
        .doc_active_block
        .get(tool_id)
        .copied()
        .unwrap_or(0)
        .min(doc.blocks.len().saturating_sub(1));
    st.doc_active_block.insert(tool_id.to_string(), active_idx);

    let commit_doc = |next_doc: &DocDocument,
                      st: &mut crate::net::room_tools::RoomToolsState,
                      cmds: &mut Vec<Value>| {
        let next_html = serialize_doc_html(next_doc);
        let h = hash_str(&next_html);
        st.doc_parsed_cache
            .insert(tool_id.to_string(), (h, next_doc.clone()));
        if next_html != cur {
            st.doc_undo
                .entry(tool_id.to_string())
                .or_default()
                .push(cur.clone());
            st.doc_redo.entry(tool_id.to_string()).or_default().clear();
            if let Some(op) = st.edit_text_tool(tool_id, &next_html) {
                cmds.push(op);
            }
        }
    };

    // Detecta selecao atual no bloco ativo
    let sel_mem_id = egui::Id::new(("doc_last_sel", tool_id, active_idx));
    let (sel_lo, sel_hi): (usize, usize) = ui
        .ctx()
        .data(|d| d.get_temp(sel_mem_id))
        .unwrap_or((0, 0));
    let has_selection = sel_hi > sel_lo;

    let cur_kind = doc.blocks.get(active_idx).and_then(|b| b.kind());
    let cur_align = doc
        .blocks
        .get(active_idx)
        .map(|b| b.align())
        .unwrap_or(DocAlign::Left);
    let pending_st = st.doc_pending_style.get(tool_id).cloned().unwrap_or_default();
    let cur_runs = doc.blocks.get(active_idx).and_then(|b| b.runs());
    let is_bold_active = pending_st.bold
        || (has_selection && cur_runs.is_some_and(|rs| rs.iter().any(|r| r.style.bold)));
    let is_italic_active = pending_st.italic
        || (has_selection && cur_runs.is_some_and(|rs| rs.iter().any(|r| r.style.italic)));
    let is_under_active = pending_st.underline
        || (has_selection && cur_runs.is_some_and(|rs| rs.iter().any(|r| r.style.underline)));
    let is_strike_active = pending_st.strike
        || (has_selection && cur_runs.is_some_and(|rs| rs.iter().any(|r| r.style.strike)));

    // Limpa aviso de exportacao expirado (3.2s)
    let notice_msg = st.doc_export_notice.get(tool_id).and_then(|(msg, ts)| {
        if ts.elapsed().as_secs_f32() < 3.2 {
            Some(msg.clone())
        } else {
            None
        }
    });
    if notice_msg.is_none() {
        st.doc_export_notice.remove(tool_id);
    }

    // Atalhos Ctrl+B / I / U / Z / Y quando algum bloco do documento estiver focado
    if can_use && ui.ctx().wants_keyboard_input() {
        let blk_id = egui::Id::new(("doc_blk", tool_id, active_idx));
        let focused_here = ui.ctx().memory(|m| m.has_focus(blk_id));
        if focused_here {
            let (kb_b, kb_i, kb_u, kb_z, kb_y) = ui.ctx().input(|i| {
                let mod_key = i.modifiers.ctrl || i.modifiers.command;
                (
                    mod_key && i.key_pressed(egui::Key::B),
                    mod_key && i.key_pressed(egui::Key::I),
                    mod_key && i.key_pressed(egui::Key::U),
                    mod_key && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                    mod_key
                        && (i.key_pressed(egui::Key::Y)
                            || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                )
            });
            if kb_z {
                if let Some(prev) = st.doc_undo.get_mut(tool_id).and_then(|v| v.pop()) {
                    st.doc_redo
                        .entry(tool_id.to_string())
                        .or_default()
                        .push(cur.clone());
                    if let Some(op) = st.edit_text_tool(tool_id, &prev) {
                        cmds.push(op);
                    }
                }
            } else if kb_y {
                if let Some(next) = st.doc_redo.get_mut(tool_id).and_then(|v| v.pop()) {
                    st.doc_undo
                        .entry(tool_id.to_string())
                        .or_default()
                        .push(cur.clone());
                    if let Some(op) = st.edit_text_tool(tool_id, &next) {
                        cmds.push(op);
                    }
                }
            } else if kb_b || kb_i || kb_u {
                if has_selection {
                    if let Some(runs) = doc.blocks.get_mut(active_idx).and_then(|b| b.runs_mut()) {
                        *runs = if kb_b {
                            toggle_runs_style(runs, sel_lo, sel_hi, |s, on| s.bold = on, |s| s.bold)
                        } else if kb_i {
                            toggle_runs_style(
                                runs,
                                sel_lo,
                                sel_hi,
                                |s, on| s.italic = on,
                                |s| s.italic,
                            )
                        } else {
                            toggle_runs_style(
                                runs,
                                sel_lo,
                                sel_hi,
                                |s, on| s.underline = on,
                                |s| s.underline,
                            )
                        };
                        commit_doc(&doc, st, cmds);
                    }
                } else {
                    let ps = st.doc_pending_style.entry(tool_id.to_string()).or_default();
                    if kb_b {
                        ps.bold = !ps.bold;
                    } else if kb_i {
                        ps.italic = !ps.italic;
                    } else if kb_u {
                        ps.underline = !ps.underline;
                    }
                }
            }
        }
    }

    // Toolbar em linha unica com scroll horizontal discreto (igual DocView.tsx)
    egui::Frame::none()
        .fill(BG_TILE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(("doc_tb_scroll", tool_id))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
                        let style_lbl = match cur_kind {
                            Some(DocBlockKind::Heading1) => "Titulo 1",
                            Some(DocBlockKind::Heading2) => "Titulo 2",
                            Some(DocBlockKind::Heading3) => "Titulo 3",
                            _ => "Estilo",
                        };
                        egui::ComboBox::from_id_salt(("doc_style", tool_id))
                            .selected_text(style_lbl)
                            .width(76.0)
                            .show_ui(ui, |ui| {
                                for (lbl, target_k) in [
                                    ("Paragrafo", DocBlockKind::Paragraph),
                                    ("Titulo 1", DocBlockKind::Heading1),
                                    ("Titulo 2", DocBlockKind::Heading2),
                                    ("Titulo 3", DocBlockKind::Heading3),
                                ] {
                                    let is_sel = cur_kind == Some(target_k);
                                    if ui.selectable_label(is_sel, lbl).clicked() && can_use {
                                        doc.set_block_kind(active_idx, target_k);
                                        commit_doc(&doc, st, cmds);
                                    }
                                }
                            });

                        let mut apply_inline_btn = |is_set: fn(&InlineStyle) -> bool,
                                                    toggle: fn(&mut InlineStyle, bool)| {
                            if has_selection {
                                if let Some(runs) =
                                    doc.blocks.get_mut(active_idx).and_then(|b| b.runs_mut())
                                {
                                    *runs = toggle_runs_style(runs, sel_lo, sel_hi, toggle, is_set);
                                    commit_doc(&doc, st, cmds);
                                }
                            } else {
                                let ps = st
                                    .doc_pending_style
                                    .entry(tool_id.to_string())
                                    .or_default();
                                let next = !is_set(ps);
                                toggle(ps, next);
                            }
                        };

                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("B").size(12.0).semibold().color(Color32::WHITE),
                                )
                                .fill(if is_bold_active { EMERALD } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Negrito (Ctrl+B)")
                            .clicked()
                        {
                            apply_inline_btn(|s| s.bold, |s, on| s.bold = on);
                        }
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("I").size(12.0).italics().color(Color32::WHITE),
                                )
                                .fill(if is_italic_active { EMERALD } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Italico (Ctrl+I)")
                            .clicked()
                        {
                            apply_inline_btn(|s| s.italic, |s, on| s.italic = on);
                        }
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("U")
                                        .size(12.0)
                                        .underline()
                                        .color(Color32::WHITE),
                                )
                                .fill(if is_under_active { EMERALD } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Sublinhado (Ctrl+U)")
                            .clicked()
                        {
                            apply_inline_btn(|s| s.underline, |s, on| s.underline = on);
                        }
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("S")
                                        .size(12.0)
                                        .strikethrough()
                                        .color(Color32::WHITE),
                                )
                                .fill(if is_strike_active { EMERALD } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Tachado")
                            .clicked()
                        {
                            apply_inline_btn(|s| s.strike, |s, on| s.strike = on);
                        }

                        let color_pop =
                            st.doc_color_popover.get(tool_id).copied().unwrap_or(false);
                        let active_text_col = pending_st
                            .color
                            .as_deref()
                            .and_then(parse_css_hex_color)
                            .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                            .unwrap_or(Color32::WHITE);
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("A")
                                        .size(12.0)
                                        .underline()
                                        .semibold()
                                        .color(active_text_col),
                                )
                                .fill(if color_pop { BLUE } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Cor do texto")
                            .clicked()
                        {
                            st.doc_color_popover.insert(tool_id.to_string(), !color_pop);
                            st.doc_link_popover.insert(tool_id.to_string(), false);
                        }

                        ui.separator();
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            if cur_kind == Some(DocBlockKind::BulletList) {
                                EMERALD
                            } else {
                                BG_ELEVATED
                            },
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Lista com marcadores",
                            |p, c, col| icons::paint_list_bullet(p, c, 12.0, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            let tgt = if cur_kind == Some(DocBlockKind::BulletList) {
                                DocBlockKind::Paragraph
                            } else {
                                DocBlockKind::BulletList
                            };
                            doc.set_block_kind(active_idx, tgt);
                            commit_doc(&doc, st, cmds);
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            if cur_kind == Some(DocBlockKind::OrderedList) {
                                EMERALD
                            } else {
                                BG_ELEVATED
                            },
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Lista numerada",
                            |p, c, col| icons::paint_list_number(p, c, 12.0, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            let tgt = if cur_kind == Some(DocBlockKind::OrderedList) {
                                DocBlockKind::Paragraph
                            } else {
                                DocBlockKind::OrderedList
                            };
                            doc.set_block_kind(active_idx, tgt);
                            commit_doc(&doc, st, cmds);
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            if cur_kind == Some(DocBlockKind::Blockquote) {
                                EMERALD
                            } else {
                                BG_ELEVATED
                            },
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Citacao",
                            |p, c, col| icons::paint_quote(p, c, 12.0, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            let tgt = if cur_kind == Some(DocBlockKind::Blockquote) {
                                DocBlockKind::Paragraph
                            } else {
                                DocBlockKind::Blockquote
                            };
                            doc.set_block_kind(active_idx, tgt);
                            commit_doc(&doc, st, cmds);
                        }

                        for (mode, al, tip) in [
                            (0u8, DocAlign::Left, "Alinhar esquerda"),
                            (1, DocAlign::Center, "Centralizar"),
                            (2, DocAlign::Right, "Alinhar direita"),
                        ] {
                            if icons::icon_button(
                                ui,
                                Vec2::splat(22.0),
                                if cur_align == al {
                                    EMERALD
                                } else {
                                    Color32::TRANSPARENT
                                },
                                Stroke::NONE,
                                Rounding::same(5.0),
                                tip,
                                |p, c, col| icons::paint_align_lines(p, c, 11.5, col, mode),
                                TEXT_PRIMARY,
                            )
                            .clicked()
                                && can_use
                            {
                                doc.set_block_align(active_idx, al);
                                commit_doc(&doc, st, cmds);
                            }
                        }

                        ui.separator();
                        let link_pop =
                            st.doc_link_popover.get(tool_id).copied().unwrap_or(false);
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            if link_pop { BLUE } else { BG_ELEVATED },
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Inserir link",
                            |p, c, col| icons::paint_link(p, c, 12.0, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            st.doc_link_popover.insert(tool_id.to_string(), !link_pop);
                            st.doc_color_popover.insert(tool_id.to_string(), false);
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            BG_ELEVATED,
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Inserir tabela 3x3",
                            |p, c, col| icons::paint_table(p, c, 12.0, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            doc.insert_table_after(active_idx, 3, 3);
                            st.doc_active_block.insert(tool_id.to_string(), active_idx + 1);
                            commit_doc(&doc, st, cmds);
                        }
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new("—")
                                    .fill(BG_ELEVATED)
                                    .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Linha horizontal")
                            .clicked()
                        {
                            doc.insert_hr_after(active_idx);
                            commit_doc(&doc, st, cmds);
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            BG_ELEVATED,
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Limpar formatacao",
                            |p, c, col| icons::paint_clear_format(p, c, 11.5, col),
                            TEXT_SECONDARY,
                        )
                        .clicked()
                            && can_use
                        {
                            st.doc_pending_style.remove(tool_id);
                            doc.clear_block_formatting(active_idx);
                            commit_doc(&doc, st, cmds);
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            BG_ELEVATED,
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Desfazer (Ctrl+Z)",
                            |p, c, col| icons::paint_undo(p, c, 11.5, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            if let Some(prev) =
                                st.doc_undo.get_mut(tool_id).and_then(|v| v.pop())
                            {
                                st.doc_redo
                                    .entry(tool_id.to_string())
                                    .or_default()
                                    .push(cur.clone());
                                if let Some(op) = st.edit_text_tool(tool_id, &prev) {
                                    cmds.push(op);
                                }
                            }
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            BG_ELEVATED,
                            Stroke::NONE,
                            Rounding::same(5.0),
                            "Refazer (Ctrl+Y)",
                            |p, c, col| icons::paint_redo(p, c, 11.5, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            if let Some(next) =
                                st.doc_redo.get_mut(tool_id).and_then(|v| v.pop())
                            {
                                st.doc_undo
                                    .entry(tool_id.to_string())
                                    .or_default()
                                    .push(cur.clone());
                                if let Some(op) = st.edit_text_tool(tool_id, &next) {
                                    cmds.push(op);
                                }
                            }
                        }

                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Word").size(10.5).color(TEXT_SECONDARY),
                                )
                                .fill(BG_ELEVATED)
                                .rounding(Rounding::same(5.0))
                                .min_size(Vec2::new(42.0, 22.0)),
                            )
                            .on_hover_text("Baixar como arquivo Word (.doc)")
                            .clicked()
                        {
                            let doc_bytes = doc_to_word_html(&cur, title);
                            let name = if title.trim().is_empty() {
                                "documento"
                            } else {
                                title.trim()
                            };
                            if let Some(path) =
                                save_export_to_downloads(name, "doc", doc_bytes.as_bytes())
                            {
                                st.doc_export_notice.insert(
                                    tool_id.to_string(),
                                    (
                                        format!(
                                            "Salvo em {}",
                                            path.file_name()
                                                .and_then(|f| f.to_str())
                                                .unwrap_or("Downloads")
                                        ),
                                        Instant::now(),
                                    ),
                                );
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("PDF").size(10.5).color(TEXT_SECONDARY),
                                )
                                .fill(BG_ELEVATED)
                                .rounding(Rounding::same(5.0))
                                .min_size(Vec2::new(42.0, 22.0)),
                            )
                            .on_hover_text("Imprimir ou salvar em PDF")
                            .clicked()
                        {
                            // Igual ao site: abre a pagina de impressao (o dialogo tem "Salvar como PDF").
                            let msg = match crate::net::doc_html::print_doc_via_browser(&cur, title) {
                                Ok(_) => "Abrindo impressão — escolha \"Salvar como PDF\"".to_string(),
                                Err(e) => format!("Não foi possível imprimir: {e}"),
                            };
                            st.doc_export_notice
                                .insert(tool_id.to_string(), (msg, Instant::now()));
                        }
                    });
                });

            // Sub-barra cor do texto
            if st.doc_color_popover.get(tool_id).copied().unwrap_or(false) && can_use {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Cor:").size(11.0).color(TEXT_MUTED));
                    for &(lbl, hex, col) in DOC_TEXT_COLORS {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 7.0, col);
                        if resp
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(lbl)
                            .clicked()
                        {
                            let chosen = if hex.is_empty() {
                                None
                            } else {
                                Some(hex.to_string())
                            };
                            if has_selection {
                                if let Some(runs) =
                                    doc.blocks.get_mut(active_idx).and_then(|b| b.runs_mut())
                                {
                                    *runs = toggle_runs_style(
                                        runs,
                                        sel_lo,
                                        sel_hi,
                                        |s, _| s.color = chosen.clone(),
                                        |s| s.color == chosen,
                                    );
                                    commit_doc(&doc, st, cmds);
                                }
                            } else {
                                st.doc_pending_style
                                    .entry(tool_id.to_string())
                                    .or_default()
                                    .color = chosen;
                            }
                            st.doc_color_popover.insert(tool_id.to_string(), false);
                        }
                    }
                });
            }

            // Sub-barra inserir link
            if st.doc_link_popover.get(tool_id).copied().unwrap_or(false) && can_use {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let url_draft = st.doc_link_url_draft.entry(tool_id.to_string()).or_default();
                    let w = ((ui.available_width() - 160.0) * 0.55).max(80.0);
                    ui.add_sized(
                        Vec2::new(w, 22.0),
                        egui::TextEdit::singleline(url_draft).hint_text("https://..."),
                    );
                    let txt_draft = st
                        .doc_link_text_draft
                        .entry(tool_id.to_string())
                        .or_default();
                    ui.add_sized(
                        Vec2::new(w * 0.75, 22.0),
                        egui::TextEdit::singleline(txt_draft).hint_text("Texto do link"),
                    );
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Inserir").size(11.0).color(Color32::WHITE))
                                .fill(EMERALD),
                        )
                        .clicked()
                    {
                        let raw_u = st
                            .doc_link_url_draft
                            .get(tool_id)
                            .cloned()
                            .unwrap_or_default();
                        let u = raw_u.trim().to_string();
                        if u.starts_with("http://") || u.starts_with("https://") {
                            let t = st
                                .doc_link_text_draft
                                .get(tool_id)
                                .cloned()
                                .unwrap_or_default();
                            let disp = if t.trim().is_empty() {
                                u.clone()
                            } else {
                                t.trim().to_string()
                            };
                            if let Some(runs) =
                                doc.blocks.get_mut(active_idx).and_then(|b| b.runs_mut())
                            {
                                if !runs.is_empty() && !runs_plain_text(runs).ends_with(' ') {
                                    runs.push(DocRun::plain(" "));
                                }
                                runs.push(DocRun {
                                    text: disp,
                                    style: InlineStyle {
                                        underline: true,
                                        link: Some(u),
                                        ..Default::default()
                                    },
                                });
                                commit_doc(&doc, st, cmds);
                            }
                            st.doc_link_url_draft.remove(tool_id);
                            st.doc_link_text_draft.remove(tool_id);
                            st.doc_link_popover.insert(tool_id.to_string(), false);
                        }
                    }
                });
            }

            if let Some(ref msg) = notice_msg {
                ui.add_space(2.0);
                ui.label(RichText::new(msg).size(10.5).color(EMERALD_BRIGHT));
            }
        });

    ui.add_space(4.0);
    let avail_h = (ui.available_height() - if can_use { 0.0 } else { 20.0 }).max(50.0);
    let mut doc_changed = false;
    let mut new_active_blk: Option<usize> = None;
    let typing_style_opt = st.doc_pending_style.get(tool_id).cloned();

    // Foco pedido por uma edicao estrutural do quadro anterior (Enter, Backspace, clique abaixo).
    let focus_key = egui::Id::new(("doc_focus_req", tool_id));
    let focus_req: Option<DocFocus> = ui.ctx().data(|d| d.get_temp(focus_key));
    if focus_req.is_some() {
        ui.ctx().data_mut(|d| d.remove::<DocFocus>(focus_key));
    }
    let focus_for = |b: usize, item: usize| -> Option<usize> {
        focus_req.and_then(|(fb, fi, pos)| (fb == b && fi == item).then_some(pos))
    };
    let mut struct_op: Option<DocStructOp> = None;

    // Visual do DocView.tsx no tema escuro: mesa zinc-800 e folha zinc-950, ate 816px.
    const DESK: Color32 = Color32::from_rgb(42, 49, 60); // zinc-800 do tema
    const PAGE: Color32 = Color32::from_rgb(22, 27, 34); // zinc-950 do tema
    const QUOTE_BAR: Color32 = Color32::from_rgb(82, 82, 91);
    const QUOTE_TEXT: Color32 = Color32::from_rgb(158, 160, 172);
    const TABLE_LINE: Color32 = Color32::from_rgb(110, 113, 124);
    let only_empty = doc.blocks.len() == 1
        && doc.blocks[0].runs().is_some_and(|r| r.is_empty())
        && matches!(doc.blocks[0], DocBlock::Paragraph { .. });

    egui::Frame::none()
        .fill(DESK)
        .rounding(Rounding::same(6.0))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            let inner_h = (avail_h - 16.0).max(40.0);
            egui::ScrollArea::vertical()
                .id_salt(("doc_body_scroll", tool_id))
                .max_height(inner_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let page_w = ui.available_width().min(816.0).max(120.0);
                    let wide = page_w > 560.0;
                    let (pad_x, pad_y) = if wide { (44.0, 36.0) } else { (18.0, 18.0) };
                    let side = ((ui.available_width() - page_w) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(side);
                        egui::Frame::none()
                            .fill(PAGE)
                            .rounding(Rounding::same(2.0))
                            .shadow(egui::epaint::Shadow {
                                offset: Vec2::new(0.0, 1.0),
                                blur: 6.0,
                                spread: 0.0,
                                color: Color32::from_black_alpha(90),
                            })
                            .inner_margin(egui::Margin::symmetric(pad_x, pad_y))
                            .show(ui, |ui| {
                                let content_w = (page_w - pad_x * 2.0).max(90.0);
                                ui.set_width(content_w);
                                // A folha ocupa pelo menos a altura visivel (min-h-full do site).
                                let page_min_h = (inner_h - pad_y * 2.0 - 4.0).max(40.0);
                                let page_top = ui.cursor().top();
                                ui.spacing_mut().item_spacing.y = 6.0;
                                let b_count = doc.blocks.len();
                                for bidx in 0..b_count {
                                    let is_active = bidx == active_idx;
                                    let typing = if is_active { typing_style_opt.as_ref() } else { None };
                                    let ordered = matches!(doc.blocks[bidx], DocBlock::OrderedList { .. });
                                    let heading_lvl = match doc.blocks[bidx] {
                                        DocBlock::Heading { level, .. } => Some(level),
                                        _ => None,
                                    };
                                    match &mut doc.blocks[bidx] {
                                        DocBlock::Paragraph { align, runs }
                                        | DocBlock::Heading { align, runs, .. } => {
                                            let (font_sz, col) = match heading_lvl {
                                                Some(1) => (27.0_f32, Color32::WHITE),
                                                Some(2) => (22.0_f32, Color32::WHITE),
                                                Some(_) => (18.0_f32, Color32::WHITE),
                                                None => (14.5_f32, TEXT_PRIMARY),
                                            };
                                            if heading_lvl.is_some() && bidx > 0 {
                                                ui.add_space(6.0);
                                            }
                                            let id = egui::Id::new(("doc_blk", tool_id, bidx));
                                            match doc_take_key(ui, id, can_use, true) {
                                                DocKey::Enter(at) => {
                                                    struct_op = Some(DocStructOp::SplitBlock(bidx, at))
                                                }
                                                DocKey::BackAtStart => {
                                                    struct_op = Some(DocStructOp::MergeBack(bidx))
                                                }
                                                _ => {}
                                            }
                                            let hint = if only_empty && can_use { "Comece a escrever…" } else { "" };
                                            let (out, changed) = doc_runs_editor(
                                                ui, id, runs, font_sz, col, block_align_to_egui(*align),
                                                typing, can_use, true, hint, focus_for(bidx, DOC_NO_ITEM),
                                            );
                                            note_doc_focus(ui, tool_id, bidx, &out, &mut new_active_blk);
                                            doc_changed |= changed;
                                        }
                                        DocBlock::Blockquote { runs } => {
                                            let id = egui::Id::new(("doc_blk", tool_id, bidx));
                                            match doc_take_key(ui, id, can_use, true) {
                                                DocKey::Enter(at) => {
                                                    struct_op = Some(DocStructOp::SplitBlock(bidx, at))
                                                }
                                                DocKey::BackAtStart => {
                                                    struct_op = Some(DocStructOp::MergeBack(bidx))
                                                }
                                                _ => {}
                                            }
                                            let resp = ui.horizontal(|ui| {
                                                ui.add_space(14.0);
                                                doc_runs_editor(
                                                    ui, id, runs, 14.5, QUOTE_TEXT, Align::LEFT, typing,
                                                    can_use, true, "", focus_for(bidx, DOC_NO_ITEM),
                                                )
                                            });
                                            let r = resp.response.rect;
                                            ui.painter().line_segment(
                                                [pos2(r.left() + 2.0, r.top()), pos2(r.left() + 2.0, r.bottom())],
                                                Stroke::new(4.0_f32, QUOTE_BAR),
                                            );
                                            let (out, changed) = resp.inner;
                                            note_doc_focus(ui, tool_id, bidx, &out, &mut new_active_blk);
                                            doc_changed |= changed;
                                        }
                                        DocBlock::BulletList { items } | DocBlock::OrderedList { items } => {
                                            let n_items = items.len();
                                            for (i, item_runs) in items.iter_mut().enumerate() {
                                                let id = egui::Id::new(("doc_li", tool_id, bidx, i));
                                                match doc_take_key(ui, id, can_use, false) {
                                                    DocKey::Enter(at) => {
                                                        struct_op = Some(DocStructOp::SplitItem(bidx, i, at))
                                                    }
                                                    DocKey::BackAtStart => {
                                                        struct_op = Some(DocStructOp::ItemBack(bidx, i))
                                                    }
                                                    DocKey::Up if i > 0 => {
                                                        struct_op = Some(DocStructOp::Focus((bidx, i - 1, 0)))
                                                    }
                                                    DocKey::Down if i + 1 < n_items => {
                                                        struct_op = Some(DocStructOp::Focus((bidx, i + 1, 0)))
                                                    }
                                                    _ => {}
                                                }
                                                let resp = ui.horizontal_top(|ui| {
                                                    ui.add_space(8.0);
                                                    let marker = if ordered {
                                                        format!("{}.", i + 1)
                                                    } else {
                                                        "•".to_string()
                                                    };
                                                    let (mr, _) = ui.allocate_exact_size(
                                                        Vec2::new(20.0, 20.0),
                                                        Sense::hover(),
                                                    );
                                                    ui.painter().text(
                                                        pos2(mr.right() - 3.0, mr.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        marker,
                                                        FontId::proportional(14.5),
                                                        TEXT_PRIMARY,
                                                    );
                                                    doc_runs_editor(
                                                        ui, id, item_runs, 14.5, TEXT_PRIMARY, Align::LEFT,
                                                        typing, can_use, false, "", focus_for(bidx, i),
                                                    )
                                                });
                                                let (out, changed) = resp.inner;
                                                note_doc_focus(ui, tool_id, bidx, &out, &mut new_active_blk);
                                                doc_changed |= changed;
                                            }
                                        }
                                        DocBlock::Table { rows } => {
                                            let ncols = rows.first().map(|r| r.len()).unwrap_or(1).max(1);
                                            let cw = (ui.available_width() / ncols as f32).max(52.0);
                                            let mut table_focused = false;
                                            for (ri, row) in rows.iter_mut().enumerate() {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                                    for (ci, cell) in row.iter_mut().enumerate() {
                                                        let (cell_r, _) = ui.allocate_exact_size(Vec2::new(cw, 28.0), Sense::hover());
                                                        ui.painter().rect_stroke(cell_r, Rounding::ZERO, Stroke::new(1.0_f32, TABLE_LINE));
                                                        let mut cell_ui = ui.new_child(egui::UiBuilder::new().max_rect(cell_r.shrink2(Vec2::new(6.0, 4.0))));
                                                        let r = cell_ui.add(
                                                            egui::TextEdit::singleline(cell)
                                                                .id(egui::Id::new(("doc_td", tool_id, bidx, ri, ci)))
                                                                .frame(false)
                                                                .desired_width(cw - 12.0)
                                                                .text_color(TEXT_PRIMARY)
                                                                .interactive(can_use),
                                                        );
                                                        if r.has_focus() || r.clicked() {
                                                            new_active_blk = Some(bidx);
                                                            table_focused = true;
                                                        }
                                                        if r.changed() && can_use {
                                                            doc_changed = true;
                                                        }
                                                    }
                                                });
                                            }
                                            // So para quem esta na tabela: crescer linhas/colunas (o site faz pelo teclado).
                                            if can_use && (table_focused || is_active) {
                                                ui.horizontal(|ui| {
                                                    if ui.small_button("+ Linha").clicked() {
                                                        rows.push(vec![String::new(); ncols]);
                                                        doc_changed = true;
                                                    }
                                                    if ncols < 8 && ui.small_button("+ Coluna").clicked() {
                                                        for r in rows.iter_mut() {
                                                            r.push(String::new());
                                                        }
                                                        doc_changed = true;
                                                    }
                                                });
                                            }
                                        }
                                        DocBlock::HorizontalRule => {
                                            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
                                            ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, QUOTE_BAR));
                                        }
                                    }
                                }

                                // Clicar no espaco livre da folha continua o texto (como clicar no fim da pagina no site).
                                // (Dentro do ScrollArea a altura disponivel e infinita: mede pelo usado.)
                                let used = ui.cursor().top() - page_top;
                                let rest = (page_min_h - used).max(36.0);
                                let (_, free) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), rest),
                                    if can_use { Sense::click() } else { Sense::hover() },
                                );
                                if can_use && free.on_hover_cursor(CursorIcon::Text).clicked() {
                                    struct_op = Some(DocStructOp::FocusEnd);
                                }
                            });
                    });
                });
        });

    if let Some(nab) = new_active_blk {
        st.doc_active_block.insert(tool_id.to_string(), nab);
    }
    if let Some(op) = struct_op {
        let target = match op {
            DocStructOp::SplitBlock(b, at) => doc.split_text_block(b, at),
            DocStructOp::MergeBack(b) => doc.merge_text_block_back(b),
            DocStructOp::SplitItem(b, i, at) => doc.split_list_item(b, i, at),
            DocStructOp::ItemBack(b, i) => doc.list_item_backspace(b, i),
            DocStructOp::Focus(f) => Some(f),
            DocStructOp::FocusEnd => Some(doc.focus_end_or_append()),
        };
        if let Some(t) = target {
            if !matches!(op, DocStructOp::Focus(_)) {
                doc_changed = true;
            }
            st.doc_active_block.insert(tool_id.to_string(), t.0);
            ui.ctx().data_mut(|d| d.insert_temp(focus_key, t));
            ui.ctx().request_repaint();
        }
    }
    if doc_changed {
        commit_doc(&doc, st, cmds);
    }
    if !can_use {
        ui.label(
            RichText::new("Somente visualização — um administrador decide quem pode usar.")
                .size(11.0)
                .color(TEXT_MUTED),
        );
    }
}


fn render_text_tool_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    kind: &str,
    language: &str,
    can_use: bool,
    _is_mgr: bool,
    cmds: &mut Vec<Value>,
) {
    if kind == "code" {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Linguagem:").size(12.0).color(TEXT_SECONDARY));
            egui::ComboBox::from_id_salt(("tool_code_lang", tool_id))
                .selected_text(language)
                .show_ui(ui, |ui| {
                    for &l in CODE_LANGUAGES {
                        if ui.selectable_label(language == l, l).clicked() && can_use {
                            cmds.push(cmd_code_language(tool_id, l));
                        }
                    }
                });
        });
        ui.add_space(4.0);
    }

    let cur = st
        .text_syncs
        .get(tool_id)
        .map(|s| s.text.clone())
        .unwrap_or_default();
    let mut draft = cur.clone();
    let avail_h = ui.available_height().max(60.0);
    egui::ScrollArea::vertical()
        .id_salt(("text_tool_scroll", tool_id))
        .max_height(avail_h)
        .show(ui, |ui| {
            let mut te = egui::TextEdit::multiline(&mut draft)
                .desired_width(ui.available_width())
                .min_size(Vec2::new(ui.available_width(), avail_h - 6.0));
            if kind == "code" {
                te = te.code_editor();
            }
            let resp = ui.add_enabled(can_use, te);
            if resp.changed() && draft != cur {
                if let Some(op) = st.edit_text_tool(tool_id, &draft) {
                    cmds.push(op);
                }
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn render_tasks_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    items: &[TaskItem],
    rules: &TaskRules,
    self_uid: Option<&str>,
    can_use: bool,
    is_mgr: bool,
    assignable: &[(String, String)],
    cmds: &mut Vec<Value>,
) {
    // Espelho de `TasksView` (components/roomTools/ToolViews.tsx).
    const PRIO_HIGH_BG: Color32 = Color32::from_rgb(66, 24, 24);
    const PRIO_HIGH_FG: Color32 = Color32::from_rgb(248, 113, 113);
    const PRIO_LOW_BG: Color32 = Color32::from_rgb(16, 42, 58);
    const PRIO_LOW_FG: Color32 = Color32::from_rgb(56, 189, 248);
    const ROW_HOVER: Color32 = Color32::from_rgb(30, 30, 34);

    fn prio_label(p: &str) -> &'static str {
        match p {
            "high" => "Alta",
            "low" => "Baixa",
            _ => "Normal",
        }
    }
    fn rule_label(r: &str) -> &'static str {
        match r {
            "everyone" => "Todos com acesso",
            "assignee" => "Só o responsável (e admins)",
            _ => "Só administradores",
        }
    }
    fn small_combo(
        ui: &mut egui::Ui,
        id: impl std::hash::Hash,
        text: &str,
        width: f32,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        egui::ComboBox::from_id_salt(id)
            .selected_text(RichText::new(text).size(11.0))
            .width(width)
            .show_ui(ui, add);
    }

    let can_add = can_use && task_allows(rules, "add", self_uid, is_mgr, None);
    let total = items.len();
    let done_count = items.iter().filter(|i| i.done).count();
    let pct = if total == 0 {
        0
    } else {
        ((done_count as f32 / total as f32) * 100.0).round() as u32
    };
    let today = crate::net::room_tools::today_iso_local();
    let name_of = |uid: &str| -> Option<&str> {
        assignable
            .iter()
            .find(|(id, _)| id == uid)
            .map(|(_, n)| n.as_str())
    };

    ui.spacing_mut().item_spacing.y = 6.0;

    // --- Formulario: texto + [ajustes] + [+] -------------------------------
    if can_add {
        let show_more = st.task_show_more.get(tool_id).copied().unwrap_or(false);
        let mut submit_add = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let input_w = (ui.available_width() - 76.0).max(100.0);
            let resp = ui.add_sized(
                Vec2::new(input_w, 28.0),
                egui::TextEdit::singleline(&mut st.task_input_draft)
                    .char_limit(300)
                    .hint_text("Nova tarefa…"),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit_add = true;
                resp.request_focus();
            }
            if icons::icon_button(
                ui,
                Vec2::new(30.0, 28.0),
                if show_more { BG_ELEVATED } else { Color32::TRANSPARENT },
                Stroke::NONE,
                Rounding::same(6.0),
                "Responsável, prioridade e prazo",
                |p, c, col| icons::paint_gear(p, c, 13.0, col),
                TEXT_SECONDARY,
            )
            .clicked()
            {
                st.task_show_more.insert(tool_id.to_string(), !show_more);
            }
            let can_submit = !st.task_input_draft.trim().is_empty() && total < 200;
            let add_resp = ui.add_enabled(
                can_submit,
                egui::Button::new("")
                    .fill(EMERALD)
                    .rounding(Rounding::same(6.0))
                    .min_size(Vec2::new(34.0, 28.0)),
            );
            icons::paint_plus(
                ui.painter(),
                add_resp.rect.center(),
                13.0,
                if can_submit { Color32::WHITE } else { TEXT_MUTED },
            );
            if add_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                submit_add = true;
            }
        });

        if show_more {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                let cur_a = st
                    .task_assignee_draft
                    .entry(tool_id.to_string())
                    .or_insert(None);
                let a_lbl = cur_a
                    .as_deref()
                    .and_then(name_of)
                    .unwrap_or("Sem responsável")
                    .to_string();
                small_combo(ui, ("task_assign", tool_id), &a_lbl, 120.0, |ui| {
                    ui.selectable_value(cur_a, None, "Sem responsável");
                    for (uid, uname) in assignable {
                        ui.selectable_value(cur_a, Some(uid.clone()), uname);
                    }
                });

                let cur_p = st
                    .task_priority_draft
                    .entry(tool_id.to_string())
                    .or_insert_with(|| "normal".to_string());
                let p_lbl = prio_label(cur_p);
                small_combo(ui, ("task_prio", tool_id), p_lbl, 70.0, |ui| {
                    for p in ["high", "normal", "low"] {
                        ui.selectable_value(cur_p, p.to_string(), prio_label(p));
                    }
                });

                // Prazo: AAAA-MM-DD, como o <input type="date"> do site.
                let due = st.task_due_draft.entry(tool_id.to_string()).or_default();
                let invalid = !due.trim().is_empty()
                    && crate::net::room_tools::parse_iso_date(due).is_none();
                ui.add(
                    egui::TextEdit::singleline(due)
                        .char_limit(10)
                        .desired_width(84.0)
                        .hint_text("Prazo AAAA-MM-DD")
                        .text_color(if invalid { RED } else { TEXT_PRIMARY }),
                )
                .on_hover_text("Prazo");
                if ui.small_button("Hoje").clicked() {
                    *due = today.clone();
                }
                if ui.small_button("Amanhã").clicked() {
                    *due = crate::net::room_tools::add_days_iso(&today, 1).unwrap_or_default();
                }
                if !due.is_empty() && ui.small_button("×").on_hover_text("Sem prazo").clicked() {
                    due.clear();
                }
            });
        }

        if submit_add {
            let txt = st.task_input_draft.trim().to_string();
            if !txt.is_empty() && total < 200 {
                let prio = st
                    .task_priority_draft
                    .get(tool_id)
                    .cloned()
                    .unwrap_or_else(|| "normal".to_string());
                let assignee = st.task_assignee_draft.get(tool_id).cloned().flatten();
                let due = st
                    .task_due_draft
                    .get(tool_id)
                    .map(|s| s.trim().to_string())
                    .filter(|s| crate::net::room_tools::parse_iso_date(s).is_some());
                cmds.push(cmd_task_add_full(
                    tool_id,
                    &txt,
                    assignee.as_deref(),
                    &prio,
                    due.as_deref(),
                ));
                // Como no site: limpa texto, prazo e prioridade; o responsavel fica.
                st.task_input_draft.clear();
                st.task_due_draft.remove(tool_id);
                st.task_priority_draft
                    .insert(tool_id.to_string(), "normal".to_string());
            }
        }
    }

    // --- Progresso + filtro + regras ---------------------------------------
    let rules_open = is_mgr && st.task_show_rules.get(tool_id).copied().unwrap_or(false);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} de {} concluídas · {}%",
                done_count, total, pct
            ))
            .size(11.5)
            .color(TEXT_MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if is_mgr {
                let resp = ui.add(
                    egui::Button::new(RichText::new("Regras").size(11.0).color(TEXT_SECONDARY))
                        .fill(if rules_open { BG_ELEVATED } else { Color32::TRANSPARENT })
                        .rounding(Rounding::same(6.0)),
                );
                if resp.on_hover_text("Regras da lista").clicked() {
                    st.task_show_rules.insert(tool_id.to_string(), !rules_open);
                }
            }
            let filter_v = st.task_filter.entry(tool_id.to_string()).or_insert(0);
            let f_lbl = match *filter_v {
                1 => "Abertas",
                2 => "Concluídas",
                3 => "Minhas",
                _ => "Todas",
            };
            small_combo(ui, ("task_filter", tool_id), f_lbl, 84.0, |ui| {
                ui.selectable_value(filter_v, 0, "Todas");
                ui.selectable_value(filter_v, 1, "Abertas");
                ui.selectable_value(filter_v, 2, "Concluídas");
                ui.selectable_value(filter_v, 3, "Minhas");
            });
        });
    });

    if total > 0 {
        let (bar, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
        ui.painter().rect_filled(bar, Rounding::same(3.0), BG_ELEVATED);
        if pct > 0 {
            let fill = Rect::from_min_size(bar.min, Vec2::new(bar.width() * pct as f32 / 100.0, 6.0));
            ui.painter().rect_filled(fill, Rounding::same(3.0), EMERALD);
        }
    }

    // --- Regras (so administradores) --------------------------------------
    if rules_open {
        egui::Frame::none()
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Regras da lista")
                        .size(11.5)
                        .semibold()
                        .color(TEXT_SECONDARY),
                );
                let mut next_r = rules.clone();
                let mut changed = false;
                let rows: [(&str, &str, &[&str]); 3] = [
                    ("add", "Criar tarefas", &["everyone", "managers"]),
                    ("complete", "Concluir tarefas", &["everyone", "managers", "assignee"]),
                    ("edit", "Editar e apagar", &["everyone", "managers"]),
                ];
                for (key, label, choices) in rows {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).size(11.0).color(TEXT_PRIMARY));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let target = match key {
                                "add" => &mut next_r.add,
                                "complete" => &mut next_r.complete,
                                _ => &mut next_r.edit,
                            };
                            let cur = rule_label(target).to_string();
                            small_combo(ui, ("task_rule", tool_id, key), &cur, 170.0, |ui| {
                                for &c in choices {
                                    changed |= ui
                                        .selectable_value(target, c.to_string(), rule_label(c))
                                        .changed();
                                }
                            });
                        });
                    });
                }
                if changed {
                    cmds.push(cmd_task_set_rules(tool_id, &next_r));
                }
                let clear = ui.add_enabled(
                    done_count > 0,
                    egui::Button::new(
                        RichText::new(format!("Apagar {} concluídas", done_count))
                            .size(11.0)
                            .color(PRIO_HIGH_FG),
                    )
                    .fill(Color32::TRANSPARENT)
                    .rounding(Rounding::same(6.0)),
                );
                if clear.clicked() {
                    cmds.push(cmd_task_clear_done(tool_id));
                }
            });
    }

    if !can_use {
        ui.label(
            RichText::new("Somente visualização — um administrador decide quem pode usar.")
                .size(11.0)
                .color(TEXT_MUTED),
        );
    } else if !can_add && total == 0 {
        ui.label(
            RichText::new("Só os administradores criam tarefas nesta sala.")
                .size(11.0)
                .color(TEXT_MUTED),
        );
    }

    // --- Lista ---------------------------------------------------------------
    let filter_mode = st.task_filter.get(tool_id).copied().unwrap_or(0);
    let mut visible: Vec<&TaskItem> = items
        .iter()
        .filter(|it| match filter_mode {
            1 => !it.done,
            2 => it.done,
            3 => self_uid.is_some_and(|u| it.assignee_id.as_deref() == Some(u)),
            _ => true,
        })
        .collect();
    // Mesma ordem do site: abertas, prioridade, prazo (sem prazo por ultimo), criacao.
    visible.sort_by(|a, b| {
        let po = |p: &str| match p {
            "high" => 0u8,
            "low" => 2,
            _ => 1,
        };
        (a.done, po(&a.priority))
            .cmp(&(b.done, po(&b.priority)))
            .then_with(|| {
                a.due
                    .as_deref()
                    .unwrap_or("9999")
                    .cmp(b.due.as_deref().unwrap_or("9999"))
            })
            .then(a.created_at.cmp(&b.created_at))
    });

    let hover_key = egui::Id::new(("task_hover_row", tool_id));
    let hovered_prev: Option<String> = ui.ctx().data(|d| d.get_temp(hover_key));
    let mut hovered_now: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt(("tasks_list_scroll", tool_id))
        .max_height(ui.available_height().max(80.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for item in visible {
                let can_tick =
                    can_use && task_allows(rules, "complete", self_uid, is_mgr, Some(item));
                let can_edit = can_use && task_allows(rules, "edit", self_uid, is_mgr, Some(item));
                let is_editing = st
                    .task_editing_id
                    .as_ref()
                    .is_some_and(|(t, id)| t == tool_id && id == &item.id);
                let show_actions =
                    can_edit && (is_editing || hovered_prev.as_deref() == Some(item.id.as_str()));
                let late = !item.done && item.due.as_deref().is_some_and(|d| d < today.as_str());

                let bg_idx = ui.painter().add(egui::Shape::Noop);
                let row = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    // Checkbox
                    let (cb_rect, cb_resp) = ui.allocate_exact_size(
                        Vec2::splat(18.0),
                        if can_tick { Sense::click() } else { Sense::hover() },
                    );
                    let box_r = Rect::from_center_size(
                        pos2(cb_rect.center().x, cb_rect.top() + 9.0),
                        Vec2::splat(15.0),
                    );
                    if item.done {
                        ui.painter().rect_filled(box_r, Rounding::same(3.0), EMERALD);
                        icons::paint_checkmark(ui.painter(), box_r.center(), 9.5, Color32::WHITE);
                    } else {
                        ui.painter().rect_stroke(
                            box_r,
                            Rounding::same(3.0),
                            Stroke::new(1.4_f32, if can_tick { TEXT_SECONDARY } else { BORDER_SUBTLE }),
                        );
                    }
                    if can_tick {
                        if cb_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            cmds.push(cmd_task_toggle(tool_id, &item.id));
                        }
                    } else {
                        cb_resp.on_hover_text("Você não pode concluir esta tarefa");
                    }

                    let actions_w = if show_actions { 96.0 } else { 0.0 };
                    let text_w = (ui.available_width() - actions_w).max(80.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(text_w, 0.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            if is_editing {
                                let edit_id = egui::Id::new(("task_edit", tool_id, &item.id));
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut st.task_editing_draft)
                                        .id(edit_id)
                                        .char_limit(300)
                                        .desired_width(text_w),
                                );
                                if !resp.has_focus() && !resp.lost_focus() {
                                    resp.request_focus();
                                }
                                let (enter, esc) = ui.input(|i| {
                                    (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape))
                                });
                                if resp.lost_focus() {
                                    if enter && !esc {
                                        let nt = st.task_editing_draft.trim().to_string();
                                        if !nt.is_empty() && nt != item.text {
                                            cmds.push(cmd_task_edit(tool_id, &item.id, &nt, None));
                                        }
                                    }
                                    st.task_editing_id = None;
                                }
                            } else {
                                let mut rt = RichText::new(&item.text).size(13.0);
                                rt = if item.done {
                                    rt.strikethrough().color(TEXT_MUTED)
                                } else {
                                    rt.color(TEXT_PRIMARY)
                                };
                                let lbl = ui.add(egui::Label::new(rt).wrap().sense(Sense::click()));
                                if can_edit && lbl.double_clicked() {
                                    st.task_editing_id = Some((tool_id.to_string(), item.id.clone()));
                                    st.task_editing_draft = item.text.clone();
                                }
                            }

                            // Linha de detalhes: prioridade, prazo, responsavel, autoria.
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                if item.priority == "high" || item.priority == "low" {
                                    let (bg, fg) = if item.priority == "high" {
                                        (PRIO_HIGH_BG, PRIO_HIGH_FG)
                                    } else {
                                        (PRIO_LOW_BG, PRIO_LOW_FG)
                                    };
                                    egui::Frame::none()
                                        .fill(bg)
                                        .rounding(Rounding::same(3.0))
                                        .inner_margin(egui::Margin::symmetric(4.0, 0.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(prio_label(&item.priority))
                                                    .size(10.5)
                                                    .semibold()
                                                    .color(fg),
                                            );
                                        });
                                }
                                if let Some(ref due) = item.due {
                                    let mut t = RichText::new(format!(
                                        "Prazo {}",
                                        crate::net::room_tools::format_due_short(due)
                                    ))
                                    .size(10.5);
                                    t = if late { t.semibold().color(RED) } else { t.color(TEXT_MUTED) };
                                    ui.label(t);
                                }
                                let assignee_txt = item
                                    .assignee_id
                                    .as_deref()
                                    .and_then(name_of)
                                    .map(str::to_string)
                                    .or_else(|| item.assignee_name.clone());
                                if can_edit {
                                    let lbl = assignee_txt
                                        .as_deref()
                                        .map(|n| format!("→ {n}"))
                                        .unwrap_or_else(|| "Sem responsável".to_string());
                                    let mut next = item.assignee_id.clone();
                                    egui::ComboBox::from_id_salt(("task_row_assign", tool_id, &item.id))
                                        .selected_text(RichText::new(lbl).size(10.5).color(TEXT_MUTED))
                                        .width(110.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut next, None, "Sem responsável");
                                            if let Some(ref cur) = item.assignee_id {
                                                if name_of(cur).is_none() {
                                                    let n = item.assignee_name.clone().unwrap_or_default();
                                                    ui.selectable_value(&mut next, Some(cur.clone()), n);
                                                }
                                            }
                                            for (uid, uname) in assignable {
                                                ui.selectable_value(
                                                    &mut next,
                                                    Some(uid.clone()),
                                                    format!("→ {uname}"),
                                                );
                                            }
                                        });
                                    if next != item.assignee_id {
                                        cmds.push(crate::net::room_tools::cmd_task_set_assignee(
                                            tool_id,
                                            &item.id,
                                            next.as_deref(),
                                        ));
                                    }
                                } else if let Some(ref n) = assignee_txt {
                                    ui.label(RichText::new(format!("→ {n}")).size(10.5).color(TEXT_MUTED));
                                }
                                let by = match (item.done, item.done_by_name.as_deref()) {
                                    (true, Some(n)) => format!("· concluída por {n}"),
                                    _ => format!("· adicionada por {}", item.by_name),
                                };
                                ui.label(RichText::new(by).size(10.5).color(TEXT_MUTED));
                            });
                        },
                    );

                    if show_actions {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            if icons::icon_button(
                                ui,
                                Vec2::splat(22.0),
                                Color32::TRANSPARENT,
                                Stroke::NONE,
                                Rounding::same(5.0),
                                "Remover",
                                |p, c, col| icons::paint_close_x(p, c, 10.0, col),
                                TEXT_MUTED,
                            )
                            .clicked()
                            {
                                cmds.push(cmd_task_remove(tool_id, &item.id));
                            }
                            let mut prio = item.priority.clone();
                            egui::ComboBox::from_id_salt(("task_row_prio", tool_id, &item.id))
                                .selected_text(RichText::new(prio_label(&prio)).size(10.5).color(TEXT_MUTED))
                                .width(60.0)
                                .show_ui(ui, |ui| {
                                    for p in ["high", "normal", "low"] {
                                        ui.selectable_value(&mut prio, p.to_string(), prio_label(p));
                                    }
                                });
                            if prio != item.priority {
                                cmds.push(crate::net::room_tools::cmd_task_set_priority(
                                    tool_id, &item.id, &prio,
                                ));
                            }
                        });
                    }
                });

                let row_rect = row.response.rect.expand2(Vec2::new(4.0, 2.0));
                if ui.rect_contains_pointer(row_rect) {
                    hovered_now = Some(item.id.clone());
                    ui.painter().set(
                        bg_idx,
                        egui::Shape::rect_filled(row_rect, Rounding::same(6.0), ROW_HOVER),
                    );
                }
            }
        });

    if hovered_now != hovered_prev {
        ui.ctx().data_mut(|d| match hovered_now {
            Some(id) => d.insert_temp(hover_key, id),
            None => d.remove::<String>(hover_key),
        });
        ui.ctx().request_repaint();
    }
}


fn render_wheel_side_panel_contents(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    entries: &[WheelEntry],
    history: &[WheelResult],
    remove_winner: bool,
    announce: bool,
    duration: u32,
    peer_names: &[String],
    is_mgr: bool,
    can_use: bool,
    avail_h: f32,
    cmds: &mut Vec<Value>,
) {
    ui.horizontal(|ui| {
        let tabs = if is_mgr {
            vec![
                (0u8, format!("Opcoes ({})", entries.len())),
                (1, format!("Historico ({})", history.len())),
                (2, "Ajustes".to_string()),
            ]
        } else {
            vec![
                (0u8, format!("Opcoes ({})", entries.len())),
                (1, format!("Historico ({})", history.len())),
            ]
        };
        for (tid, tlbl) in tabs {
            let active = st.wheel_tab == tid;
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(tlbl)
                            .size(11.0)
                            .semibold()
                            .color(if active { Color32::WHITE } else { TEXT_SECONDARY }),
                    )
                    .fill(if active { BG_ELEVATED } else { Color32::TRANSPARENT })
                    .rounding(Rounding::same(6.0)),
                )
                .clicked()
            {
                st.wheel_tab = tid;
            }
        }
    });
    ui.separator();

    match st.wheel_tab {
        1 => {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Historico de sorteios")
                        .size(12.0)
                        .semibold()
                        .color(TEXT_PRIMARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if can_use && !history.is_empty() && ui.small_button("Limpar").clicked() {
                        cmds.push(json!({
                            "type": "tool-wheel",
                            "id": tool_id,
                            "action": "clear-history"
                        }));
                    }
                });
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt(("wheel_hist_scroll", tool_id))
                .max_height((avail_h - 82.0).max(60.0))
                .show(ui, |ui| {
                    if history.is_empty() {
                        ui.label(
                            RichText::new("Nenhum sorteio realizado ainda.")
                                .size(11.0)
                                .color(TEXT_MUTED),
                        );
                    } else {
                        for (i, h) in history.iter().enumerate() {
                            egui::Frame::none()
                                .fill(BG_TILE)
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("#{}", i + 1))
                                                .size(10.5)
                                                .color(TEXT_MUTED),
                                        );
                                        let (dr, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                        ui.painter().circle_filled(
                                            dr.center(),
                                            4.0,
                                            parse_hex_color32(if h.color.is_empty() {
                                                WHEEL_COLORS[i % WHEEL_COLORS.len()]
                                            } else {
                                                &h.color
                                            }),
                                        );
                                        ui.label(
                                            RichText::new(&h.text)
                                                .size(11.5)
                                                .semibold()
                                                .color(TEXT_PRIMARY),
                                        );
                                        if !h.by_name.is_empty() {
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.label(
                                                    RichText::new(format!("por {}", h.by_name))
                                                        .size(10.0)
                                                        .color(TEXT_MUTED),
                                                );
                                            });
                                        }
                                    });
                                });
                            ui.add_space(3.0);
                        }
                    }
                });
        }
        2 if is_mgr => {
            let mut rem = remove_winner;
            if ui
                .checkbox(&mut rem, "Remover vencedor apos o sorteio")
                .changed()
            {
                cmds.push(json!({
                    "type": "tool-wheel",
                    "id": tool_id,
                    "action": "settings",
                    "settings": { "removeWinner": rem }
                }));
            }
            let mut ann = announce;
            if ui.checkbox(&mut ann, "Anunciar vencedor no chat").changed() {
                cmds.push(json!({
                    "type": "tool-wheel",
                    "id": tool_id,
                    "action": "settings",
                    "settings": { "announce": ann }
                }));
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Duracao do giro").size(11.5).color(TEXT_SECONDARY));
            ui.horizontal_wrapped(|ui| {
                for d in [3u32, 5, 8, 12, 20] {
                    let active = duration == d;
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!("{}s", d)).size(11.0).color(Color32::WHITE),
                            )
                            .fill(if active { EMERALD } else { BG_ELEVATED })
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        cmds.push(json!({
                            "type": "tool-wheel",
                            "id": tool_id,
                            "action": "settings",
                            "settings": { "duration": d }
                        }));
                    }
                }
            });
        }
        _ => {
            if can_use {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 32.0).max(70.0);
                    let resp = ui.add_sized(
                        Vec2::new(w, 24.0),
                        egui::TextEdit::singleline(&mut st.wheel_entry_draft)
                            .hint_text("Nova opcao (ou cole lista)"),
                    );
                    let enter =
                        resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui
                        .add(
                            egui::Button::new(
                                RichText::new("+").semibold().color(Color32::WHITE),
                            )
                            .fill(EMERALD)
                            .min_size(Vec2::splat(24.0)),
                        )
                        .clicked()
                        || enter)
                        && !st.wheel_entry_draft.trim().is_empty()
                    {
                        let list: Vec<Value> = st
                            .wheel_entry_draft
                            .split(['\n', ';'])
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(|s| json!({ "text": s }))
                            .collect();
                        st.wheel_entry_draft.clear();
                        if !list.is_empty() {
                            cmds.push(json!({
                                "type": "tool-wheel",
                                "id": tool_id,
                                "action": "add",
                                "entries": list,
                                "replace": false
                            }));
                        }
                    }
                });
                ui.add_space(4.0);

                // Presets iguais a Imagem 1
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                    let add_preset = |items: Vec<String>, cmds: &mut Vec<Value>| {
                        let arr: Vec<Value> =
                            items.into_iter().map(|t| json!({ "text": t })).collect();
                        cmds.push(json!({
                            "type": "tool-wheel",
                            "id": tool_id,
                            "action": "add",
                            "entries": arr,
                            "replace": true
                        }));
                    };
                    if render_preset_pill(ui, "Pessoas", false, |p, c, col| {
                        icons::paint_groups(p, c, 11.0, col);
                    }) {
                        add_preset(peer_names.to_vec(), cmds);
                    }
                    if render_preset_pill(ui, "Sim / Nao", false, |p, c, col| {
                        icons::paint_checkmark(p, c, 10.5, col);
                    }) {
                        add_preset(vec!["Sim".into(), "Nao".into()], cmds);
                    }
                    if render_preset_pill(ui, "Cara/Coroa", false, |p, c, col| {
                        icons::paint_coin(p, c, 11.0, col);
                    }) {
                        add_preset(vec!["Cara".into(), "Coroa".into()], cmds);
                    }
                    if render_preset_pill(ui, "Dado", false, |p, c, col| {
                        icons::paint_dice(p, c, 11.0, col);
                    }) {
                        add_preset((1..=6).map(|n| n.to_string()).collect(), cmds);
                    }
                    let num_open = st.wheel_opt_num_open.get(tool_id).copied().unwrap_or(false);
                    if render_preset_pill(ui, "Numeros", num_open, |p, c, col| {
                        p.text(c, egui::Align2::CENTER_CENTER, "12", FontId::proportional(9.0), col);
                    }) {
                        st.wheel_opt_num_open
                            .insert(tool_id.to_string(), !num_open);
                    }
                    if render_preset_pill(ui, "Letras", false, |p, c, col| {
                        p.text(c, egui::Align2::CENTER_CENTER, "Ab", FontId::proportional(9.0), col);
                    }) {
                        add_preset(('A'..='Z').map(|c| c.to_string()).collect(), cmds);
                    }
                });

                if st.wheel_opt_num_open.get(tool_id).copied().unwrap_or(false) {
                    ui.add_space(3.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("De").size(10.5).color(TEXT_MUTED));
                        ui.add_sized(
                            Vec2::new(36.0, 20.0),
                            egui::TextEdit::singleline(&mut st.wheel_num_from),
                        );
                        ui.label(RichText::new("Ate").size(10.5).color(TEXT_MUTED));
                        ui.add_sized(
                            Vec2::new(36.0, 20.0),
                            egui::TextEdit::singleline(&mut st.wheel_num_to),
                        );
                        if ui
                            .add(
                                egui::Button::new(RichText::new("OK").size(10.5).color(Color32::WHITE))
                                    .fill(EMERALD),
                            )
                            .clicked()
                        {
                            let a = st.wheel_num_from.trim().parse::<i32>().unwrap_or(1);
                            let b = st.wheel_num_to.trim().parse::<i32>().unwrap_or(10);
                            let (lo, hi) = (a.min(b), a.max(b));
                            let arr: Vec<Value> = (lo..=lo + (hi - lo).min(59))
                                .map(|n| json!({ "text": n.to_string() }))
                                .collect();
                            cmds.push(json!({
                                "type": "tool-wheel",
                                "id": tool_id,
                                "action": "add",
                                "entries": arr,
                                "replace": true
                            }));
                            st.wheel_opt_num_open.insert(tool_id.to_string(), false);
                        }
                    });
                }

                ui.add_space(3.0);
                // Botoes de acao com icones vetoriais exatos da Imagem 2
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Embaralhar")
                                    .size(11.0)
                                    .color(TEXT_PRIMARY),
                            )
                            .fill(BG_ELEVATED)
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        cmds.push(json!({
                            "type": "tool-wheel",
                            "id": tool_id,
                            "action": "shuffle"
                        }));
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("A–Z").size(11.0).color(TEXT_PRIMARY),
                            )
                            .fill(BG_ELEVATED)
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        cmds.push(json!({
                            "type": "tool-wheel",
                            "id": tool_id,
                            "action": "sort"
                        }));
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(if st.wheel_bulk_mode {
                                    "Voltar lista"
                                } else {
                                    "Editar como texto"
                                })
                                .size(11.0)
                                .color(TEXT_PRIMARY),
                            )
                            .fill(if st.wheel_bulk_mode { BLUE } else { BG_ELEVATED })
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        st.wheel_bulk_mode = !st.wheel_bulk_mode;
                        if st.wheel_bulk_mode {
                            st.wheel_bulk_draft = entries
                                .iter()
                                .map(|e| e.text.clone())
                                .collect::<Vec<_>>()
                                .join("\n");
                        }
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Limpar")
                                    .size(11.0)
                                    .color(Color32::from_rgb(248, 113, 113)),
                            )
                            .fill(BG_ELEVATED)
                            .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        cmds.push(json!({
                            "type": "tool-wheel",
                            "id": tool_id,
                            "action": "clear"
                        }));
                    }
                });
            }

            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("Opcoes: {}", entries.len()))
                    .size(11.0)
                    .color(TEXT_MUTED),
            );
            let list_h = (avail_h - 175.0).max(52.0);
            if st.wheel_bulk_mode && can_use {
                ui.add_sized(
                    Vec2::new(ui.available_width(), (list_h - 26.0).max(40.0)),
                    egui::TextEdit::multiline(&mut st.wheel_bulk_draft),
                );
                if ui.small_button("Aplicar lista").clicked() {
                    let arr: Vec<Value> = st
                        .wheel_bulk_draft
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(|l| json!({ "text": l }))
                        .collect();
                    cmds.push(json!({
                        "type": "tool-wheel",
                        "id": tool_id,
                        "action": "add",
                        "entries": arr,
                        "replace": true
                    }));
                    st.wheel_bulk_mode = false;
                }
            } else {
                let total_w_val: f32 = entries
                    .iter()
                    .map(|e| e.weight.max(1.0) as f32)
                    .sum::<f32>()
                    .max(1.0);
                egui::ScrollArea::vertical()
                    .id_salt(("wheel_opts_scroll", tool_id))
                    .max_height(list_h)
                    .show(ui, |ui| {
                        for (idx, ent) in entries.iter().enumerate() {
                            let hex = if ent.color.trim().is_empty() {
                                WHEEL_COLORS[idx % WHEEL_COLORS.len()]
                            } else {
                                &ent.color
                            };
                            let cur_w = (ent.weight.round() as u32).clamp(1, 10);
                            let pct =
                                ((ent.weight.max(1.0) as f32 / total_w_val) * 100.0).round() as u32;
                            ui.horizontal(|ui| {
                                let (dr, _) =
                                    ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                ui.painter().circle_filled(
                                    dr.center(),
                                    5.5,
                                    parse_hex_color32(hex),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&ent.text)
                                            .size(12.0)
                                            .color(TEXT_PRIMARY),
                                    )
                                    .truncate(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    if can_use && ui.small_button("×").clicked() {
                                        cmds.push(json!({
                                            "type": "tool-wheel",
                                            "id": tool_id,
                                            "action": "remove",
                                            "entryId": ent.id
                                        }));
                                    }
                                    if can_use {
                                        let mut sel_w = cur_w;
                                        egui::ComboBox::from_id_salt(("wheel_w", tool_id, &ent.id))
                                            .selected_text(format!("x{}", sel_w))
                                            .width(46.0)
                                            .show_ui(ui, |ui| {
                                                for w_opt in 1u32..=10 {
                                                    if ui
                                                        .selectable_value(
                                                            &mut sel_w,
                                                            w_opt,
                                                            format!("x{}", w_opt),
                                                        )
                                                        .changed()
                                                    {
                                                        cmds.push(json!({
                                                            "type": "tool-wheel",
                                                            "id": tool_id,
                                                            "action": "edit",
                                                            "entryId": ent.id,
                                                            "weight": w_opt
                                                        }));
                                                    }
                                                }
                                            });
                                    } else {
                                        ui.label(
                                            RichText::new(format!("x{}", cur_w))
                                                .size(10.5)
                                                .color(TEXT_MUTED),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(format!("{}%", pct))
                                            .size(10.5)
                                            .color(TEXT_MUTED),
                                    );
                                });
                            });
                        }
                    });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_wheel_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    entries: &[WheelEntry],
    spin: Option<&WheelSpinState>,
    history: &[WheelResult],
    remove_winner: bool,
    announce: bool,
    duration: u32,
    peer_names: &[String],
    is_mgr: bool,
    can_use: bool,
    cmds: &mut Vec<Value>,
) {
    let now = Instant::now();
    let (angle_deg, spinning) = spin.map(|sp| wheel_angle_at(sp, now)).unwrap_or((0.0, false));

    let total_w = ui.available_width();
    let avail_h = ui.available_height().max(110.0);
    let show_side = total_w >= 460.0;
    let drawer_open = !show_side && st.wheel_drawer_open.get(tool_id).copied().unwrap_or(false);
    let side_w = if show_side {
        (total_w * 0.42).clamp(215.0, 310.0)
    } else {
        0.0
    };
    let stage_w = (total_w - side_w - if show_side { 10.0 } else { 0.0 }).max(140.0);

    ui.horizontal_top(|ui| {
        // Esquerda: Roleta + Botao Girar + Card Flutuante do Vencedor
        ui.vertical(|ui| {
            ui.set_min_size(Vec2::new(stage_w, avail_h));
            ui.set_max_size(Vec2::new(stage_w, avail_h));

            if !show_side {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let lbl = if drawer_open {
                            "Fechar painel".to_string()
                        } else {
                            format!("Opcoes / Historico ({})", history.len())
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(lbl).size(10.5).semibold().color(Color32::WHITE),
                                )
                                .fill(if drawer_open { BLUE } else { BG_ELEVATED })
                                .rounding(Rounding::same(10.0)),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            st.wheel_drawer_open
                                .insert(tool_id.to_string(), !drawer_open);
                        }
                    });
                });
            }

            if drawer_open {
                egui::Frame::none()
                    .fill(Color32::from_rgb(22, 27, 34))
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.set_min_size(Vec2::new(stage_w - 18.0, avail_h - 34.0));
                        ui.set_max_size(Vec2::new(stage_w - 18.0, avail_h - 34.0));
                        render_wheel_side_panel_contents(
                            st,
                            ui,
                            tool_id,
                            entries,
                            history,
                            remove_winner,
                            announce,
                            duration,
                            peer_names,
                            is_mgr,
                            can_use,
                            avail_h - 24.0,
                            cmds,
                        );
                    });
                return;
            }

            let wheel_box_h = (avail_h - if show_side { 54.0 } else { 64.0 }).max(48.0);
            let wheel_d = wheel_box_h.min(stage_w - 12.0).clamp(44.0, 290.0);
            let mut wheel_center_rect = Rect::NOTHING;

            ui.vertical_centered(|ui| {
                let (w_rect, _) = ui.allocate_exact_size(Vec2::splat(wheel_d), Sense::hover());
                wheel_center_rect = w_rect;
                let c = w_rect.center();
                let rad = wheel_d * 0.45;
                let p = ui.painter();

                p.circle_stroke(
                    c,
                    rad + 3.0,
                    Stroke::new(2.5_f32, Color32::from_rgb(42, 49, 60)),
                );

                if entries.is_empty() {
                    p.circle_filled(c, rad, BG_ELEVATED);
                    p.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        "Adicione opcoes",
                        FontId::proportional(11.0),
                        TEXT_MUTED,
                    );
                } else {
                    let total_weight: f32 =
                        entries.iter().map(|e| e.weight.max(1.0) as f32).sum();
                    let mut start_deg = -90.0_f32 + angle_deg;
                    for (idx, ent) in entries.iter().enumerate() {
                        let span_deg =
                            360.0 * (ent.weight.max(1.0) as f32 / total_weight.max(1.0));
                        let hex = if ent.color.trim().is_empty() {
                            WHEEL_COLORS[idx % WHEEL_COLORS.len()]
                        } else {
                            &ent.color
                        };
                        let slice_col = parse_hex_color32(hex);
                        let segs = ((span_deg / 6.0).ceil() as usize).clamp(4, 36);
                        let mut poly = Vec::with_capacity(segs + 2);
                        poly.push(c);
                        for step in 0..=segs {
                            let a = (start_deg + span_deg * (step as f32 / segs as f32)).to_radians();
                            poly.push(pos2(c.x + a.cos() * rad, c.y + a.sin() * rad));
                        }
                        p.add(egui::Shape::convex_polygon(
                            poly,
                            slice_col,
                            Stroke::new(1.2_f32, Color32::from_rgb(22, 27, 34)),
                        ));

                        let mid_a = (start_deg + span_deg * 0.5).to_radians();
                        let label_pos = pos2(
                            c.x + mid_a.cos() * (rad * 0.62),
                            c.y + mid_a.sin() * (rad * 0.62),
                        );
                        let short: String = ent.text.chars().take(12).collect();
                        p.text(
                            label_pos,
                            egui::Align2::CENTER_CENTER,
                            short,
                            FontId::proportional(if wheel_d < 120.0 || entries.len() > 10 {
                                9.0
                            } else {
                                11.5
                            }),
                            Color32::WHITE,
                        );
                        start_deg += span_deg;
                    }
                    p.circle_filled(c, rad * 0.16, Color32::from_rgb(22, 27, 34));
                    p.circle_stroke(
                        c,
                        rad * 0.16,
                        Stroke::new(2.0_f32, Color32::from_rgb(55, 65, 81)),
                    );
                }

                // Ponteiro esmeralda no topo
                p.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(c.x - 7.0, c.y - rad - 9.0),
                        pos2(c.x + 7.0, c.y - rad - 9.0),
                        pos2(c.x, c.y - rad + 4.0),
                    ],
                    EMERALD_BRIGHT,
                    Stroke::new(1.0_f32, Color32::WHITE),
                ));

                ui.add_space(4.0);
                if ui
                    .add_enabled(
                        can_use && !spinning && !entries.is_empty(),
                        egui::Button::new(
                            RichText::new(if spinning { "Girando..." } else { "Girar" })
                                .size(12.5)
                                .semibold()
                                .color(Color32::WHITE),
                        )
                        .fill(EMERALD)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(118.0, 26.0)),
                    )
                    .clicked()
                {
                    cmds.push(json!({ "type": "tool-wheel", "id": tool_id, "action": "spin" }));
                }

                if !spinning {
                    if let Some(sp) = spin {
                        if !sp.winner_text.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "Ultimo resultado: {} ({})",
                                    sp.winner_text, sp.by_name
                                ))
                                .size(11.0)
                                .semibold()
                                .color(EMERALD_BRIGHT),
                            );
                        }
                    } else if let Some(last) = history.last() {
                        ui.label(
                            RichText::new(format!("Ultimo: {}", last.text))
                                .size(11.0)
                                .semibold()
                                .color(EMERALD_BRIGHT),
                        );
                    }
                }
            });

            // Card flutuante destacado de RESULTADO sobre a roleta ao terminar o giro (igual WheelView.tsx:283-293)
            if !spinning && wheel_center_rect != Rect::NOTHING {
                if let Some(sp) = spin {
                    let dismissed = st
                        .wheel_winner_dismissed_spin_id
                        .get(tool_id)
                        .is_some_and(|id| id == &sp.id);
                    if !sp.winner_text.is_empty() && !dismissed {
                        let card_w = (stage_w * 0.84).clamp(150.0, 270.0);
                        let card_h = 68.0_f32;
                        let card_rect = Rect::from_center_size(
                            wheel_center_rect.center(),
                            Vec2::new(card_w, card_h),
                        );
                        let mut c_ui = ui.new_child(egui::UiBuilder::new().max_rect(card_rect));
                        let frame_resp = egui::Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(8, 38, 29, 242))
                            .stroke(Stroke::new(1.8_f32, EMERALD_BRIGHT))
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(&mut c_ui, |ui| {
                                ui.set_min_size(Vec2::new(card_w - 20.0, card_h - 14.0));
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("RESULTADO")
                                            .size(9.5)
                                            .semibold()
                                            .color(EMERALD_BRIGHT),
                                    );
                                    ui.label(
                                        RichText::new(&sp.winner_text)
                                            .size(18.0)
                                            .semibold()
                                            .color(Color32::WHITE),
                                    );
                                    ui.label(
                                        RichText::new(format!("Sorteado por {} • clique para fechar", sp.by_name))
                                            .size(9.5)
                                            .color(TEXT_SECONDARY),
                                    );
                                });
                            });
                        if frame_resp
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            st.wheel_winner_dismissed_spin_id
                                .insert(tool_id.to_string(), sp.id.clone());
                        }
                    }
                }
            }
        });

        // Direita: Opcoes | Historico | Ajustes quando largo
        if show_side {
            ui.add_space(8.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(22, 27, 34))
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_min_size(Vec2::new(side_w - 18.0, avail_h - 4.0));
                    ui.set_max_size(Vec2::new(side_w - 18.0, avail_h - 4.0));
                    render_wheel_side_panel_contents(
                        st,
                        ui,
                        tool_id,
                        entries,
                        history,
                        remove_winner,
                        announce,
                        duration,
                        peer_names,
                        is_mgr,
                        can_use,
                        avail_h,
                        cmds,
                    );
                });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_calc_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    expr: &str,
    history: &[CalcEntry],
    memory: f64,
    angle: &str,
    can_use: bool,
    cmds: &mut Vec<Value>,
) {
    let total_w = ui.available_width();
    let total_h = ui.available_height().max(110.0);
    let show_tape = total_w >= 360.0;
    let drawer_open =
        !show_tape && st.calc_history_drawer.get(tool_id).copied().unwrap_or(false);
    let tape_w = if show_tape {
        (total_w * 0.30).clamp(135.0, 220.0)
    } else {
        0.0
    };
    let left_w = (total_w - tape_w - if show_tape { 8.0 } else { 0.0 }).max(150.0);

    let trigger_key = |k: &str, st: &mut crate::net::room_tools::RoomToolsState, cmds: &mut Vec<Value>| {
        if !can_use {
            return;
        }
        if k == "=" {
            st.calc_fresh = true;
            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "equals", "expr": expr }));
        } else if k == "MC" {
            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "memory", "op": "clear" }));
        } else if k == "M+" {
            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "memory", "op": "add" }));
        } else if k == "M-" {
            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "memory", "op": "sub" }));
        } else {
            let next = press_calc_key(expr, k, st.calc_fresh, memory);
            st.calc_fresh = false;
            let seq = st.calc_seq.get(tool_id).copied().unwrap_or(0) + 1;
            st.calc_seq.insert(tool_id.to_string(), seq);
            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "set", "expr": next, "seq": seq }));
        }
    };

    let render_tape_box = |ui: &mut egui::Ui,
                           st: &mut crate::net::room_tools::RoomToolsState,
                           cmds: &mut Vec<Value>,
                           box_w: f32,
                           box_h: f32| {
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 27, 34))
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_min_size(Vec2::new((box_w - 18.0).max(90.0), (box_h - 4.0).max(60.0)));
                ui.set_max_size(Vec2::new((box_w - 18.0).max(90.0), (box_h - 4.0).max(60.0)));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Historico")
                            .size(11.5)
                            .semibold()
                            .color(TEXT_PRIMARY),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !show_tape && ui.small_button("Voltar").clicked() {
                            st.calc_history_drawer.insert(tool_id.to_string(), false);
                        }
                        if can_use && !history.is_empty() && ui.small_button("Limpar").clicked() {
                            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "clear-history" }));
                        }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt(("calc_hist_scroll", tool_id))
                    .max_height((box_h - 46.0).max(40.0))
                    .show(ui, |ui| {
                        if history.is_empty() {
                            ui.label(
                                RichText::new("Contas com = aparecem aqui.")
                                    .size(11.0)
                                    .color(TEXT_MUTED),
                            );
                        } else {
                            for entry in history.iter().rev() {
                                let btn = egui::Button::new(
                                    RichText::new(format!("{} = {}", entry.expr, entry.result))
                                        .size(11.0)
                                        .color(TEXT_PRIMARY),
                                )
                                .fill(BG_TILE)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(ui.available_width(), 22.0));
                                if ui.add(btn).clicked() && can_use {
                                    let seq = st.calc_seq.get(tool_id).copied().unwrap_or(0) + 1;
                                    st.calc_seq.insert(tool_id.to_string(), seq);
                                    cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "set", "expr": entry.result, "seq": seq }));
                                    if !show_tape {
                                        st.calc_history_drawer.insert(tool_id.to_string(), false);
                                    }
                                }
                            }
                        }
                    });
            });
    };

    if drawer_open {
        render_tape_box(ui, st, cmds, total_w, total_h);
        return;
    }

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_size(Vec2::new(left_w, total_h));
            ui.set_max_size(Vec2::new(left_w, total_h));

            // Visor
            egui::Frame::none()
                .fill(Color32::from_rgb(12, 15, 22))
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                .show(ui, |ui| {
                    ui.set_width((left_w - 18.0).max(100.0));
                    ui.horizontal(|ui| {
                        let ang_lbl = if angle == "rad" { "RAD" } else { "DEG" };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(ang_lbl)
                                        .size(10.0)
                                        .semibold()
                                        .color(TEXT_SECONDARY),
                                )
                                .fill(BG_ELEVATED)
                                .rounding(Rounding::same(5.0))
                                .min_size(Vec2::new(34.0, 18.0)),
                            )
                            .clicked()
                            && can_use
                        {
                            let next_ang = if angle == "deg" { "rad" } else { "deg" };
                            cmds.push(json!({ "type": "tool-calc", "id": tool_id, "action": "angle", "angle": next_ang }));
                        }
                        if !show_tape {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!("Hist ({})", history.len()))
                                            .size(10.0)
                                            .color(EMERALD_BRIGHT),
                                    )
                                    .fill(BG_ELEVATED)
                                    .rounding(Rounding::same(5.0))
                                    .min_size(Vec2::new(56.0, 18.0)),
                                )
                                .clicked()
                            {
                                st.calc_history_drawer.insert(tool_id.to_string(), true);
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Copiar").clicked() && !expr.is_empty() {
                                ui.ctx().copy_text(expr.to_string());
                            }
                        });
                    });
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        let display_txt = if expr.is_empty() { "0" } else { expr };
                        ui.label(
                            RichText::new(display_txt)
                                .size(if left_w > 320.0 { 24.0 } else { 18.0 })
                                .semibold()
                                .color(Color32::WHITE),
                        );
                        let prev_txt = eval_calc_preview(expr, angle)
                            .map(|v| format!("= {}", v))
                            .unwrap_or_default();
                        ui.label(RichText::new(prev_txt).size(11.0).color(TEXT_MUTED));
                    });
                });

            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Cientifica").size(10.5).color(if st.calc_scientific {
                                EMERALD_BRIGHT
                            } else {
                                TEXT_SECONDARY
                            }),
                        )
                        .fill(Color32::TRANSPARENT),
                    )
                    .clicked()
                {
                    st.calc_scientific = !st.calc_scientific;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for mk in ["M-", "M+", "MR", "MC"] {
                        let enabled =
                            can_use && ((mk != "MR" && mk != "MC") || memory.abs() > 1e-12);
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(
                                    RichText::new(mk)
                                        .size(10.0)
                                        .color(if enabled { TEXT_PRIMARY } else { TEXT_MUTED }),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            trigger_key(mk, st, cmds);
                        }
                    }
                });
            });

            if st.calc_scientific {
                let sci_keys = ["sin", "cos", "tan", "√", "^2", "(", ")", "π", "ln", "^"];
                let kw = ((left_w - 24.0) / 5.0).max(24.0);
                for chunk in sci_keys.chunks(5) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                        for &sk in chunk {
                            if ui
                                .add_enabled(
                                    can_use,
                                    egui::Button::new(
                                        RichText::new(sk).size(10.5).color(TEXT_SECONDARY),
                                    )
                                    .fill(BG_TILE)
                                    .rounding(Rounding::same(5.0))
                                    .min_size(Vec2::new(kw, 20.0)),
                                )
                                .clicked()
                            {
                                trigger_key(sk, st, cmds);
                            }
                        }
                    });
                }
            }

            let rows = [
                ["C", "⌫", "%", "÷"],
                ["7", "8", "9", "×"],
                ["4", "5", "6", "−"],
                ["1", "2", "3", "+"],
                ["±", "0", ",", "="],
            ];
            let gap = 4.0;
            let btn_w = ((left_w - gap * 3.0 - 2.0) / 4.0).max(26.0);
            let rem_h = ui.available_height().max(70.0);
            let btn_h = ((rem_h - gap * 4.0 - 2.0) / 5.0).clamp(15.0, 56.0);

            ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
            for r in rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for k in r {
                        let is_eq = k == "=";
                        let is_op = matches!(k, "÷" | "×" | "−" | "+");
                        let fill = if is_eq {
                            Color32::from_rgb(5, 150, 105)
                        } else if is_op {
                            Color32::from_rgb(16, 50, 43)
                        } else {
                            Color32::from_rgb(42, 49, 60)
                        };
                        let txt_col = if is_op {
                            Color32::from_rgb(52, 211, 153)
                        } else {
                            Color32::WHITE
                        };
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new(k)
                                        .size(if btn_h < 24.0 { 12.0 } else { 14.0 })
                                        .semibold()
                                        .color(txt_col),
                                )
                                .fill(fill)
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(btn_w, btn_h)),
                            )
                            .clicked()
                        {
                            trigger_key(k, st, cmds);
                        }
                    }
                });
            }
        });

        if show_tape {
            ui.add_space(6.0);
            render_tape_box(ui, st, cmds, tape_w, total_h);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_sheet_view(
    st: &mut crate::net::room_tools::RoomToolsState,
    ui: &mut egui::Ui,
    tool_id: &str,
    title: &str,
    rows: usize,
    cols: usize,
    cells: &std::collections::HashMap<String, SheetCell>,
    col_widths: &std::collections::HashMap<usize, u32>,
    can_use: bool,
    cmds: &mut Vec<Value>,
) {
    let sel_r = st.sheet_sel_row.min(rows.saturating_sub(1));
    let sel_c = st.sheet_sel_col.min(cols.saturating_sub(1));
    let anch_r = st.sheet_anchor_row.min(rows.saturating_sub(1));
    let anch_c = st.sheet_anchor_col.min(cols.saturating_sub(1));
    let top = sel_r.min(anch_r);
    let bottom = sel_r.max(anch_r);
    let left = sel_c.min(anch_c);
    let right = sel_c.max(anch_c);

    let sel_key = sheet_cell_key(sel_r, sel_c);
    let sel_cell = cells.get(&sel_key).cloned().unwrap_or_default();
    let inline_editing = st.sheet_editing_inline.get(tool_id).copied().unwrap_or(false);
    let selection_label = if top == bottom && left == right {
        sheet_cell_name(top, left)
    } else {
        format!("{}:{}", sheet_cell_name(top, left), sheet_cell_name(bottom, right))
    };

    // Sincroniza rascunho quando a celula selecionada mudar externamente (sem apagar digitacao ativa)
    let fx_id = egui::Id::new(("sheet_fx_bar", tool_id));
    let fx_focused = ui.ctx().memory(|m| m.has_focus(fx_id));
    if !inline_editing && !fx_focused {
        st.sheet_cell_draft = sel_cell.v.clone();
    }

    let send_sheet = |change: Value, cmds: &mut Vec<Value>| {
        cmds.push(json!({ "type": "tool-sheet", "id": tool_id, "change": change }));
    };

    let selected_keys = || -> Vec<String> {
        let mut out = Vec::with_capacity((bottom - top + 1) * (right - left + 1));
        for r in top..=bottom {
            for c in left..=right {
                out.push(sheet_cell_key(r, c));
            }
        }
        out
    };

    let raw_block = |r0: usize, c0: usize, h: usize, w: usize| -> Vec<Vec<String>> {
        (0..h)
            .map(|dr| {
                (0..w)
                    .map(|dc| {
                        cells
                            .get(&sheet_cell_key(r0 + dr, c0 + dc))
                            .map(|c| c.v.clone())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect()
    };

    let write_block = |r0: usize,
                       c0: usize,
                       after: Vec<Vec<String>>,
                       st: &mut crate::net::room_tools::RoomToolsState,
                       cmds: &mut Vec<Value>| {
        if !can_use || after.is_empty() {
            return;
        }
        let max_w = after.iter().map(|r| r.len()).max().unwrap_or(0);
        if max_w == 0 {
            return;
        }
        let before = raw_block(r0, c0, after.len(), max_w);
        if before == after {
            return;
        }
        st.sheet_undo_blocks.push((r0, c0, before, after.clone()));
        if st.sheet_undo_blocks.len() > 100 {
            st.sheet_undo_blocks.remove(0);
        }
        st.sheet_redo_blocks.clear();
        send_sheet(
            json!({ "op": "fill", "cell": sheet_cell_key(r0, c0), "values": after }),
            cmds,
        );
    };

    let move_selection = |nr: usize,
                          nc: usize,
                          extend: bool,
                          st: &mut crate::net::room_tools::RoomToolsState,
                          cells: &std::collections::HashMap<String, SheetCell>| {
        let nr = nr.min(rows.saturating_sub(1));
        let nc = nc.min(cols.saturating_sub(1));
        let nk = sheet_cell_key(nr, nc);
        st.sheet_sel_row = nr;
        st.sheet_sel_col = nc;
        if !extend {
            st.sheet_anchor_row = nr;
            st.sheet_anchor_col = nc;
        }
        st.sheet_selected_cell = nk.clone();
        st.sheet_cell_draft = cells.get(&nk).map(|x| x.v.clone()).unwrap_or_default();
        st.sheet_editing_inline.insert(tool_id.to_string(), false);
        st.sheet_inline_just_started.remove(tool_id);
    };

    let selection_tsv = || -> String {
        let mut lines = Vec::with_capacity(bottom - top + 1);
        for r in top..=bottom {
            let mut flds = Vec::with_capacity(right - left + 1);
            for c in left..=right {
                flds.push(eval_sheet_display(cells, r, c));
            }
            lines.push(flds.join("\t"));
        }
        lines.join("\n")
    };

    // Limpa aviso expirado
    let sheet_notice = st.sheet_export_notice.get(tool_id).and_then(|(msg, ts)| {
        if ts.elapsed().as_secs_f32() < 3.2 {
            Some(msg.clone())
        } else {
            None
        }
    });
    if sheet_notice.is_none() {
        st.sheet_export_notice.remove(tool_id);
    }

    // Toolbar fiel ao SheetView.tsx em linha unica com scroll horizontal quando estreita
    let pal_mode = st.sheet_palette_mode.get(tool_id).copied().unwrap_or(0);
    if can_use {
    egui::Frame::none()
        .fill(Color32::from_rgb(22, 27, 34))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 49, 60)))
        .rounding(Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(6.0, 3.0))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(("sheet_tb_scroll", tool_id))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            BG_ELEVATED,
                            Stroke::NONE,
                            Rounding::same(4.0),
                            "Desfazer (Ctrl+Z)",
                            |p, c, col| icons::paint_undo(p, c, 11.5, col),
                            if st.sheet_undo_blocks.is_empty() && st.sheet_undo.is_empty() {
                                TEXT_MUTED
                            } else {
                                TEXT_PRIMARY
                            },
                        )
                        .clicked()
                            && can_use
                        {
                            if let Some((ur, uc, before, after)) = st.sheet_undo_blocks.pop() {
                                st.sheet_redo_blocks
                                    .push((ur, uc, before.clone(), after));
                                send_sheet(
                                    json!({ "op": "fill", "cell": sheet_cell_key(ur, uc), "values": before }),
                                    cmds,
                                );
                            }
                        }
                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            BG_ELEVATED,
                            Stroke::NONE,
                            Rounding::same(4.0),
                            "Refazer (Ctrl+Y)",
                            |p, c, col| icons::paint_redo(p, c, 11.5, col),
                            if st.sheet_redo_blocks.is_empty() {
                                TEXT_MUTED
                            } else {
                                TEXT_PRIMARY
                            },
                        )
                        .clicked()
                            && can_use
                        {
                            if let Some((rr, rc, before, after)) = st.sheet_redo_blocks.pop() {
                                st.sheet_undo_blocks.push((rr, rc, before, after.clone()));
                                send_sheet(
                                    json!({ "op": "fill", "cell": sheet_cell_key(rr, rc), "values": after }),
                                    cmds,
                                );
                            }
                        }
                        ui.separator();

                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("B").size(11.5).semibold().color(Color32::WHITE),
                                )
                                .fill(if sel_cell.b { EMERALD } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Negrito (Ctrl+B)")
                            .clicked()
                        {
                            send_sheet(
                                json!({ "op": "style", "cells": selected_keys(), "style": { "b": !sel_cell.b } }),
                                cmds,
                            );
                        }
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("I").size(11.5).italics().color(Color32::WHITE),
                                )
                                .fill(if sel_cell.i { EMERALD } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Italico (Ctrl+I)")
                            .clicked()
                        {
                            send_sheet(
                                json!({ "op": "style", "cells": selected_keys(), "style": { "i": !sel_cell.i } }),
                                cmds,
                            );
                        }

                        for (mode, al_name, tip) in [
                            (0u8, "left", "Alinhar esquerda"),
                            (1, "center", "Centralizar"),
                            (2, "right", "Alinhar direita"),
                        ] {
                            let active = sel_cell.align.as_deref() == Some(al_name);
                            if icons::icon_button(
                                ui,
                                Vec2::splat(22.0),
                                if active { EMERALD } else { Color32::TRANSPARENT },
                                Stroke::NONE,
                                Rounding::same(4.0),
                                tip,
                                |p, c, col| icons::paint_align_lines(p, c, 11.5, col, mode),
                                TEXT_PRIMARY,
                            )
                            .clicked()
                                && can_use
                            {
                                let next_al = if active { Value::Null } else { json!(al_name) };
                                send_sheet(
                                    json!({ "op": "style", "cells": selected_keys(), "style": { "align": next_al } }),
                                    cmds,
                                );
                            }
                        }

                        if icons::icon_button(
                            ui,
                            Vec2::splat(22.0),
                            if pal_mode == 1 { BLUE } else { BG_ELEVATED },
                            Stroke::NONE,
                            Rounding::same(4.0),
                            "Cor de preenchimento da celula",
                            |p, c, col| icons::paint_fill_bucket(p, c, 11.5, col),
                            TEXT_PRIMARY,
                        )
                        .clicked()
                            && can_use
                        {
                            st.sheet_palette_mode
                                .insert(tool_id.to_string(), if pal_mode == 1 { 0 } else { 1 });
                        }
                        if ui
                            .add_enabled(
                                can_use,
                                egui::Button::new(
                                    RichText::new("A")
                                        .size(11.5)
                                        .underline()
                                        .semibold()
                                        .color(Color32::WHITE),
                                )
                                .fill(if pal_mode == 2 { BLUE } else { BG_ELEVATED })
                                .min_size(Vec2::splat(22.0)),
                            )
                            .on_hover_text("Cor do texto")
                            .clicked()
                        {
                            st.sheet_palette_mode
                                .insert(tool_id.to_string(), if pal_mode == 2 { 0 } else { 2 });
                        }

                        ui.separator();
                        if ui
                            .add_enabled(
                                can_use && rows < 500,
                                egui::Button::new(
                                    RichText::new("+ Linha").size(11.0).color(TEXT_PRIMARY),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            send_sheet(json!({ "op": "insert", "axis": "row", "at": top }), cmds);
                        }
                        if ui
                            .add_enabled(
                                can_use && cols < 52,
                                egui::Button::new(
                                    RichText::new("+ Coluna").size(11.0).color(TEXT_PRIMARY),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            send_sheet(
                                json!({ "op": "insert", "axis": "col", "at": left }),
                                cmds,
                            );
                        }
                        if ui
                            .add_enabled(
                                can_use && rows > 1,
                                egui::Button::new(
                                    RichText::new("− Linha").size(11.0).color(TEXT_SECONDARY),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            send_sheet(json!({ "op": "delete", "axis": "row", "at": top }), cmds);
                        }
                        if ui
                            .add_enabled(
                                can_use && cols > 1,
                                egui::Button::new(
                                    RichText::new("− Coluna").size(11.0).color(TEXT_SECONDARY),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            send_sheet(
                                json!({ "op": "delete", "axis": "col", "at": left }),
                                cmds,
                            );
                        }

                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Excel").size(10.5).color(TEXT_PRIMARY),
                                )
                                .fill(BG_ELEVATED)
                                .rounding(Rounding::same(4.0)),
                            )
                            .on_hover_text("Baixar planilha como Excel (.xls)")
                            .clicked()
                        {
                            let xls = sheet_to_excel_xls(rows, cols, cells, col_widths, title);
                            let name = if title.trim().is_empty() {
                                "planilha"
                            } else {
                                title.trim()
                            };
                            if let Some(p) =
                                save_export_to_downloads(name, "xls", xls.as_bytes())
                            {
                                st.sheet_export_notice.insert(
                                    tool_id.to_string(),
                                    (
                                        format!(
                                            "Salvo: {}",
                                            p.file_name()
                                                .and_then(|f| f.to_str())
                                                .unwrap_or("Downloads")
                                        ),
                                        Instant::now(),
                                    ),
                                );
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("CSV").size(10.5).color(TEXT_SECONDARY),
                                )
                                .fill(BG_ELEVATED)
                                .rounding(Rounding::same(4.0)),
                            )
                            .on_hover_text("Salvar CSV (;) em Downloads e copiar")
                            .clicked()
                        {
                            let csv = sheet_to_csv_semicolon(rows, cols, cells);
                            ui.ctx().copy_text(csv.clone());
                            let name = if title.trim().is_empty() {
                                "planilha"
                            } else {
                                title.trim()
                            };
                            if let Some(p) =
                                save_export_to_downloads(name, "csv", csv.as_bytes())
                            {
                                st.sheet_export_notice.insert(
                                    tool_id.to_string(),
                                    (
                                        format!(
                                            "Salvo: {}",
                                            p.file_name()
                                                .and_then(|f| f.to_str())
                                                .unwrap_or("Downloads")
                                        ),
                                        Instant::now(),
                                    ),
                                );
                            }
                        }
                    });
                });

            if pal_mode != 0 && can_use {
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(if pal_mode == 1 { "Fundo:" } else { "Texto:" })
                            .size(10.5)
                            .color(TEXT_MUTED),
                    );
                    if pal_mode == 1 {
                        for &(hex, col) in SHEET_FILL_PALETTE {
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                            ui.painter().rect_filled(r, Rounding::same(3.0), col);
                            ui.painter().rect_stroke(
                                r,
                                Rounding::same(3.0),
                                Stroke::new(1.0_f32, BORDER_SUBTLE),
                            );
                            if resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text(hex)
                                .clicked()
                            {
                                send_sheet(
                                    json!({ "op": "style", "cells": selected_keys(), "style": { "bg": hex } }),
                                    cmds,
                                );
                                st.sheet_palette_mode.insert(tool_id.to_string(), 0);
                            }
                        }
                        if ui.small_button("Sem cor").clicked() {
                            send_sheet(
                                json!({ "op": "style", "cells": selected_keys(), "style": { "bg": Value::Null } }),
                                cmds,
                            );
                            st.sheet_palette_mode.insert(tool_id.to_string(), 0);
                        }
                    } else {
                        for &(hex, col) in SHEET_TEXT_PALETTE {
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 6.0, col);
                            if resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text(hex)
                                .clicked()
                            {
                                send_sheet(
                                    json!({ "op": "style", "cells": selected_keys(), "style": { "color": hex } }),
                                    cmds,
                                );
                                st.sheet_palette_mode.insert(tool_id.to_string(), 0);
                            }
                        }
                        if ui.small_button("Sem cor").clicked() {
                            send_sheet(
                                json!({ "op": "style", "cells": selected_keys(), "style": { "color": Value::Null } }),
                                cmds,
                            );
                            st.sheet_palette_mode.insert(tool_id.to_string(), 0);
                        }
                    }
                });
            }

            if let Some(ref msg) = sheet_notice {
                ui.add_space(2.0);
                ui.label(RichText::new(msg).size(10.5).color(EMERALD_BRIGHT));
            }
        });
    }

    ui.add_space(3.0);
    // Barra de formula: [ A1 | fx <input> ]
    egui::Frame::none()
        .fill(Color32::from_rgb(14, 18, 25))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 49, 60)))
        .rounding(Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_sized(
                    Vec2::new(56.0, 22.0),
                    egui::Label::new(
                        RichText::new(&selection_label)
                            .size(11.0)
                            .semibold()
                            .color(TEXT_SECONDARY),
                    )
                    .truncate(),
                );
                ui.separator();
                ui.label(RichText::new("fx").size(11.5).italics().color(TEXT_MUTED));
                let fw = ui.available_width().max(80.0);
                let fx_resp = ui.add_enabled(
                    can_use,
                    egui::TextEdit::singleline(&mut st.sheet_cell_draft)
                        .id_source(fx_id)
                        .frame(false)
                        .desired_width(fw)
                        .hint_text("Digite na celula ou =SOMA(A1:A4)"),
                );
                if fx_resp.gained_focus() && !inline_editing {
                    st.sheet_cell_draft = sel_cell.v.clone();
                }
                let enter = fx_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let esc = fx_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
                if esc {
                    st.sheet_cell_draft = sel_cell.v.clone();
                    fx_resp.surrender_focus();
                } else if (enter || fx_resp.lost_focus())
                    && can_use
                    && !inline_editing
                    && st.sheet_cell_draft != sel_cell.v
                {
                    let draft = st.sheet_cell_draft.clone();
                    write_block(sel_r, sel_c, vec![vec![draft]], st, cmds);
                    if enter {
                        move_selection(sel_r + 1, sel_c, false, st, cells);
                    }
                }
            });
        });

    ui.add_space(3.0);
    let row_hdr_w = 36.0_f32;
    let cell_h = 24.0_f32;
    // Somente visualizacao: a toolbar some (como no site) e a nota fica embaixo da grade.
    let avail_h = (ui.available_height() - if can_use { 0.0 } else { 20.0 }).max(50.0);
    let grid_focus_id = egui::Id::new(("sheet_grid_focus", tool_id));
    let just_started_pending = st
        .sheet_inline_just_started
        .get(tool_id)
        .copied()
        .unwrap_or(false);

    // Atalhos de teclado direto na grade (setas, Shift+seta, Tab, Enter/F2, Del, Ctrl+C/X/V/B/I/Z/Y/A, digitar direto)
    if !inline_editing && !just_started_pending {
        let fx_has = ui.ctx().memory(|m| m.has_focus(fx_id));
        let other_text = ui.ctx().wants_keyboard_input();
        if !fx_has && !other_text {
            let (
                shift,
                mod_k,
                enter,
                tab,
                del,
                up,
                down,
                left_k,
                right_k,
                kb_b,
                kb_i,
                kb_z,
                kb_y,
                kb_a,
                copied_ev,
                cut_ev,
                pasted_txt,
                typed,
            ) = ui.input(|i| {
                let m = i.modifiers.ctrl || i.modifiers.command;
                let s = i.modifiers.shift;
                let mut cp = m && i.key_pressed(egui::Key::C);
                let mut ct = m && i.key_pressed(egui::Key::X);
                let mut pst: Option<String> = None;
                let mut txt = String::new();
                for ev in &i.events {
                    match ev {
                        egui::Event::Copy => cp = true,
                        egui::Event::Cut => ct = true,
                        egui::Event::Paste(v) => pst = Some(v.clone()),
                        egui::Event::Text(t)
                            if !m && !i.modifiers.alt && !t.chars().any(char::is_control) =>
                        {
                            txt.push_str(t);
                        }
                        _ => {}
                    }
                }
                (
                    s,
                    m,
                    i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::F2),
                    i.key_pressed(egui::Key::Tab),
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                    m && i.key_pressed(egui::Key::B),
                    m && i.key_pressed(egui::Key::I),
                    m && !s && i.key_pressed(egui::Key::Z),
                    m && (i.key_pressed(egui::Key::Y) || (s && i.key_pressed(egui::Key::Z))),
                    m && i.key_pressed(egui::Key::A),
                    cp,
                    ct,
                    pst,
                    txt,
                )
            });

            if kb_a {
                st.sheet_anchor_row = 0;
                st.sheet_anchor_col = 0;
                st.sheet_sel_row = rows.saturating_sub(1);
                st.sheet_sel_col = cols.saturating_sub(1);
            } else if copied_ev {
                ui.ctx().copy_text(selection_tsv());
            } else if ct_check(cut_ev) && can_use {
                ui.ctx().copy_text(selection_tsv());
                let empty = vec![vec![String::new(); right - left + 1]; bottom - top + 1];
                write_block(top, left, empty, st, cmds);
            } else if let Some(pt) = pasted_txt {
                if can_use && !pt.is_empty() {
                    let mut grid = parse_pasted_tsv_grid(&pt);
                    grid.truncate(rows.saturating_sub(top));
                    let max_cols = cols.saturating_sub(left);
                    for line in &mut grid {
                        line.truncate(max_cols);
                    }
                    if grid.len() == 1
                        && grid[0].len() == 1
                        && (bottom > top || right > left)
                    {
                        let val = grid[0][0].clone();
                        let block = vec![vec![val; right - left + 1]; bottom - top + 1];
                        write_block(top, left, block, st, cmds);
                    } else if !grid.is_empty() {
                        let w = grid.iter().map(|r| r.len()).max().unwrap_or(0);
                        if w > 0 {
                            for line in &mut grid {
                                while line.len() < w {
                                    line.push(String::new());
                                }
                            }
                            let h = grid.len();
                            write_block(top, left, grid, st, cmds);
                            st.sheet_anchor_row = top;
                            st.sheet_anchor_col = left;
                            st.sheet_sel_row = (top + h - 1).min(rows - 1);
                            st.sheet_sel_col = (left + w - 1).min(cols - 1);
                        }
                    }
                }
            } else if kb_b && can_use {
                send_sheet(
                    json!({ "op": "style", "cells": selected_keys(), "style": { "b": !sel_cell.b } }),
                    cmds,
                );
            } else if kb_i && can_use {
                send_sheet(
                    json!({ "op": "style", "cells": selected_keys(), "style": { "i": !sel_cell.i } }),
                    cmds,
                );
            } else if kb_z && can_use {
                if let Some((ur, uc, before, after)) = st.sheet_undo_blocks.pop() {
                    st.sheet_redo_blocks.push((ur, uc, before.clone(), after));
                    send_sheet(
                        json!({ "op": "fill", "cell": sheet_cell_key(ur, uc), "values": before }),
                        cmds,
                    );
                }
            } else if kb_y && can_use {
                if let Some((rr, rc, before, after)) = st.sheet_redo_blocks.pop() {
                    st.sheet_undo_blocks.push((rr, rc, before, after.clone()));
                    send_sheet(
                        json!({ "op": "fill", "cell": sheet_cell_key(rr, rc), "values": after }),
                        cmds,
                    );
                }
            } else if !mod_k && !typed.is_empty() && can_use {
                st.sheet_cell_draft = typed;
                st.sheet_editing_inline.insert(tool_id.to_string(), true);
                st.sheet_inline_just_started.insert(tool_id.to_string(), true);
            } else if enter && can_use {
                st.sheet_cell_draft = sel_cell.v.clone();
                st.sheet_editing_inline.insert(tool_id.to_string(), true);
                st.sheet_inline_just_started.insert(tool_id.to_string(), true);
            } else if del && can_use {
                let empty = vec![vec![String::new(); right - left + 1]; bottom - top + 1];
                write_block(top, left, empty, st, cmds);
                st.sheet_cell_draft.clear();
            } else if up {
                move_selection(sel_r.saturating_sub(1), sel_c, shift, st, cells);
            } else if down {
                move_selection(sel_r + 1, sel_c, shift, st, cells);
            } else if left_k {
                move_selection(sel_r, sel_c.saturating_sub(1), shift, st, cells);
            } else if right_k {
                move_selection(sel_r, sel_c + 1, shift, st, cells);
            } else if tab {
                let nc = if shift { sel_c.saturating_sub(1) } else { sel_c + 1 };
                move_selection(sel_r, nc, false, st, cells);
            }
        }
    }

    let width_of_col = |c: usize, st: &crate::net::room_tools::RoomToolsState| -> f32 {
        if let Some((ref tid, rc, _, cur_w)) = st.sheet_col_resize {
            if tid == tool_id && rc == c {
                return cur_w;
            }
        }
        col_widths
            .get(&c)
            .copied()
            .unwrap_or(SHEET_DEFAULT_COL_WIDTH) as f32
    };

    let grid_bg = Color32::from_rgb(22, 27, 34);
    let hdr_bg = Color32::from_rgb(28, 34, 43);
    let grid_line = Color32::from_rgb(42, 49, 60);

    if ui.input(|i| !i.pointer.primary_down()) {
        st.sheet_dragging_sel = false;
    }

    egui::ScrollArea::both()
        .id_salt(("sheet_grid_scroll", tool_id))
        .max_height(avail_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            // Cabecalho A..Z com alca de redimensionamento na borda direita de cada coluna
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (corner_r, _) =
                    ui.allocate_exact_size(Vec2::new(row_hdr_w, cell_h), Sense::hover());
                ui.painter().rect(
                    corner_r,
                    Rounding::ZERO,
                    hdr_bg,
                    Stroke::new(0.8_f32, grid_line),
                );
                for c in 0..cols {
                    let cw = width_of_col(c, st);
                    let col_in_sel = c >= left && c <= right;
                    let (hr, _) = ui.allocate_exact_size(Vec2::new(cw, cell_h), Sense::hover());
                    ui.painter().rect(
                        hr,
                        Rounding::ZERO,
                        if col_in_sel {
                            Color32::from_rgb(27, 42, 46)
                        } else {
                            hdr_bg
                        },
                        Stroke::new(0.8_f32, grid_line),
                    );
                    ui.painter().text(
                        hr.center(),
                        egui::Align2::CENTER_CENTER,
                        sheet_col_name(c),
                        FontId::proportional(11.0),
                        if col_in_sel {
                            EMERALD_BRIGHT
                        } else {
                            TEXT_SECONDARY
                        },
                    );
                    if can_use {
                        let edge_rect = Rect::from_min_max(
                            pos2(hr.right() - 4.0, hr.top()),
                            pos2(hr.right() + 2.0, hr.bottom()),
                        );
                        let edge_resp = ui
                            .interact(edge_rect, ui.id().with(("col_rz", c)), Sense::drag())
                            .on_hover_cursor(CursorIcon::ResizeHorizontal);
                        if edge_resp.drag_started() {
                            let px = edge_resp.interact_pointer_pos().map(|p| p.x).unwrap_or(0.0);
                            let base_w = col_widths
                                .get(&c)
                                .copied()
                                .unwrap_or(SHEET_DEFAULT_COL_WIDTH) as f32;
                            st.sheet_col_resize = Some((tool_id.to_string(), c, px, base_w));
                        }
                        if edge_resp.dragged() {
                            if let Some((ref tid, rc, sx, ref mut cur_w)) = st.sheet_col_resize {
                                if tid == tool_id && rc == c {
                                    if let Some(px) = ui.ctx().pointer_interact_pos().map(|p| p.x) {
                                        let base = col_widths
                                            .get(&c)
                                            .copied()
                                            .unwrap_or(SHEET_DEFAULT_COL_WIDTH)
                                            as f32;
                                        *cur_w = (base + (px - sx)).clamp(30.0, 800.0);
                                    }
                                }
                            }
                        }
                        if edge_resp.drag_stopped() {
                            if let Some((tid, rc, _, final_w)) = st.sheet_col_resize.take() {
                                if tid == tool_id && rc == c {
                                    send_sheet(
                                        json!({ "op": "width", "col": c, "width": final_w.round() as u32 }),
                                        cmds,
                                    );
                                }
                            }
                        }
                    }
                }
            });

            // Virtualizacao vertical de todas as `rows` (1..=500): desenha apenas linhas visiveis no viewport
            let total_w: f32 = row_hdr_w + (0..cols).map(|c| width_of_col(c, st)).sum::<f32>();
            let grid_top_y = ui.cursor().top();
            let vis_top = ui.clip_rect().top();
            let vis_bot = ui.clip_rect().bottom();
            let first_row = (((vis_top - grid_top_y) / cell_h).floor() as isize - 4)
                .clamp(0, rows.saturating_sub(1) as isize) as usize;
            let last_row = (((vis_bot - grid_top_y) / cell_h).ceil() as isize + 4)
                .clamp(first_row as isize, rows.saturating_sub(1) as isize) as usize;

            if first_row > 0 {
                ui.allocate_exact_size(
                    Vec2::new(total_w, first_row as f32 * cell_h),
                    Sense::hover(),
                );
            }

            let is_editing_now = st.sheet_editing_inline.get(tool_id).copied().unwrap_or(false);
            let mut commit_inline_move: Option<(usize, usize)> = None;
            let mut cancel_inline = false;
            let mut formula_ref_click: Option<(usize, usize)> = None;
            let primary_down = ui.input(|i| i.pointer.primary_down());
            let shift_held = ui.input(|i| i.modifiers.shift);

            for r in first_row..=last_row {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    let row_in_sel = r >= top && r <= bottom;
                    let (rh_r, _) =
                        ui.allocate_exact_size(Vec2::new(row_hdr_w, cell_h), Sense::hover());
                    ui.painter().rect(
                        rh_r,
                        Rounding::ZERO,
                        if row_in_sel {
                            Color32::from_rgb(27, 42, 46)
                        } else {
                            hdr_bg
                        },
                        Stroke::new(0.8_f32, grid_line),
                    );
                    ui.painter().text(
                        rh_r.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{}", r + 1),
                        FontId::proportional(10.5),
                        if row_in_sel {
                            EMERALD_BRIGHT
                        } else {
                            TEXT_MUTED
                        },
                    );

                    for c in 0..cols {
                        let cw = width_of_col(c, st);
                        let k = sheet_cell_key(r, c);
                        let cell = cells.get(&k);
                        let is_focus = r == sel_r && c == sel_c;
                        let in_range = r >= top && r <= bottom && c >= left && c <= right;
                        let has_custom_bg = cell.and_then(|x| x.bg.as_deref()).is_some();
                        let fill = cell
                            .and_then(|x| x.bg.as_deref())
                            .map(parse_hex_color32)
                            .unwrap_or(if in_range && !is_focus {
                                Color32::from_rgb(22, 42, 38)
                            } else {
                                grid_bg
                            });
                        let fg = cell
                            .and_then(|x| x.color.as_deref())
                            .map(parse_hex_color32)
                            .unwrap_or(if has_custom_bg {
                                Color32::from_rgb(24, 24, 27)
                            } else {
                                TEXT_PRIMARY
                            });

                        if is_focus && is_editing_now && can_use {
                            let (cell_r, _) =
                                ui.allocate_exact_size(Vec2::new(cw, cell_h), Sense::hover());
                            ui.painter().rect_filled(
                                cell_r,
                                Rounding::ZERO,
                                Color32::from_rgb(22, 27, 34),
                            );
                            let edit_id = egui::Id::new(("sheet_inline_edit", tool_id, r, c));
                            let mut cell_ui =
                                ui.new_child(egui::UiBuilder::new().max_rect(cell_r.shrink(2.0)));
                            let out = egui::TextEdit::singleline(&mut st.sheet_cell_draft)
                                .id_source(edit_id)
                                .frame(false)
                                .desired_width(cw - 6.0)
                                .show(&mut cell_ui);
                            let started_now =
                                st.sheet_inline_just_started.remove(tool_id).unwrap_or(false);
                            if started_now {
                                out.response.request_focus();
                                if let Some(mut state) =
                                    egui::TextEdit::load_state(ui.ctx(), edit_id)
                                {
                                    let end_cc = egui::text::CCursor::new(
                                        st.sheet_cell_draft.chars().count(),
                                    );
                                    state.cursor.set_char_range(Some(
                                        egui::text::CCursorRange::one(end_cc),
                                    ));
                                    state.store(ui.ctx(), edit_id);
                                }
                            }
                            ui.painter().rect_stroke(
                                cell_r,
                                Rounding::ZERO,
                                Stroke::new(1.8_f32, EMERALD_BRIGHT),
                            );
                            if out.response.has_focus() {
                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    cancel_inline = true;
                                } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    commit_inline_move = Some((r + 1, c));
                                } else if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
                                    let nc = if shift_held { c.saturating_sub(1) } else { c + 1 };
                                    commit_inline_move = Some((r, nc));
                                }
                            } else if !started_now && out.response.lost_focus() {
                                commit_inline_move = Some((r, c));
                            }
                        } else {
                            let (cell_r, resp) = ui.allocate_exact_size(
                                Vec2::new(cw, cell_h),
                                Sense::click_and_drag(),
                            );
                            ui.painter().rect(
                                cell_r,
                                Rounding::ZERO,
                                fill,
                                Stroke::new(
                                    if is_focus { 1.8_f32 } else { 0.8_f32 },
                                    if is_focus {
                                        EMERALD_BRIGHT
                                    } else if in_range {
                                        Color32::from_rgb(16, 185, 129)
                                    } else {
                                        grid_line
                                    },
                                ),
                            );
                            let display = eval_sheet_display(cells, r, c);
                            if !display.is_empty() {
                                // Sem alinhamento escolhido, numero vai a direita (como no site/Excel).
                                let align = match cell.and_then(|x| x.align.as_deref()) {
                                    Some("center") => egui::Align2::CENTER_CENTER,
                                    Some("right") => egui::Align2::RIGHT_CENTER,
                                    Some(_) => egui::Align2::LEFT_CENTER,
                                    None if display.trim().parse::<f64>().is_ok() => {
                                        egui::Align2::RIGHT_CENTER
                                    }
                                    None => egui::Align2::LEFT_CENTER,
                                };
                                let tx = match align {
                                    egui::Align2::CENTER_CENTER => cell_r.center().x,
                                    egui::Align2::RIGHT_CENTER => cell_r.right() - 5.0,
                                    _ => cell_r.left() + 5.0,
                                };
                                let eff_fg = if display.starts_with('#') && display.ends_with('!')
                                {
                                    Color32::from_rgb(248, 113, 113)
                                } else {
                                    fg
                                };
                                let painter_cell = ui.painter_at(cell_r.shrink(1.0));
                                let bold = cell.is_some_and(|x| x.b);
                                let italic = cell.is_some_and(|x| x.i);
                                let galley = ui.fonts(|f| {
                                    f.layout_job(egui::text::LayoutJob::single_section(
                                        display,
                                        egui::TextFormat {
                                            font_id: FontId::proportional(12.0),
                                            color: eff_fg,
                                            italics: italic,
                                            ..Default::default()
                                        },
                                    ))
                                });
                                let text_rect = align.anchor_size(
                                    pos2(tx, cell_r.center().y),
                                    galley.size(),
                                );
                                // A fonte embutida nao tem peso negrito: duas passadas de 0.6px
                                // dao o mesmo efeito sem carregar outra fonte (zero RAM extra).
                                if bold {
                                    painter_cell.galley(
                                        text_rect.min + Vec2::new(0.6, 0.0),
                                        galley.clone(),
                                        eff_fg,
                                    );
                                }
                                painter_cell.galley(text_rect.min, galley, eff_fg);
                            }

                            if resp.double_clicked() && can_use {
                                move_selection(r, c, false, st, cells);
                                st.sheet_editing_inline.insert(tool_id.to_string(), true);
                                st.sheet_inline_just_started.insert(tool_id.to_string(), true);
                            } else if resp.drag_started() || resp.clicked() {
                                if is_editing_now
                                    && can_use
                                    && st.sheet_cell_draft.trim_start().starts_with('=')
                                {
                                    formula_ref_click = Some((r, c));
                                } else if resp.clicked() && is_focus && can_use && !shift_held {
                                    st.sheet_cell_draft = sel_cell.v.clone();
                                    st.sheet_editing_inline.insert(tool_id.to_string(), true);
                                    st.sheet_inline_just_started
                                        .insert(tool_id.to_string(), true);
                                } else {
                                    if is_editing_now && can_use {
                                        let draft = st.sheet_cell_draft.clone();
                                        write_block(sel_r, sel_c, vec![vec![draft]], st, cmds);
                                    }
                                    move_selection(r, c, shift_held, st, cells);
                                    if primary_down {
                                        st.sheet_dragging_sel = true;
                                    }
                                }
                                ui.ctx().memory_mut(|m| {
                                    if m.has_focus(fx_id) {
                                        m.surrender_focus(fx_id);
                                    }
                                    m.request_focus(grid_focus_id);
                                });
                            } else if st.sheet_dragging_sel && primary_down && resp.hovered() {
                                st.sheet_sel_row = r;
                                st.sheet_sel_col = c;
                            }
                        }
                    }
                });
            }

            if last_row + 1 < rows {
                ui.allocate_exact_size(
                    Vec2::new(total_w, (rows - last_row - 1) as f32 * cell_h),
                    Sense::hover(),
                );
            }

            if let Some((fr, fc)) = formula_ref_click {
                st.sheet_cell_draft.push_str(&sheet_cell_name(fr, fc));
                st.sheet_inline_just_started.insert(tool_id.to_string(), true);
            } else if cancel_inline {
                st.sheet_cell_draft = sel_cell.v.clone();
                st.sheet_editing_inline.insert(tool_id.to_string(), false);
            } else if let Some((nr, nc)) = commit_inline_move {
                let draft = st.sheet_cell_draft.clone();
                write_block(sel_r, sel_c, vec![vec![draft]], st, cmds);
                if nr != sel_r || nc != sel_c {
                    move_selection(nr, nc, false, st, cells);
                } else {
                    st.sheet_editing_inline.insert(tool_id.to_string(), false);
                }
            }
        });

    if !can_use {
        ui.label(
            RichText::new("Somente visualização — um administrador decide quem pode usar.")
                .size(11.0)
                .color(TEXT_MUTED),
        );
    }
}

#[inline]
fn ct_check(v: bool) -> bool {
    v
}

/// Alinha cada linha de um texto montado a esquerda dentro de `width` (centro ou direita),
/// deslocando glifos, retangulos e a malha ja tesselada — o cursor e a selecao continuam certos,
/// porque tudo esta nas coordenadas da galley.
fn align_galley_rows(
    galley: std::sync::Arc<egui::Galley>,
    halign: Align,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    if halign == Align::LEFT || !width.is_finite() || width <= 0.0 {
        return galley;
    }
    let mut g = (*galley).clone();
    let mut bounds = egui::Rect::NOTHING;
    let mut mesh_bounds = egui::Rect::NOTHING;
    for row in &mut g.rows {
        let free = (width - row.rect.width()).max(0.0);
        let dx = if halign == Align::Center { (free / 2.0).floor() } else { free.floor() };
        if dx > 0.0 {
            let d = egui::vec2(dx, 0.0);
            row.rect = row.rect.translate(d);
            for glyph in &mut row.glyphs {
                glyph.pos.x += dx;
            }
            row.visuals.mesh.translate(d);
            row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(d);
        }
        bounds = bounds.union(row.rect);
        mesh_bounds = mesh_bounds.union(row.visuals.mesh_bounds);
    }
    // A caixa do texto vai do 0 ate a borda: clicar no espaco vazio a esquerda tambem posiciona.
    g.rect = egui::Rect::from_min_max(egui::pos2(0.0, bounds.min.y), egui::pos2(width.max(bounds.max.x), bounds.max.y));
    g.mesh_bounds = mesh_bounds;
    std::sync::Arc::new(g)
}

#[cfg(test)]
mod tool_error_tests {
    use super::*;

    #[test]
    fn server_tool_errors_are_shown_in_any_language() {
        assert_eq!(
            tool_error_text("Seu plano não inclui esta ferramenta.").as_deref(),
            Some("Seu plano não inclui esta ferramenta.")
        );
        assert!(tool_error_text("Your plan does not include this tool.").is_some());
        assert!(tool_error_text("Tu plan no incluye esta herramienta.").is_some());
        assert!(tool_error_text("Room not found or already closed.").is_none());
        assert!(is_plan_error("Seu plano não inclui esta ferramenta."));
        assert!(!is_plan_error("No máximo 12 ferramentas abertas por sala."));
    }
}

#[cfg(test)]
mod doc_align_tests {
    use super::*;

    fn galley(ctx: &egui::Context, text: &str, width: f32) -> std::sync::Arc<egui::Galley> {
        let mut job = egui::text::LayoutJob::single_section(
            text.to_string(),
            egui::TextFormat::simple(FontId::proportional(14.0), Color32::WHITE),
        );
        job.wrap.max_width = width;
        ctx.fonts(|f| f.layout_job(job))
    }

    #[test]
    fn centered_and_right_lines_stay_inside_the_box() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |_| {});
        let width = 300.0;
        let left = galley(&ctx, "Olá mundo", width);
        let line_w = left.rows[0].rect.width();
        assert!(left.rows[0].rect.min.x.abs() < 0.5);

        let center = align_galley_rows(left.clone(), Align::Center, width);
        let r = center.rows[0].rect;
        assert!((r.min.x - ((width - line_w) / 2.0).floor()).abs() < 1.0, "centered: {r:?}");
        assert!(r.max.x <= width + 0.5);

        let right = align_galley_rows(left.clone(), Align::RIGHT, width);
        let r = right.rows[0].rect;
        assert!(r.min.x > 0.0 && (r.max.x - width).abs() < 1.5, "right: {r:?}");
        // O cursor acompanha os glifos (clicar/navegar continua certo).
        let c0 = right.pos_from_cursor(&right.begin()).min.x;
        assert!(c0 > 0.0, "cursor moved with the line: {c0}");

        // Varias linhas: cada uma centralizada pela propria largura.
        let long = galley(&ctx, "uma frase bem comprida que quebra em mais de uma linha no editor", 160.0);
        assert!(long.rows.len() >= 2);
        let c = align_galley_rows(long, Align::Center, 160.0);
        for row in &c.rows {
            let free = 160.0 - row.rect.width();
            assert!((row.rect.min.x - (free / 2.0).floor()).abs() < 1.0);
        }
    }
}
