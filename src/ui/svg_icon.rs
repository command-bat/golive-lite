//! Icones do site desenhados a partir do proprio SVG (`react-icons/md`, viewBox 24x24).
//!
//! Le o atributo `d` (M L H V C S Q T A Z, absolutos e relativos), achata curvas e arcos em
//! poligonos e preenche com a regra nonzero (a do SVG). Cada icone vira uma mascara branca de
//! 64x64 criada uma unica vez e guardada no egui; desenhar custa uma imagem tingida.

use egui::{pos2, Color32, Painter, Pos2, Rect};

type Poly = Vec<(f32, f32)>;

fn parse_numbers_and_cmds(d: &str) -> Vec<Tok> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            if c == '-' || c == '+' {
                i += 1;
            }
            let mut seen_dot = false;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if (ch == 'e' || ch == 'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse::<f32>() {
                out.push(Tok::Num(v, start, i));
            }
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Clone, Copy)]
enum Tok {
    Cmd(char),
    /// valor, inicio e fim no texto (flags de arco compactadas: "011" = 0, 1, 1).
    Num(f32, usize, usize),
}

/// Le os argumentos de um comando; para arcos, os dois flags podem vir colados ("01").
fn take_args(toks: &[Tok], pos: &mut usize, d: &str, arc: bool) -> Option<Vec<f32>> {
    let n = if arc { 7 } else { 0 };
    let mut v = Vec::new();
    if arc {
        while v.len() < n {
            match toks.get(*pos)? {
                Tok::Num(x, s, e) => {
                    if (v.len() == 3 || v.len() == 4) && e - s > 1 && matches!(d.as_bytes()[*s], b'0' | b'1') {
                        // flag colado ao proximo numero: consome um caractere
                        v.push(if d.as_bytes()[*s] == b'1' { 1.0 } else { 0.0 });
                        let rest = &d[s + 1..*e];
                        if let Ok(r) = rest.parse::<f32>() {
                            // reaproveita o resto como proximo token
                            let next_val = r;
                            *pos += 1;
                            if v.len() == 4 && rest.len() > 1 && matches!(rest.as_bytes()[0], b'0' | b'1') {
                                v.push(if rest.as_bytes()[0] == b'1' { 1.0 } else { 0.0 });
                                if let Ok(r2) = rest[1..].parse::<f32>() {
                                    v.push(r2);
                                }
                            } else {
                                v.push(next_val);
                            }
                            continue;
                        }
                    }
                    v.push(*x);
                    *pos += 1;
                }
                Tok::Cmd(_) => return None,
            }
        }
        v.truncate(n);
        return Some(v);
    }
    None
}

fn read_n(toks: &[Tok], pos: &mut usize, n: usize) -> Option<Vec<f32>> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        match toks.get(*pos)? {
            Tok::Num(x, _, _) => {
                v.push(*x);
                *pos += 1;
            }
            Tok::Cmd(_) => return None,
        }
    }
    Some(v)
}

fn cubic(out: &mut Poly, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) {
    const STEPS: usize = 10;
    for s in 1..=STEPS {
        let t = s as f32 / STEPS as f32;
        let u = 1.0 - t;
        let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
        let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
        out.push((x, y));
    }
}

/// Arco SVG (parametrizacao por extremos -> centro, SVG 1.1 apendice F.6.5).
#[allow(clippy::too_many_arguments)]
fn arc(out: &mut Poly, p0: (f32, f32), mut rx: f32, mut ry: f32, phi_deg: f32, large: bool, sweep: bool, p1: (f32, f32)) {
    if rx.abs() < 1e-6 || ry.abs() < 1e-6 || (p0.0 - p1.0).abs() + (p0.1 - p1.1).abs() < 1e-6 {
        out.push(p1);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    let phi = phi_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (p0.0 - p1.0) / 2.0;
    let dy = (p0.1 - p1.1) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * (rx * y1 / ry);
    let cy1 = coef * (-ry * x1 / rx);
    let cx = cp * cx1 - sp * cy1 + (p0.0 + p1.0) / 2.0;
    let cy = sp * cx1 + cp * cy1 + (p0.1 + p1.1) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let theta1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dtheta = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }
    let steps = ((dtheta.abs() / (std::f32::consts::PI / 12.0)).ceil() as usize).max(2);
    for s in 1..=steps {
        let t = theta1 + dtheta * s as f32 / steps as f32;
        let (st, ct) = t.sin_cos();
        out.push((cx + rx * cp * ct - ry * sp * st, cy + rx * sp * ct + ry * cp * st));
    }
}

