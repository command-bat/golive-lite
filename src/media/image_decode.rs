//! Native Windows WIC (Windows Imaging Component) image decoder.
//! Decodes PNG, JPEG, GIF, BMP, ICO, and WebP bytes to RGBA8 without external crates.

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows::core::Interface;

struct ComGuard {
    should_uninit: bool,
}

impl ComGuard {
    fn enter() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // S_OK (0) or S_FALSE (1) require balanced CoUninitialize; RPC_E_CHANGED_MODE does not.
        Self {
            should_uninit: hr.0 == 0 || hr.0 == 1,
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe { CoUninitialize() };
        }
    }
}

/// Decodes encoded image bytes into RGBA8 pixels, scaling down to fit `(max_w, max_h)`.
pub fn decode_image_to_rgba(
    bytes: &[u8],
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.is_empty() {
        return Err("imagem vazia".to_string());
    }
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("imagem excede 16 MB".to_string());
    }

    let _com = ComGuard::enter();

    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("WIC CoCreateInstance falhou: {e}"))?;

        let stream = factory
            .CreateStream()
            .map_err(|e| format!("WIC CreateStream falhou: {e}"))?;
        stream
            .InitializeFromMemory(bytes)
            .map_err(|e| format!("WIC InitializeFromMemory falhou: {e}"))?;

        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(|e| format!("WIC CreateDecoderFromStream falhou: {e}"))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| format!("WIC GetFrame falhou: {e}"))?;

        let mut orig_w = 0u32;
        let mut orig_h = 0u32;
        frame
            .GetSize(&mut orig_w, &mut orig_h)
            .map_err(|e| format!("WIC GetSize falhou: {e}"))?;

        if orig_w == 0 || orig_h == 0 || orig_w > 8192 || orig_h > 8192 {
            return Err(format!("dimensoes de imagem invalidas: {orig_w}x{orig_h}"));
        }

        let converter = factory
            .CreateFormatConverter()
            .map_err(|e| format!("WIC CreateFormatConverter falhou: {e}"))?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| format!("WIC FormatConverter Initialize falhou: {e}"))?;

        let max_w = max_width.max(1);
        let max_h = max_height.max(1);
        let (target_w, target_h, source): (u32, u32, IWICBitmapSource) =
            if orig_w <= max_w && orig_h <= max_h {
                (
                    orig_w,
                    orig_h,
                    converter
                        .cast()
                        .map_err(|e| format!("WIC cast converter falhou: {e}"))?,
                )
            } else {
                let scale = (max_w as f64 / orig_w as f64).min(max_h as f64 / orig_h as f64);
                let tw = ((orig_w as f64 * scale).round() as u32).max(1);
                let th = ((orig_h as f64 * scale).round() as u32).max(1);
                let scaler = factory
                    .CreateBitmapScaler()
                    .map_err(|e| format!("WIC CreateBitmapScaler falhou: {e}"))?;
                scaler
                    .Initialize(&converter, tw, th, WICBitmapInterpolationModeFant)
                    .map_err(|e| format!("WIC BitmapScaler Initialize falhou: {e}"))?;
                (
                    tw,
                    th,
                    scaler
                        .cast()
                        .map_err(|e| format!("WIC cast scaler falhou: {e}"))?,
                )
            };

        let stride = target_w
            .checked_mul(4)
            .ok_or_else(|| "stride overflow".to_string())?;
        let total_bytes = (stride as usize)
            .checked_mul(target_h as usize)
            .ok_or_else(|| "buffer overflow".to_string())?;
        let mut rgba = vec![0u8; total_bytes];
        source
            .CopyPixels(std::ptr::null(), stride, &mut rgba)
            .map_err(|e| format!("WIC CopyPixels falhou: {e}"))?;

        Ok((target_w, target_h, rgba))
    }
}

/// Faz center-crop quadrado e aplica mascara circular suave (anti-aliased) nos pixels RGBA8,
/// garantindo que fotos de perfil nunca vazem fora de molduras redondas.
pub fn crop_and_mask_circle_rgba(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    if width == 0 || height == 0 || rgba.len() < (width as usize) * (height as usize) * 4 {
        return (width, height, rgba.to_vec());
    }
    let side = width.min(height);
    let ox = (width - side) / 2;
    let oy = (height - side) / 2;
    let mut out = vec![0u8; (side as usize) * (side as usize) * 4];
    let center = (side as f32) * 0.5;
    let radius = (side as f32) * 0.5 - 0.35;
    let feather = 1.15_f32;

    for y in 0..side {
        let dy = (y as f32 + 0.5) - center;
        let sy = (oy + y) as usize;
        for x in 0..side {
            let dx = (x as f32 + 0.5) - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let si = (sy * width as usize + (ox + x) as usize) * 4;
            let di = (y as usize * side as usize + x as usize) * 4;
            if dist >= radius {
                out[di] = 0;
                out[di + 1] = 0;
                out[di + 2] = 0;
                out[di + 3] = 0;
            } else if dist > radius - feather {
                let cov = ((radius - dist) / feather).clamp(0.0, 1.0);
                out[di] = rgba[si];
                out[di + 1] = rgba[si + 1];
                out[di + 2] = rgba[si + 2];
                out[di + 3] = ((rgba[si + 3] as f32) * cov).round() as u8;
            } else {
                out[di] = rgba[si];
                out[di + 1] = rgba[si + 1];
                out[di + 2] = rgba[si + 2];
                out[di + 3] = rgba[si + 3];
            }
        }
    }
    (side, side, out)
}

