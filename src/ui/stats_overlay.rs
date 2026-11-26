use crate::ui::theme::SemiBoldExt;
use egui::{Align, Color32, FontId, Layout, RichText, Rounding, Stroke, Vec2};
use std::collections::HashMap;

use crate::net::protocol::PeerInfo;
use crate::net::signaling::now_epoch_ms;
use crate::net::webrtc::{
    CauseSeverity, OwnStreamStatsSnapshot, RemoteTileStatsSnapshot, RouteKind,
    TileStatsSnapshot,
};

pub struct StatsOverlayAction {
    pub close: bool,
    pub copied: bool,
}

pub fn render_stats_overlay(
    ctx: &egui::Context,
    tile_id: &str,
    video_rect: egui::Rect,
    is_self: bool,
    snapshot: Option<&TileStatsSnapshot>,
    peers: &[PeerInfo],
    copied_recently: bool,
) -> StatsOverlayAction {
    let mut action = StatsOverlayAction {
        close: false,
        copied: false,
    };

    if video_rect.width() < 120.0 || video_rect.height() < 90.0 {
        return action;
    }

    let overlay_w = (video_rect.width() - 16.0).clamp(220.0, 368.0);
    let max_h = (video_rect.height() - 16.0).max(160.0);
    let screen = ctx.screen_rect();
    let x = (video_rect.min.x + 8.0).clamp(
        screen.min.x + 4.0,
        (screen.max.x - overlay_w - 4.0).max(screen.min.x + 4.0),
    );
    let y = (video_rect.min.y + 8.0).clamp(
        screen.min.y + 4.0,
        (screen.max.y - 140.0).max(screen.min.y + 4.0),
    );

    let panel_bg = Color32::from_rgba_unmultiplied(12, 14, 18, 236);
    let border_col = Color32::from_rgba_unmultiplied(255, 255, 255, 22);
    let btn_bg = Color32::from_rgba_unmultiplied(255, 255, 255, 28);
    let divider_col = Color32::from_rgba_unmultiplied(255, 255, 255, 26);

    egui::Area::new(egui::Id::new(("stats_overlay", tile_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, y))
        .show(ctx, |ui| {
            let frame_resp = egui::Frame::none()
                .fill(panel_bg)
                .rounding(Rounding::same(10.0))
                .stroke(Stroke::new(1.0_f32, border_col))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_width((overlay_w - 20.0).max(196.0));
                    ui.set_max_height(max_h);

                    // Cabecalho: Titulo + Copiar + Fechar
                    ui.horizontal(|ui| {
                        let title = if is_self {
                            "Sua transmissão"
                        } else {
                            "Estatísticas da conexão"
                        };
                        ui.label(
                            RichText::new(title)
                                .size(12.5)
                                .semibold()
                                .color(Color32::WHITE),
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let close_btn = crate::ui::icons::icon_button(
                                ui,
                                Vec2::new(22.0, 20.0),
                                btn_bg,
                                Stroke::NONE,
                                Rounding::same(4.0),
                                "Fechar (Esc)",
                                |p, c, col| crate::ui::icons::paint_close_x(p, c, 11.0, col),
                                Color32::WHITE,
                            );
                            if close_btn.clicked() {
                                action.close = true;
                            }

                            let copy_lbl = if copied_recently { "Copiado" } else { "Copiar" };
                            let copy_btn = ui.add(
                                egui::Button::new(
                                    RichText::new(copy_lbl).size(11.0).color(Color32::WHITE),
                                )
                                .fill(btn_bg)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(4.0))
                                .min_size(Vec2::new(50.0, 20.0)),
                            );
                            if copy_btn.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                if let Ok(json_str) =
                                    build_clipboard_json(tile_id, is_self, snapshot)
                                {
                                    ctx.copy_text(json_str);
                                    action.copied = true;
                                }
                            }
                        });
                    });

                    ui.add_space(4.0);

                    egui::ScrollArea::vertical()
                        .max_height((max_h - 46.0).max(96.0))
                        .show(ui, |ui| match snapshot {
                            Some(TileStatsSnapshot::Remote(remote)) => {
                                render_remote_body(ui, remote, divider_col);
                            }
                            Some(TileStatsSnapshot::Own(own)) => {
                                render_own_body(ui, own, peers, divider_col);
                            }
                            None => {
                                if is_self {
                                    render_waiting_own_body(ui, divider_col);
                                } else {
                                    render_waiting_remote_body(ui, divider_col);
                                }
                            }
                        });
                });

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                action.close = true;
            }
            let _ = &frame_resp;
        });

    action
}

