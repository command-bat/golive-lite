//! Pure-Rust rasterizer for the GoLive Lite application icon.
//! Shared between `build.rs`, `installer/build.rs` (Win32 PE `.res`) and runtime `eframe`/brand UI.

/// Renders a square `size x size` RGBA8 GoLive Lite icon with 3x3 supersampling.
pub fn render_golive_lite_icon_rgba(size: u32) -> Vec<u8> {
    let size = size.max(1);
    let mut out = vec![0u8; (size * size * 4) as usize];
    let inv_size = 512.0 / size as f32;

    // Exact official GoLive icon geometry (`resources/ios/icon.svg`, 512x512):
    // same #0094FF background and same monitor/screen dimensions, with a crisp
    // white lightning bolt placed dead-center inside the screen cutout.
    let bolt_poly: [(f32, f32); 6] = [
        (268.0, 167.0),
        (215.0, 240.0),
        (257.0, 240.0),
        (245.0, 296.0),
        (298.0, 223.0),
        (256.0, 223.0),
    ];

    const SUB: u32 = 3;
    const INV_SUB: f32 = 1.0 / (SUB as f32);
    const INV_SAMPLES: f32 = 1.0 / ((SUB * SUB) as f32);

    for y in 0..size {
        for x in 0..size {
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut acc_a = 0.0f32;

            for sy in 0..SUB {
                for sx in 0..SUB {
                    let px = (x as f32 + (sx as f32 + 0.5) * INV_SUB) * inv_size;
                    let py = (y as f32 + (sy as f32 + 0.5) * INV_SUB) * inv_size;
                    let (r, g, b, a) = sample_icon_pixel(px, py, &bolt_poly);
                    acc_r += r * a;
                    acc_g += g * a;
                    acc_b += b * a;
                    acc_a += a;
                }
            }

            let a = (acc_a * INV_SAMPLES).clamp(0.0, 1.0);
            let idx = ((y * size + x) * 4) as usize;
            if a > 1e-4 {
                out[idx] = ((acc_r / acc_a).clamp(0.0, 1.0) * 255.0).round() as u8;
                out[idx + 1] = ((acc_g / acc_a).clamp(0.0, 1.0) * 255.0).round() as u8;
                out[idx + 2] = ((acc_b / acc_a).clamp(0.0, 1.0) * 255.0).round() as u8;
                out[idx + 3] = (a * 255.0).round() as u8;
            }
        }
    }

    out
}

#[inline]
fn sd_rounded_box(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5 - r;
    let hy = (y1 - y0) * 0.5 - r;
    let dx = (px - cx).abs() - hx;
    let dy = (py - cy).abs() - hy;
    let ox = dx.max(0.0);
    let oy = dy.max(0.0);
    (ox * ox + oy * oy).sqrt() + dx.max(dy).min(0.0) - r
}

#[inline]
fn point_in_poly(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        let intersect = ((yi > py) != (yj > py))
            && (px < (xj - xi) * (py - yi) / ((yj - yi) + 1e-6) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[inline]
fn in_stand_shape(px: f32, py: f32) -> bool {
    if !(360.0..=399.0).contains(&py) {
        return false;
    }
    if py <= 370.0 {
        (217.0..=295.0).contains(&px)
    } else {
        let u = (py - 370.0) / 29.0;
        let left = 217.0 - u * 14.0;
        let right = 295.0 + u * 14.0;
        px >= left && px <= right
    }
}

#[inline]
fn sample_icon_pixel(px: f32, py: f32, bolt_poly: &[(f32, f32); 6]) -> (f32, f32, f32, f32) {
    // Rounded tile silhouette preserving full 512x512 SVG scale
    let bg_sd = sd_rounded_box(px, py, 16.0, 16.0, 496.0, 496.0, 108.0);
    if bg_sd > 1.5 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    // Official solid GoLive blue background (#0094FF)
    let mut r = 0.0f32;
    let mut g = 148.0f32 / 255.0;
    let mut b = 1.0f32;

    // Official feDropShadow(dx=6, dy=8, stdDeviation=10, flood-opacity=0.16)
    let sx = px - 6.0;
    let sy = py - 8.0;
    let sh_outer = sd_rounded_box(sx, sy, 105.0, 113.0, 408.0, 362.0, 16.0);
    let sh_inner = sd_rounded_box(sx, sy, 136.0, 145.0, 377.0, 318.0, 0.0);
    let sh_bezel = sh_outer.max(-sh_inner);
    let sh_stand = if in_stand_shape(sx, sy) { -1.0 } else { 20.0 };
    let sh_bolt = if point_in_poly(sx, sy, bolt_poly) { -1.0 } else { 20.0 };
    let shadow_dist = sh_bezel.min(sh_stand).min(sh_bolt);
    if shadow_dist < 14.0 {
        let shadow_factor = (1.0 - (shadow_dist.max(0.0) / 14.0)) * 0.16;
        r *= 1.0 - shadow_factor;
        g *= 1.0 - shadow_factor;
        b *= 1.0 - shadow_factor;
    }

    // Official monitor frame [105..408, 113..362] with exact screen cutout [136..377, 145..318]
    let outer_sd = sd_rounded_box(px, py, 105.0, 113.0, 408.0, 362.0, 16.0);
    let in_cutout = (136.0..=377.0).contains(&px) && (145.0..=318.0).contains(&py);
    let in_bezel = outer_sd <= 0.0 && !in_cutout;

    if in_bezel || in_stand_shape(px, py) || point_in_poly(px, py, bolt_poly) {
        return (1.0, 1.0, 1.0, 1.0);
    }

    let edge_alpha = if bg_sd <= 0.0 {
        1.0
    } else {
        (1.0 - bg_sd / 1.5).clamp(0.0, 1.0)
    };
    (r, g, b, edge_alpha)
}