/// Achata o `d` em subcaminhos fechados.
fn flatten(d: &str) -> Vec<Poly> {
    let toks = parse_numbers_and_cmds(d);
    let mut polys: Vec<Poly> = Vec::new();
    let mut cur: Poly = Vec::new();
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut last_ctrl: Option<(f32, f32)> = None;
    let mut cmd = 'M';
    let mut pos = 0;
    let close = |cur: &mut Poly, polys: &mut Vec<Poly>| {
        if cur.len() >= 3 {
            polys.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    while pos < toks.len() {
        if let Tok::Cmd(c) = toks[pos] {
            cmd = c;
            pos += 1;
            if c == 'Z' || c == 'z' {
                close(&mut cur, &mut polys);
                x = sx;
                y = sy;
                last_ctrl = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let Some(v) = read_n(&toks, &mut pos, 2) else { break };
                close(&mut cur, &mut polys);
                x = ox + v[0];
                y = oy + v[1];
                sx = x;
                sy = y;
                cur.push((x, y));
                // pares seguintes de um M sao L
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let Some(v) = read_n(&toks, &mut pos, 2) else { break };
                x = ox + v[0];
                y = oy + v[1];
                cur.push((x, y));
                last_ctrl = None;
            }
            'H' => {
                let Some(v) = read_n(&toks, &mut pos, 1) else { break };
                x = if rel { x + v[0] } else { v[0] };
                cur.push((x, y));
                last_ctrl = None;
            }
            'V' => {
                let Some(v) = read_n(&toks, &mut pos, 1) else { break };
                y = if rel { y + v[0] } else { v[0] };
                cur.push((x, y));
                last_ctrl = None;
            }
            'C' => {
                let Some(v) = read_n(&toks, &mut pos, 6) else { break };
                let p1 = (ox + v[0], oy + v[1]);
                let p2 = (ox + v[2], oy + v[3]);
                let p3 = (ox + v[4], oy + v[5]);
                cubic(&mut cur, (x, y), p1, p2, p3);
                last_ctrl = Some(p2);
                x = p3.0;
                y = p3.1;
            }
            'S' => {
                let Some(v) = read_n(&toks, &mut pos, 4) else { break };
                let p1 = last_ctrl.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy));
                let p2 = (ox + v[0], oy + v[1]);
                let p3 = (ox + v[2], oy + v[3]);
                cubic(&mut cur, (x, y), p1, p2, p3);
                last_ctrl = Some(p2);
                x = p3.0;
                y = p3.1;
            }
            'Q' | 'T' => {
                let (q, p3) = if cmd.to_ascii_uppercase() == 'Q' {
                    let Some(v) = read_n(&toks, &mut pos, 4) else { break };
                    ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]))
                } else {
                    let Some(v) = read_n(&toks, &mut pos, 2) else { break };
                    (last_ctrl.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy)), (ox + v[0], oy + v[1]))
                };
                // quadratica como cubica
                let p1 = (x + 2.0 / 3.0 * (q.0 - x), y + 2.0 / 3.0 * (q.1 - y));
                let p2 = (p3.0 + 2.0 / 3.0 * (q.0 - p3.0), p3.1 + 2.0 / 3.0 * (q.1 - p3.1));
                cubic(&mut cur, (x, y), p1, p2, p3);
                last_ctrl = Some(q);
                x = p3.0;
                y = p3.1;
            }
            'A' => {
                let Some(v) = take_args(&toks, &mut pos, d, true) else { break };
                if v.len() < 7 {
                    break;
                }
                let p1 = (ox + v[5], oy + v[6]);
                arc(&mut cur, (x, y), v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0, p1);
                x = p1.0;
                y = p1.1;
                last_ctrl = None;
            }
            _ => {
                pos += 1;
            }
        }
    }
    close(&mut cur, &mut polys);
    polys
}