fn render_waiting_remote_body(ui: &mut egui::Ui, divider_col: Color32) {
    let muted = Color32::from_rgb(158, 160, 172);
    let primary = Color32::from_rgb(244, 244, 245);

    row_prefix(ui, "Rota", |ui| {
        ui.label(
            RichText::new(RouteKind::Unknown.label_pt())
                .font(FontId::monospace(11.2))
                .color(primary),
        );
    });
    row_prefix(ui, "Recebendo", |ui| {
        ui.label(
            RichText::new("aguardando…")
                .font(FontId::monospace(11.0))
                .color(muted),
        );
    });
    row_prefix(ui, "Quem transmite", |ui| {
        ui.label(RichText::new("aguardando…").size(11.2).color(muted));
    });
    draw_subtle_divider(ui, divider_col);
    ui.label(RichText::new("aguardando…").size(11.2).color(muted));
}

fn render_waiting_own_body(ui: &mut egui::Ui, divider_col: Color32) {
    let muted = Color32::from_rgb(158, 160, 172);
    row_prefix(ui, "Capturando", |ui| {
        ui.label(RichText::new("aguardando…").size(11.2).color(muted));
    });
    row_prefix(ui, "Enviando", |ui| {
        ui.label(RichText::new("aguardando…").size(11.2).color(muted));
    });
    draw_subtle_divider(ui, divider_col);
    ui.label(RichText::new("Por espectador").size(11.2).color(muted));
}