/// Redimensiona uma imagem RGBA8 com filtragem bilinear se exceder `max_dim`, mantendo proporcao.
pub fn downscale_rgba_if_needed(width: u32, height: u32, rgba: Vec<u8>, max_dim: u32) -> (u32, u32, Vec<u8>) {
    if width == 0 || height == 0 || (width <= max_dim && height <= max_dim) {
        return (width, height, rgba);
    }
    let scale = (max_dim as f32 / width as f32).min(max_dim as f32 / height as f32);
    let nw = ((width as f32 * scale).round() as u32).max(1);
    let nh = ((height as f32 * scale).round() as u32).max(1);
    let mut out = vec![0u8; (nw as usize) * (nh as usize) * 4];
    for dy in 0..nh {
        let sy0 = ((dy as usize) * (height as usize)) / (nh as usize);
        let sy1 = (((dy as usize + 1) * (height as usize)) / (nh as usize)).max(sy0 + 1).min(height as usize);
        for dx in 0..nw {
            let sx0 = ((dx as usize) * (width as usize)) / (nw as usize);
            let sx1 = (((dx as usize + 1) * (width as usize)) / (nw as usize)).max(sx0 + 1).min(width as usize);
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut a_sum = 0u32;
            let mut count = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let si = (sy * width as usize + sx) * 4;
                    if si + 3 < rgba.len() {
                        r_sum += rgba[si] as u32;
                        g_sum += rgba[si + 1] as u32;
                        b_sum += rgba[si + 2] as u32;
                        a_sum += rgba[si + 3] as u32;
                        count += 1;
                    }
                }
            }
            let di = (dy as usize * nw as usize + dx as usize) * 4;
            if count > 0 {
                out[di] = (r_sum / count) as u8;
                out[di + 1] = (g_sum / count) as u8;
                out[di + 2] = (b_sum / count) as u8;
                out[di + 3] = (a_sum / count) as u8;
            }
        }
    }
    (nw, nh, out)
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn write_png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(tag);
    out.extend_from_slice(payload);
    let crc = crc32_ieee(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Codifica RGBA8 em PNG valido sem dependencias externas (blocos zlib stored).
pub fn encode_rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("dimensoes invalidas".to_string());
    }
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() < expected {
        return Err("buffer RGBA menor que esperado".to_string());
    }
    let row_len = (width as usize) * 4 + 1;
    let mut raw = Vec::with_capacity(row_len * height as usize);
    for y in 0..height as usize {
        raw.push(0); // filter type 0 (None)
        let s = y * (width as usize) * 4;
        raw.extend_from_slice(&rgba[s..s + (width as usize) * 4]);
    }

    // Wrap raw in a valid zlib stream using uncompressed DEFLATE blocks (<= 65535 B each)
    let mut zlib = Vec::with_capacity(raw.len() + 64);
    zlib.push(0x78); // CMF: deflate 32K window
    zlib.push(0x01); // FLG: no preset dict, FCHECK satisfies (0x78*256 + 0x01) % 31 == 0
    let mut pos = 0usize;
    while pos < raw.len() {
        let rem = raw.len() - pos;
        let chunk_len = rem.min(65535);
        let is_final = pos + chunk_len >= raw.len();
        zlib.push(if is_final { 1 } else { 0 });
        let len16 = chunk_len as u16;
        zlib.extend_from_slice(&len16.to_le_bytes());
        zlib.extend_from_slice(&(!len16).to_le_bytes());
        zlib.extend_from_slice(&raw[pos..pos + chunk_len]);
        pos += chunk_len;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // RGBA
    write_png_chunk(&mut png, b"IHDR", &ihdr);
    write_png_chunk(&mut png, b"IDAT", &zlib);
    write_png_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(hWndNewOwner: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn GetClipboardData(uFormat: u32) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalLock(hMem: isize) -> *const u8;
    fn GlobalUnlock(hMem: isize) -> i32;
    fn GlobalSize(hMem: isize) -> usize;
}

/// Le uma imagem da area de transferencia do Windows (`CF_DIB`) em RGBA8, redimensionando se > 640 px.
pub fn read_clipboard_image_rgba() -> Option<(u32, u32, Vec<u8>)> {
    const CF_DIB: u32 = 8;
    unsafe {
        if OpenClipboard(0) == 0 {
            return None;
        }
        let h_mem = GetClipboardData(CF_DIB);
        if h_mem == 0 {
            CloseClipboard();
            return None;
        }
        let size = GlobalSize(h_mem);
        let ptr = GlobalLock(h_mem);
        if ptr.is_null() || size < 40 {
            if !ptr.is_null() {
                GlobalUnlock(h_mem);
            }
            CloseClipboard();
            return None;
        }
        let dib = std::slice::from_raw_parts(ptr, size).to_vec();
        GlobalUnlock(h_mem);
        CloseClipboard();

        let hdr_size = u32::from_le_bytes(dib[0..4].try_into().ok()?) as usize;
        if hdr_size < 40 || dib.len() < hdr_size {
            return None;
        }
        let width_i = i32::from_le_bytes(dib[4..8].try_into().ok()?);
        let height_i = i32::from_le_bytes(dib[8..12].try_into().ok()?);
        let bpp = u16::from_le_bytes(dib[14..16].try_into().ok()?);
        let comp = u32::from_le_bytes(dib[16..20].try_into().ok()?);
        if width_i <= 0 || height_i == 0 || width_i > 8192 || height_i.abs() > 8192 {
            return None;
        }
        // Construct a full BMP file in memory so WIC handles BI_RGB, BI_BITFIELDS, palettes, top-down & bottom-up uniformly!
        let mut mask_extra = 0usize;
        if comp == 3 && hdr_size == 40 {
            mask_extra = 12; // 3 DWORD bitmasks following BITMAPINFOHEADER
        }
        let clr_used = u32::from_le_bytes(dib[32..36].try_into().ok()?) as usize;
        let palette_bytes = if bpp <= 8 {
            let entries = if clr_used > 0 { clr_used } else { 1usize << bpp };
            entries * 4
        } else {
            0
        };
        let pixel_off = 14 + hdr_size + mask_extra + palette_bytes;
        let file_size = 14 + dib.len();
        let mut bmp = Vec::with_capacity(file_size);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&(pixel_off as u32).to_le_bytes());
        bmp.extend_from_slice(&dib);
        decode_image_to_rgba(&bmp, 640, 640).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{crop_and_mask_circle_rgba, decode_image_to_rgba, encode_rgba_to_png};

    #[test]
    fn test_crop_and_mask_circle_rgba_clears_corners() {
        let (w, h) = (32u32, 24u32);
        let rgba = vec![255u8; (w * h * 4) as usize];
        let (cw, ch, out) = crop_and_mask_circle_rgba(w, h, &rgba);
        assert_eq!((cw, ch), (24, 24));
        // Cantos completamente transparentes (nao vazam fora da moldura redonda)
        assert_eq!(out[3], 0);
        assert_eq!(out[((24 - 1) * 4) + 3], 0);
        assert_eq!(out[((23 * 24) * 4) + 3], 0);
        // Centro preservado opaco
        let mid = ((12 * 24 + 12) * 4) + 3;
        assert_eq!(out[mid], 255);
    }

    #[test]
    fn test_wic_decodes_minimal_bmp_to_rgba() {
        // 2x2 uncompressed 32-bit BI_RGB BMP (bottom-up scanlines: bottom row first, top row second)
        let mut bmp = Vec::with_capacity(54 + 16);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(70u32).to_le_bytes()); // file size
        bmp.extend_from_slice(&0u32.to_le_bytes()); // reserved
        bmp.extend_from_slice(&(54u32).to_le_bytes()); // pixel offset
        bmp.extend_from_slice(&(40u32).to_le_bytes()); // DIB header size
        bmp.extend_from_slice(&(2i32).to_le_bytes()); // width
        bmp.extend_from_slice(&(2i32).to_le_bytes()); // height (positive = bottom-up)
        bmp.extend_from_slice(&(1u16).to_le_bytes()); // planes
        bmp.extend_from_slice(&(32u16).to_le_bytes()); // bpp
        bmp.extend_from_slice(&(0u32).to_le_bytes()); // BI_RGB
        bmp.extend_from_slice(&(16u32).to_le_bytes()); // image size
        bmp.extend_from_slice(&[0u8; 16]); // resolution & palette counts

        // Bottom row (y=1): blue, white
        bmp.extend_from_slice(&[255, 0, 0, 255]);
        bmp.extend_from_slice(&[255, 255, 255, 255]);
        // Top row (y=0): red, green
        bmp.extend_from_slice(&[0, 0, 255, 255]);
        bmp.extend_from_slice(&[0, 255, 0, 255]);

        let (w, h, rgba) = decode_image_to_rgba(&bmp, 64, 64).expect("WIC decode must succeed");
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba.len(), 16);
        // Top-left pixel = red
        assert_eq!(&rgba[0..3], &[255, 0, 0]);
        assert_eq!(rgba[3], 255);
        // Top-right pixel = green
        assert_eq!(&rgba[4..7], &[0, 255, 0]);
        // Bottom-left pixel = blue
        assert_eq!(&rgba[8..11], &[0, 0, 255]);
    }

    #[test]
    fn test_encode_png_roundtrip_via_wic() {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255,
            0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let png = encode_rgba_to_png(2, 2, &rgba).expect("PNG encode");
        let (w, h, decoded) = decode_image_to_rgba(&png, 64, 64).expect("WIC PNG decode");
        assert_eq!((w, h), (2, 2));
        assert_eq!(decoded, rgba);
    }
}