/// Regra nonzero: soma das voltas de todas as arestas ao redor do ponto.
fn winding(px: f32, py: f32, polys: &[Poly]) -> i32 {
    let mut w = 0;
    for poly in polys {
        let n = poly.len();
        for i in 0..n {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            if y0 <= py {
                if y1 > py && (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0) > 0.0 {
                    w += 1;
                }
            } else if y1 <= py && (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0) < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

fn mask_texture(ctx: &egui::Context, key: &'static str, paths: &[&str]) -> egui::TextureId {
    let id = egui::Id::new(("golive_svg_icon", key));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex.id();
    }
    let polys: Vec<Poly> = paths.iter().flat_map(|d| flatten(d)).collect();
    const N: usize = 64;
    const SUB: usize = 4;
    let mut px = vec![Color32::TRANSPARENT; N * N];
    for y in 0..N {
        for x in 0..N {
            let mut hits = 0;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SUB as f32) * 24.0 / N as f32;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SUB as f32) * 24.0 / N as f32;
                    if winding(fx, fy, &polys) != 0 {
                        hits += 1;
                    }
                }
            }
            px[y * N + x] = Color32::from_white_alpha((hits * 255 / (SUB * SUB)) as u8);
        }
    }
    let tex = ctx.load_texture(key, egui::ColorImage { size: [N, N], pixels: px }, egui::TextureOptions::LINEAR);
    let tid = tex.id();
    ctx.data_mut(|d| d.insert_temp(id, tex));
    tid
}

/// Desenha o icone `key` (os `paths` do SVG do site) centrado em `c`, com lado `sz`, na cor `col`.
pub fn paint(p: &Painter, key: &'static str, paths: &[&str], c: Pos2, sz: f32, col: Color32) {
    let tex = mask_texture(p.ctx(), key, paths);
    p.image(tex, Rect::from_center_size(c, egui::vec2(sz, sz)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), col);
}

// --- Os icones do site (react-icons/md, mesmo `d`) -----------------------------------

pub const MD_HANDYMAN: &[&str] = &[
    "m21.67 18.17-5.3-5.3h-.99l-2.54 2.54v.99l5.3 5.3c.39.39 1.02.39 1.41 0l2.12-2.12a.996.996 0 0 0 0-1.41",
    "m17.34 10.19 1.41-1.41 2.12 2.12a3 3 0 0 0 0-4.24l-3.54-3.54-1.41 1.41V1.71l-.7-.71-3.54 3.54.71.71h2.83l-1.41 1.41 1.06 1.06-2.89 2.89-4.13-4.13V5.06L4.83 2.04 2 4.87 5.03 7.9h1.41l4.13 4.13-.85.85H7.6l-5.3 5.3a.996.996 0 0 0 0 1.41l2.12 2.12c.39.39 1.02.39 1.41 0l5.3-5.3v-2.12l5.15-5.15z",
];
pub const MD_OUTLINE_DRAW: &[&str] = &[
    "m18.85 10.39 1.06-1.06c.78-.78.78-2.05 0-2.83L18.5 5.09c-.78-.78-2.05-.78-2.83 0l-1.06 1.06zm-4.24 1.42L7.41 19H6v-1.41l7.19-7.19zm-1.42-4.25L4 16.76V21h4.24l9.19-9.19zM19 17.5c0 2.19-2.54 3.5-5 3.5-.55 0-1-.45-1-1s.45-1 1-1c1.54 0 3-.73 3-1.5 0-.47-.48-.87-1.23-1.2l1.48-1.48c1.07.63 1.75 1.47 1.75 2.68M4.58 13.35C3.61 12.79 3 12.06 3 11c0-1.8 1.89-2.63 3.56-3.36C7.59 7.18 9 6.56 9 6c0-.41-.78-1-2-1-1.26 0-1.8.61-1.83.64-.35.41-.98.46-1.4.12a.99.99 0 0 1-.15-1.38C3.73 4.24 4.76 3 7 3s4 1.32 4 3c0 1.87-1.93 2.72-3.64 3.47C6.42 9.88 5 10.5 5 11c0 .31.43.6 1.07.86z",
];
pub const MD_CHECKLIST: &[&str] = &[
    "M22 7h-9v2h9zm0 8h-9v2h9zM5.54 11 2 7.46l1.41-1.41 2.12 2.12 4.24-4.24 1.41 1.41zm0 8L2 15.46l1.41-1.41 2.12 2.12 4.24-4.24 1.41 1.41z",
];
pub const MD_GESTURE: &[&str] = &[
    "M4.59 6.89c.7-.71 1.4-1.35 1.71-1.22.5.2 0 1.03-.3 1.52-.25.42-2.86 3.89-2.86 6.31 0 1.28.48 2.34 1.34 2.98.75.56 1.74.73 2.64.46 1.07-.31 1.95-1.4 3.06-2.77 1.21-1.49 2.83-3.44 4.08-3.44 1.63 0 1.65 1.01 1.76 1.79-3.78.64-5.38 3.67-5.38 5.37s1.44 3.09 3.21 3.09c1.63 0 4.29-1.33 4.69-6.1H21v-2.5h-2.47c-.15-1.65-1.09-4.2-4.03-4.2-2.25 0-4.18 1.91-4.94 2.84-.58.73-2.06 2.48-2.29 2.72-.25.3-.68.84-1.11.84-.45 0-.72-.83-.36-1.92.35-1.09 1.4-2.86 1.85-3.52.78-1.14 1.3-1.92 1.3-3.28C8.95 3.69 7.31 3 6.44 3 5.12 3 3.97 4 3.72 4.25c-.36.36-.66.66-.88.93zm9.29 11.66c-.31 0-.74-.26-.74-.72 0-.6.73-2.2 2.87-2.76-.3 2.69-1.43 3.48-2.13 3.48",
];