fn render_remote_body(
    ui: &mut egui::Ui,
    snap: &RemoteTileStatsSnapshot,
    divider_col: Color32,
) {
    let muted = Color32::from_rgb(158, 160, 172);
    let primary = Color32::from_rgb(244, 244, 245);
    let amber = Color32::from_rgb(252, 211, 77);
    let amber_soft = Color32::from_rgb(253, 230, 138);
    let red_soft = Color32::from_rgb(252, 165, 165);

    // 1. Rota
    row_prefix(ui, "Rota", |ui| {
        let route_col = if snap.route.kind.is_relayed() {
            amber
        } else {
            primary
        };
        ui.label(
            RichText::new(snap.route.kind.label_pt())
                .font(FontId::monospace(11.2))
                .color(route_col),
        );
        if let Some(ref rp) = snap.route.relay_protocol {
            ui.label(
                RichText::new(format!(" ({})", rp.to_uppercase()))
                    .font(FontId::monospace(11.2))
                    .color(primary),
            );
        }
        if let Some(rtt) = snap.route.rtt_ms {
            ui.label(
                RichText::new(format!(" · RTT {} ms", rtt))
                    .font(FontId::monospace(11.2))
                    .color(primary),
            );
        }
    });

    let local_relays = matches!(
        snap.route.kind,
        RouteKind::RelayLocal | RouteKind::RelayBoth
    );
    let remote_relays = matches!(
        snap.route.kind,
        RouteKind::RelayRemote | RouteKind::RelayBoth
    );
    if local_relays {
        ui.label(
            RichText::new(format!(
                "TURN · seu lado: {}",
                turn_provider_label(snap.route.relay_url.as_deref())
            ))
            .font(FontId::monospace(10.5))
            .color(muted),
        );
    }
    if remote_relays {
        let rem_url = snap
            .remote_diag
            .as_ref()
            .and_then(|d| d.route.as_ref())
            .and_then(|r| r.relay_url.as_deref());
        ui.label(
            RichText::new(format!(
                "TURN · lado da outra pessoa: {}",
                turn_provider_label(rem_url)
            ))
            .font(FontId::monospace(10.5))
            .color(muted),
        );
    }

    // 2. Recebendo
    row_prefix(ui, "Recebendo", |ui| {
        if let Some(ref recv) = snap.recv {
            let codec_suffix = recv
                .codec
                .as_ref()
                .map(|c| format!(" · {}", c))
                .unwrap_or_default();
            ui.label(
                RichText::new(format!(
                    "{}×{} · {} fps · {} Mbps{}",
                    recv.width,
                    recv.height,
                    recv.fps,
                    format_mbps(recv.kbps),
                    codec_suffix
                ))
                .font(FontId::monospace(11.2))
                .color(primary),
            );
        } else {
            ui.label(RichText::new("aguardando…").size(11.2).color(muted));
        }
    });
    if let Some(ref recv) = snap.recv {
        let dec_txt = match recv.hardware {
            Some(true) => " · decodificador hardware",
            Some(false) => " · decodificador software",
            None => "",
        };
        ui.label(
            RichText::new(format!(
                "perda {}% · descartados {}% · travadas {}{}",
                format_pct(recv.loss),
                format_pct(recv.drop_share),
                recv.freezes,
                dec_txt
            ))
            .font(FontId::monospace(10.8))
            .color(muted),
        );
    }

    // 3. Quem transmite
    row_prefix(ui, "Quem transmite", |ui| {
        if let Some(ref diag) = snap.remote_diag {
            if let Some(ref send) = diag.send {
                let codec_suffix = send
                    .codec
                    .as_ref()
                    .map(|c| format!(" · {}", c))
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!(
                        "{}×{} · {} fps · {} Mbps{}",
                        send.width,
                        send.height,
                        send.fps,
                        format_mbps(send.kbps),
                        codec_suffix
                    ))
                    .font(FontId::monospace(11.2))
                    .color(primary),
                );
            } else {
                ui.label(RichText::new("aguardando…").size(11.2).color(muted));
            }
        } else if snap.sender_silent {
            ui.label(
                RichText::new("sem resposta (o app de quem transmite pode estar desatualizado)")
                    .size(11.0)
                    .color(muted),
            );
        } else {
            ui.label(RichText::new("aguardando…").size(11.2).color(muted));
        }
    });
    if let Some(ref diag) = snap.remote_diag {
        if let Some(ref send) = diag.send {
            let enc_kind = match send.hardware {
                Some(true) => "hardware".to_string(),
                Some(false) => "software".to_string(),
                None => send.encoder.clone().unwrap_or_else(|| "?".to_string()),
            };
            let cores_txt = diag
                .cores
                .map(|c| format!(" · {} núcleos", c))
                .unwrap_or_default();
            let tier_txt = diag
                .tier
                .as_ref()
                .map(|t| format!(" · {}", t))
                .unwrap_or_default();
            ui.label(
                RichText::new(format!("codificador {}{}{}", enc_kind, cores_txt, tier_txt))
                    .font(FontId::monospace(10.8))
                    .color(muted),
            );

            if send.cpu_limited_share >= send.bandwidth_limited_share
                && send.cpu_limited_share > 0.0
            {
                ui.label(
                    RichText::new(format!(
                        "limitado pelo processador {}% do tempo",
                        format_pct(send.cpu_limited_share)
                    ))
                    .size(11.0)
                    .color(amber),
                );
            } else if send.bandwidth_limited_share > 0.0 {
                ui.label(
                    RichText::new(format!(
                        "limitado pela banda {}% do tempo",
                        format_pct(send.bandwidth_limited_share)
                    ))
                    .size(11.0)
                    .color(amber),
                );
            }
        }
    }

    draw_subtle_divider(ui, divider_col);

    // 4. Veredicto / Causas
    if snap.not_connected {
        ui.label(
            RichText::new(
                "• A conexão entre vocês não está fechando. Quase sempre é a rede de um dos dois (operadora com CGNAT, firewall, rede de empresa ou faculdade) bloqueando a conexão direta.",
            )
            .size(11.2)
            .color(red_soft),
        );
        if snap.turn_configured {
            ui.label(
                RichText::new(
                    "Nem pelos nossos servidores deu para ligar vocês. Toque em \"Copiar\" e mande para o suporte.",
                )
                .size(11.2)
                .color(red_soft),
            );
        }
    }

    if !(snap.not_connected && snap.causes.is_empty()) {
        if snap.causes.is_empty() {
            ui.label(
                RichText::new("Nenhum problema detectado agora.")
                    .size(11.2)
                    .color(muted),
            );
        } else {
            for cause in &snap.causes {
                let col = match cause.severity {
                    CauseSeverity::High => red_soft,
                    CauseSeverity::Medium => amber_soft,
                };
                ui.label(
                    RichText::new(format!("• {}", cause.message_pt))
                        .size(11.2)
                        .color(col),
                );
            }
        }
    }
}

