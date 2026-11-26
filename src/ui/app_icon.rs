//! Runtime window icon (`ViewportBuilder::with_icon`) and Home brand badge texture.

use super::icon_raster::render_golive_lite_icon_rgba;

/// Builds the 64x64 RGBA `egui::IconData` for the Windows titlebar and taskbar.
pub fn build_window_icon() -> egui::IconData {
    let size = 64u32;
    egui::IconData {
        rgba: render_golive_lite_icon_rgba(size),
        width: size,
        height: size,
    }
}

/// Uploads the GoLive Lite brand icon as a GPU texture for the Home header.
pub fn load_brand_icon_texture(ctx: &egui::Context, size: u32) -> egui::TextureHandle {
    let rgba = render_golive_lite_icon_rgba(size);
    let image = egui::ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba);
    ctx.load_texture("golive_lite_brand_icon", image, egui::TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::render_golive_lite_icon_rgba;

    #[test]
    fn test_render_golive_lite_icon_rgba_valid_dimensions_and_colors() {
        let rgba = render_golive_lite_icon_rgba(64);
        assert_eq!(rgba.len(), 64 * 64 * 4);

        // Corner pixels should be transparent outside the rounded icon mask.
        assert_eq!(rgba[3], 0);

        // Center of the monitor screen sits on the white lightning bolt.
        let bolt = (29 * 64 + 32) * 4;
        assert_eq!(rgba[bolt + 3], 255);
        assert!(rgba[bolt] > 230 && rgba[bolt + 1] > 230 && rgba[bolt + 2] > 230);

        // Inner screen off-center keeps exact official #0094FF GoLive blue (unchanged).
        let inner_bg = (29 * 64 + 21) * 4;
        assert_eq!(rgba[inner_bg + 3], 255);
        assert!(rgba[inner_bg] < 20);
        assert!(rgba[inner_bg + 1] >= 120 && rgba[inner_bg + 1] <= 170);
        assert!(rgba[inner_bg + 2] >= 240);

        // Outer icon background preserves exact official #0094FF GoLive blue.
        let bg = (10 * 64 + 10) * 4;
        assert_eq!(rgba[bg + 3], 255);
        assert_eq!(rgba[bg], 0);
        assert!((rgba[bg + 1] as i16 - 148).abs() <= 2);
        assert_eq!(rgba[bg + 2], 255);
    }
}