pub const MD_OUTLINE_MAP: &[&str] = &["m20.5 3-.16.03L15 5.1 9 3 3.36 4.9c-.21.07-.36.25-.36.48V20.5c0 .28.22.5.5.5l.16-.03L9 18.9l6 2.1 5.64-1.9c.21-.07.36-.25.36-.48V3.5c0-.28-.22-.5-.5-.5M10 5.47l4 1.4v11.66l-4-1.4zm-5 .99 3-1.01v11.7l-3 1.16zm14 11.08-3 1.01V6.86l3-1.16z"];
pub const MD_LOCK: &[&str] = &["M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2m-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2m3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1s3.1 1.39 3.1 3.1z"];
pub const MD_PALETTE: &[&str] = &["M12 2C6.49 2 2 6.49 2 12s4.49 10 10 10a2.5 2.5 0 0 0 2.5-2.5c0-.61-.23-1.2-.64-1.67a.53.53 0 0 1-.13-.33c0-.28.22-.5.5-.5H16c3.31 0 6-2.69 6-6 0-4.96-4.49-9-10-9m5.5 11c-.83 0-1.5-.67-1.5-1.5s.67-1.5 1.5-1.5 1.5.67 1.5 1.5-.67 1.5-1.5 1.5m-3-4c-.83 0-1.5-.67-1.5-1.5S13.67 6 14.5 6s1.5.67 1.5 1.5S15.33 9 14.5 9M5 11.5c0-.83.67-1.5 1.5-1.5s1.5.67 1.5 1.5S7.33 13 6.5 13 5 12.33 5 11.5m6-4c0 .83-.67 1.5-1.5 1.5S8 8.33 8 7.5 8.67 6 9.5 6s1.5.67 1.5 1.5"];
pub const MD_EXPAND_MORE: &[&str] = &["M16.59 8.59 12 13.17 7.41 8.59 6 10l6 6 6-6z"];
pub const MD_CHEVRON_RIGHT: &[&str] = &["M10 6 8.59 7.41 13.17 12l-4.58 4.59L10 18l6-6z"];
pub const MD_PERSON_ADD: &[&str] = &["M15 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4m-9-2V7H4v3H1v2h3v3h2v-3h3v-2zm9 4c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_lines_curves_and_arcs() {
        let polys = flatten("M2 2h20v20H2z");
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].len(), 4);
        assert_ne!(winding(12.0, 12.0, &polys), 0);
        assert_eq!(winding(1.0, 1.0, &polys), 0);
        // O cabo da chave do MdHandyman tem um arco e uma curva.
        let polys = flatten(MD_HANDYMAN[0]);
        assert_eq!(polys.len(), 1);
        assert!(polys[0].len() > 8);
        assert_ne!(winding(17.0, 17.0, &polys), 0, "inside the handle");
        // Subcaminhos separados (MdChecklist: 4 pecas).
        assert_eq!(flatten(MD_CHECKLIST[0]).len(), 4);
    }
}