fn render_own_body(
    ui: &mut egui::Ui,
    own: &OwnStreamStatsSnapshot,
    peers: &[PeerInfo],
    divider_col: Color32,
) {
    let muted = Color32::from_rgb(158, 160, 172);
    let primary = Color32::from_rgb(244, 244, 245);
    let viewer_dim = Color32::from_rgb(212, 212, 216);
    let amber = Color32::from_rgb(252, 211, 77);
    let amber_soft = Color32::from_rgb(253, 230, 138);
    let red_soft = Color32::from_rgb(252, 165, 165);

    let peer_map: HashMap<&str, &PeerInfo> = peers.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut visible_viewers: Vec<_> = own
        .viewers
        .iter()
        .filter(|row| {
            !peer_map
                .get(row.peer_id.as_str())
                .is_some_and(|p| p.is_silent_moderator())
        })
        .collect();

    visible_viewers.sort_by(|a, b| {
        let na = peer_name(&a.peer_id, &peer_map);
        let nb = peer_name(&b.peer_id, &peer_map);
        na.to_lowercase().cmp(&nb.to_lowercase())
    });

    row_prefix(ui, "Capturando", |ui| {
        if own.capture_width > 0 && own.capture_height > 0 {
            let fps_txt = if own.capture_fps > 0 {
                format!(" · {} fps", own.capture_fps)
            } else {
                String::new()
            };
            ui.label(
                RichText::new(format!(
                    "{}×{}{}",
                    own.capture_width, own.capture_height, fps_txt
                ))
                .font(FontId::monospace(11.2))
                .color(primary),
            );
        } else {
            ui.label(RichText::new("aguardando…").size(11.2).color(muted));
        }
    });

    let max_avail_kbps = visible_viewers
        .iter()
        .filter_map(|v| v.route.available_outgoing_kbps)
        .max()
        .unwrap_or(0);

    row_prefix(ui, "Enviando", |ui| {
        ui.label(
            RichText::new(format!(
                "{} espectador(es) · {} Mbps",
                visible_viewers.len(),
                format_mbps(own.total_kbps)
            ))
            .font(FontId::monospace(11.2))
            .color(primary),
        );
        if max_avail_kbps > 0 {
            ui.label(
                RichText::new(format!(" de ~{} Mbps estimados", format_mbps(max_avail_kbps)))
                    .font(FontId::monospace(10.8))
                    .color(muted),
            );
        }
    });

    row_prefix(ui, "Codificador", |ui| {
        let cores_txt = own
            .cores
            .map(|c| format!(" · {} núcleos", c))
            .unwrap_or_default();
        let enc_kind = if own.hardware { "hardware" } else { "software" };
        let enc_detail = if own.encoder.trim().is_empty() {
            String::new()
        } else {
            format!(" ({})", own.encoder.trim())
        };
        ui.label(
            RichText::new(format!(
                "{} · {}{}{}",
                own.codec, enc_kind, enc_detail, cores_txt
            ))
            .font(FontId::monospace(11.2))
            .color(primary),
        );
    });

    draw_subtle_divider(ui, divider_col);

    ui.label(RichText::new("Por espectador").size(11.2).color(muted));
    ui.add_space(2.0);

    if visible_viewers.is_empty() {
        ui.label(
            RichText::new("Ninguém recebendo sua transmissão agora.")
                .size(11.2)
                .color(muted),
        );
        return;
    }

    for row in visible_viewers {
        let name = peer_name(&row.peer_id, &peer_map);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(RichText::new(name).size(11.2).semibold().color(Color32::WHITE));
            let route_lbl = if row.route.kind.is_relayed() {
                "TURN"
            } else if row.route.kind == RouteKind::Unknown {
                "Conectando…"
            } else {
                "Direta"
            };
            let route_col = if row.route.kind.is_relayed() {
                amber
            } else {
                muted
            };
            ui.label(
                RichText::new(format!(" · {}", route_lbl))
                    .font(FontId::monospace(10.8))
                    .color(route_col),
            );
            if row.route.local_type == "relay" {
                ui.label(
                    RichText::new(format!(
                        " · {}",
                        turn_provider_label(row.route.relay_url.as_deref())
                    ))
                    .font(FontId::monospace(10.8))
                    .color(muted),
                );
            }
        });

        if row.paused {
            ui.label(
                RichText::new("Não está olhando: nada sendo enviado agora")
                    .size(10.8)
                    .color(muted),
            );
        } else if let Some(ref send) = row.send {
            let rtt_txt = row
                .route
                .rtt_ms
                .map(|r| format!(" · RTT {} ms", r))
                .unwrap_or_default();
            ui.label(
                RichText::new(format!(
                    "{}×{} · {} fps · {} Mbps{}",
                    send.width,
                    send.height,
                    send.fps,
                    format_mbps(send.kbps),
                    rtt_txt
                ))
                .font(FontId::monospace(10.8))
                .color(viewer_dim),
            );
        }

        if row.not_connected {
            let suffix = if own.turn_configured {
                " Nem pelos nossos servidores deu para ligar vocês."
            } else {
                ""
            };
            ui.label(
                RichText::new(format!(
                    "A conexão entre vocês não está fechando.{}",
                    suffix
                ))
                .size(10.8)
                .color(red_soft),
            );
        } else if let Some(ref worst) = row.worst_cause {
            let col = match worst.severity {
                CauseSeverity::High => red_soft,
                CauseSeverity::Medium => amber_soft,
            };
            ui.label(RichText::new(worst.message_pt).size(10.8).color(col));
        }

        ui.add_space(3.0);
    }
}

