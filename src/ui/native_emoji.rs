//! Native Windows color emoji rasterizer (DirectWrite + Direct2D `ENABLE_COLOR_FONT` over WIC).
//! Rasterizes only the requested emojis on demand into tiny 48x48 RGBA textures (~9 KB each),
//! avoiding loading a 15-20 MB emoji font into `egui`'s CPU/GPU font atlas.

use std::cell::RefCell;
use std::collections::HashMap;

const EMOJI_TEX_SIZE: u32 = 48;

thread_local! {
    static EMOJI_CACHE: RefCell<HashMap<String, Option<egui::TextureHandle>>> =
        RefCell::new(HashMap::new());
}

pub fn clear_cache() {
    EMOJI_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        map.clear();
        map.shrink_to_fit();
    });
}

pub fn get_or_load_emoji_texture(
    ctx: &egui::Context,
    emoji: &str,
) -> Option<egui::TextureId> {
    let key = emoji.trim();
    if key.is_empty() {
        return None;
    }
    EMOJI_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if let Some(existing) = map.get(key) {
            return existing.as_ref().map(|h| h.id());
        }
        let rgba = rasterize_windows_emoji_rgba(key, EMOJI_TEX_SIZE);
        let handle = rgba.map(|pixels| {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [EMOJI_TEX_SIZE as usize, EMOJI_TEX_SIZE as usize],
                &pixels,
            );
            ctx.load_texture(
                format!("golive_emoji_{}", key),
                img,
                egui::TextureOptions::LINEAR,
            )
        });
        let id = handle.as_ref().map(|h| h.id());
        if map.len() >= 48 {
            map.clear();
        }
        map.insert(key.to_string(), handle);
        id
    })
}

pub fn paint_native_emoji(
    ui: &egui::Ui,
    rect: egui::Rect,
    emoji: &str,
    tint: egui::Color32,
    fallback_font_size: f32,
) {
    if let Some(tid) = get_or_load_emoji_texture(ui.ctx(), emoji) {
        ui.painter().image(
            tid,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            emoji,
            egui::FontId::proportional(fallback_font_size),
            tint,
        );
    }
}

#[cfg(windows)]
pub fn rasterize_windows_emoji_rgba(emoji: &str, size: u32) -> Option<Vec<u8>> {
    use windows::core::{w, HSTRING};
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1CreateFactory, ID2D1Factory, D2D1_DRAW_TEXT_OPTIONS, D2D1_DRAW_TEXT_OPTIONS_NONE,
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
        D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    };
    use windows::Win32::Graphics::DirectWrite::{
        DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapCacheOnLoad,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };

    const D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT_FLAG: D2D1_DRAW_TEXT_OPTIONS =
        D2D1_DRAW_TEXT_OPTIONS(0x0000_0004);

    if emoji.trim().is_empty() || size == 0 {
        return None;
    }

    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let need_uninit = hr.0 == 0 || hr.0 == 1;

        let res = (|| -> Option<Vec<u8>> {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let bitmap = wic
                .CreateBitmap(
                    size,
                    size,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnLoad,
                )
                .ok()?;

            let d2d_factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = d2d_factory
                .CreateWicBitmapRenderTarget(&bitmap, &rt_props)
                .ok()?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dwrite: IDWriteFactory =
                DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let font_size = (size as f32) * 0.68;
            let text_format = dwrite
                .CreateTextFormat(
                    w!("Segoe UI Emoji"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font_size,
                    w!("en-us"),
                )
                .ok()?;
            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let brush_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            };
            let brush = rt.CreateSolidColorBrush(&brush_col, None).ok()?;
            let wide_u16: Vec<u16> = HSTRING::from(emoji).as_wide().to_vec();
            let layout_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: size as f32,
                bottom: size as f32,
            };

            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            rt.DrawText(
                &wide_u16,
                &text_format,
                &layout_rect,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT_FLAG,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            if rt.EndDraw(None, None).is_err() {
                // Retry without color-font flag for strict Windows 8.0 compatibility
                rt.BeginDraw();
                rt.Clear(Some(&D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }));
                rt.DrawText(
                    &wide_u16,
                    &text_format,
                    &layout_rect,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                rt.EndDraw(None, None).ok()?;
            }

            let stride = size * 4;
            let mut pbgra = vec![0u8; (stride * size) as usize];
            bitmap.CopyPixels(std::ptr::null(), stride, &mut pbgra).ok()?;

            // Un-premultiply PBGRA -> straight RGBA8
            let mut has_visible = false;
            let mut rgba = vec![0u8; pbgra.len()];
            for i in (0..pbgra.len()).step_by(4) {
                let b = pbgra[i];
                let g = pbgra[i + 1];
                let r = pbgra[i + 2];
                let a = pbgra[i + 3];
                if a > 0 {
                    has_visible = true;
                    if a == 255 {
                        rgba[i] = r;
                        rgba[i + 1] = g;
                        rgba[i + 2] = b;
                        rgba[i + 3] = 255;
                    } else {
                        let af = a as f32 / 255.0;
                        rgba[i] = ((r as f32 / af).round()).clamp(0.0, 255.0) as u8;
                        rgba[i + 1] = ((g as f32 / af).round()).clamp(0.0, 255.0) as u8;
                        rgba[i + 2] = ((b as f32 / af).round()).clamp(0.0, 255.0) as u8;
                        rgba[i + 3] = a;
                    }
                }
            }
            if has_visible {
                Some(rgba)
            } else {
                None
            }
        })();

        if need_uninit {
            CoUninitialize();
        }
        res
    }
}

#[cfg(not(windows))]
pub fn rasterize_windows_emoji_rgba(_emoji: &str, _size: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::rasterize_windows_emoji_rgba;

    #[test]
    fn test_windows_native_emoji_rasterizes_color_pixels() {
        let rgba = rasterize_windows_emoji_rgba("👍", 48).expect("Windows Direct2D emoji raster");
        assert_eq!(rgba.len(), 48 * 48 * 4);
        let non_transparent = rgba.chunks_exact(4).filter(|px| px[3] > 20).count();
        assert!(non_transparent > 50, "expected visible emoji pixels");
    }
}