fn row_prefix(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let muted = Color32::from_rgb(158, 160, 172);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(
            RichText::new(format!("{}: ", label))
                .size(11.3)
                .color(muted),
        );
        content(ui);
    });
    ui.add_space(1.0);
}

fn draw_subtle_divider(ui: &mut egui::Ui, color: Color32) {
    ui.add_space(5.0);
    let avail_w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(avail_w, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
    ui.add_space(5.0);
}

fn peer_name(peer_id: &str, map: &HashMap<&str, &PeerInfo>) -> String {
    if let Some(p) = map.get(peer_id) {
        p.name.clone()
    } else {
        peer_id.chars().take(6).collect()
    }
}

fn turn_provider_label(url: Option<&str>) -> &'static str {
    match url {
        Some(u) if u.to_ascii_lowercase().contains("cloudflare.com") => "Cloudflare",
        Some(_) => "servidor do Go Live",
        None => "não informado",
    }
}

fn format_mbps(kbps: u32) -> String {
    format!("{:.1}", (kbps as f32) / 1000.0)
}

fn format_pct(share: f32) -> i32 {
    (share.clamp(0.0, 1.0) * 100.0).round() as i32
}

fn build_clipboard_json(
    tile_id: &str,
    is_self: bool,
    snapshot: Option<&TileStatsSnapshot>,
) -> Result<String, serde_json::Error> {
    let payload = match snapshot {
        Some(TileStatsSnapshot::Remote(r)) => serde_json::json!({
            "atEpochMs": now_epoch_ms(),
            "goliveLite": true,
            "tileId": tile_id,
            "kind": "remote",
            "stats": r,
        }),
        Some(TileStatsSnapshot::Own(o)) => serde_json::json!({
            "atEpochMs": now_epoch_ms(),
            "goliveLite": true,
            "tileId": tile_id,
            "kind": "own",
            "stats": o,
        }),
        None => serde_json::json!({
            "atEpochMs": now_epoch_ms(),
            "goliveLite": true,
            "tileId": tile_id,
            "kind": if is_self { "own" } else { "remote" },
            "stats": serde_json::Value::Null,
        }),
    };
    serde_json::to_string_pretty(&payload)
}
