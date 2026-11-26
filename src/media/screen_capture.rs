use std::mem::size_of;
use std::ptr::null_mut;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    BOOL as WIN_BOOL, CloseHandle, HWND, LPARAM as WIN_LPARAM, RECT as WIN_RECT,
};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS,
};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE,
    GW_OWNER, WS_EX_TOOLWINDOW,
};
use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HMONITOR,
    MONITORINFOEXW, SRCCOPY,
};

#[link(name = "user32")]
extern "system" {
    fn PrintWindow(hwnd: isize, hdc_blt: HDC, n_flags: u32) -> BOOL;
}

const PW_RENDERFULLCONTENT: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorTarget {
    pub index: usize,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
    pub refresh_hz: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTarget {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    pub process_name: String,
    pub width: i32,
    pub height: i32,
}

impl WindowTarget {
    pub fn is_alive(&self) -> bool {
        let hwnd = HWND(self.hwnd);
        unsafe { IsWindow(hwnd).as_bool() }
    }

    pub fn display_label(&self) -> String {
        let short_title = truncate_label(&self.title, 44);
        if self.process_name.is_empty()
            || short_title
                .to_ascii_lowercase()
                .contains(&self.process_name.to_ascii_lowercase())
        {
            format!("{} - {}x{}", short_title, self.width, self.height)
        } else {
            format!(
                "{} ({}) - {}x{}",
                short_title, self.process_name, self.width, self.height
            )
        }
    }
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in s.chars() {
        if count >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

impl MonitorTarget {
    pub fn display_label(&self) -> String {
        let hz_suffix = if self.refresh_hz > 0 {
            format!(" @ {}Hz", self.refresh_hz)
        } else {
            String::new()
        };
        if self.is_primary {
            format!(
                "Tela {} (Principal) - {}x{}{}",
                self.index + 1,
                self.width,
                self.height,
                hz_suffix
            )
        } else {
            format!(
                "Tela {} - {}x{}{}",
                self.index + 1,
                self.width,
                self.height,
                hz_suffix
            )
        }
    }
}

/// Calcula a resolucao exata de saida respeitando a configuracao escolhida (ate 3840x2160 4K),
/// preservando a proporcao do monitor e garantindo lados pares sem reduzir pelo tamanho do monitor.
pub fn compute_target_capture_dimensions(
    mon_w: u32,
    mon_h: u32,
    cfg_w: u32,
    cfg_h: u32,
) -> (u32, u32) {
    let mw = mon_w.max(16) as u64;
    let mh = mon_h.max(16) as u64;
    let max_w = (cfg_w.clamp(16, 3840) & !1).max(16);
    let max_h = (cfg_h.clamp(16, 2160) & !1).max(16);

    let (tw, th) = if mw * (max_h as u64) >= mh * (max_w as u64) {
        let h = ((mh * (max_w as u64) + (mw / 2)) / mw) as u32;
        (max_w, h)
    } else {
        let w = ((mw * (max_h as u64) + (mh / 2)) / mh) as u32;
        (w, max_h)
    };

    let even_w = (tw & !1).clamp(16, max_w);
    let even_h = (th & !1).clamp(16, max_h);
    (even_w, even_h)
}

/// Calcula dimensoes leves de thumbnail para o preview local da UI sem sobrecarregar CPU/GPU.
pub fn compute_preview_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let sw = src_w.max(16);
    let sh = src_h.max(16);
    if sw <= max_w && sh <= max_h {
        return ((sw & !1).max(16), (sh & !1).max(16));
    }
    compute_target_capture_dimensions(sw, sh, max_w, max_h)
}

/// Converte um quadro BGRA de alta resolucao para thumbnail RGBA nitido usando filtragem de area (box filter).
pub fn make_preview_rgba_from_bgra(
    bgra: &[u8],
    src_w: u32,
    src_h: u32,
    max_w: u32,
    max_h: u32,
) -> Option<(usize, usize, Vec<u8>)> {
    let sw = src_w as usize;
    let sh = src_h as usize;
    if sw < 2 || sh < 2 || bgra.len() < sw * sh * 4 {
        return None;
    }
    let (pw_u32, ph_u32) = compute_preview_dimensions(src_w, src_h, max_w, max_h);
    let pw = pw_u32 as usize;
    let ph = ph_u32 as usize;
    let mut rgba = vec![255u8; pw * ph * 4];

    if pw == sw && ph == sh {
        // Copia direta 1:1 nitida (ex: stream 720p no preview 1280x720)
        for i in 0..(pw * ph) {
            let s = i * 4;
            rgba[s] = bgra[s + 2];
            rgba[s + 1] = bgra[s + 1];
            rgba[s + 2] = bgra[s];
        }
        return Some((pw, ph, rgba));
    }

    let x_ranges: Vec<(usize, usize)> = (0..pw)
        .map(|dx| {
            let x0 = (dx * sw) / pw;
            let x1 = (((dx + 1) * sw) / pw).max(x0 + 1).min(sw);
            (x0, x1)
        })
        .collect();

    for dy in 0..ph {
        let sy0 = (dy * sh) / ph;
        let sy1 = (((dy + 1) * sh) / ph).max(sy0 + 1).min(sh);
        let dst_row = dy * pw * 4;
        for (dx, &(sx0, sx1)) in x_ranges.iter().enumerate() {
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut cnt = 0u32;
            for sy in sy0..sy1 {
                let row_off = sy * sw * 4;
                for sx in sx0..sx1 {
                    let s = row_off + sx * 4;
                    b_sum += bgra[s] as u32;
                    g_sum += bgra[s + 1] as u32;
                    r_sum += bgra[s + 2] as u32;
                    cnt += 1;
                }
            }
            let d = dst_row + dx * 4;
            let c = cnt.max(1);
            rgba[d] = (r_sum / c) as u8;
            rgba[d + 1] = (g_sum / c) as u8;
            rgba[d + 2] = (b_sum / c) as u8;
        }
    }
    Some((pw, ph, rgba))
}

/// Guard RAII para elevar temporariamente a granularidade do timer do Windows a 1 ms (necessario para 60/120/240 fps).
pub struct HighResTimerGuard {
    active: bool,
}

impl HighResTimerGuard {
    pub fn new() -> Self {
        let res = unsafe { timeBeginPeriod(1) };
        Self { active: res == 0 }
    }
}

impl Drop for HighResTimerGuard {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                let _ = timeEndPeriod(1);
            }
        }
    }
}

/// Dorme ate proximo ao `deadline` e conclui com precisao para manter cadencia estavel em 60/120/240 fps.
pub fn sleep_until_precise(deadline: Instant) {
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let rem = deadline - now;
        if rem > Duration::from_micros(2_000) {
            std::thread::sleep(rem - Duration::from_micros(1_500));
        } else {
            while Instant::now() < deadline {
                std::hint::spin_loop();
            }
            break;
        }
    }
}

unsafe extern "system" fn enum_monitor_cb(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = &mut *(lparam as *mut Vec<MonitorTarget>);
    let mut info: MONITORINFOEXW = std::mem::zeroed();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _) != 0 {
        let rc = info.monitorInfo.rcMonitor;
        let mut width = (rc.right - rc.left).max(1);
        let mut height = (rc.bottom - rc.top).max(1);
        let is_primary = (info.monitorInfo.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY
        let raw_name = &info.szDevice;
        let len = raw_name.iter().position(|&c| c == 0).unwrap_or(raw_name.len());
        let name = String::from_utf16_lossy(&raw_name[..len]);

        let mut refresh_hz = 0u32;
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(
            PCWSTR(info.szDevice.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut dm,
        )
        .as_bool()
        {
            refresh_hz = dm.dmDisplayFrequency;
            if dm.dmPelsWidth > width as u32 && dm.dmPelsHeight > height as u32 {
                width = dm.dmPelsWidth as i32;
                height = dm.dmPelsHeight as i32;
            }
        }

        let index = list.len();
        list.push(MonitorTarget {
            index,
            name,
            x: rc.left,
            y: rc.top,
            width,
            height,
            is_primary,
            refresh_hz,
        });
    }
    1
}

pub fn list_monitors() -> Vec<MonitorTarget> {
    let mut monitors: Vec<MonitorTarget> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null_mut(),
            Some(enum_monitor_cb),
            &mut monitors as *mut Vec<MonitorTarget> as LPARAM,
        );
    }
    monitors
}

fn process_basename_from_pid(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok || len == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        let base = full.rsplit(['\\', '/']).next().unwrap_or(&full);
        base.strip_suffix(".exe")
            .or_else(|| base.strip_suffix(".EXE"))
            .unwrap_or(base)
            .to_string()
    }
}

unsafe extern "system" fn enum_window_cb(hwnd: HWND, lparam: WIN_LPARAM) -> WIN_BOOL {
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return WIN_BOOL(1);
    }
    if GetWindow(hwnd, GW_OWNER).0 != 0 {
        return WIN_BOOL(1);
    }
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
        return WIN_BOOL(1);
    }
    let mut cloaked: WIN_BOOL = WIN_BOOL(0);
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut WIN_BOOL as *mut _,
        size_of::<WIN_BOOL>() as u32,
    )
    .is_ok()
        && cloaked.as_bool()
    {
        return WIN_BOOL(1);
    }

    let text_len = GetWindowTextLengthW(hwnd);
    if text_len <= 0 {
        return WIN_BOOL(1);
    }

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == std::process::id() {
        return WIN_BOOL(1);
    }

    let mut rc = WIN_RECT::default();
    let has_bounds = DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rc as *mut WIN_RECT as *mut _,
        size_of::<WIN_RECT>() as u32,
    )
    .is_ok()
        || GetWindowRect(hwnd, &mut rc).is_ok();
    if !has_bounds {
        return WIN_BOOL(1);
    }
    let width = rc.right - rc.left;
    let height = rc.bottom - rc.top;
    if width < 64 || height < 64 {
        return WIN_BOOL(1);
    }

    let mut title_buf = vec![0u16; (text_len as usize) + 2];
    let copied = GetWindowTextW(hwnd, &mut title_buf);
    if copied <= 0 {
        return WIN_BOOL(1);
    }
    let title = String::from_utf16_lossy(&title_buf[..copied as usize])
        .trim()
        .to_string();
    if title.is_empty() {
        return WIN_BOOL(1);
    }

    let process_name = process_basename_from_pid(pid);
    let list = &mut *(lparam.0 as *mut Vec<WindowTarget>);
    list.push(WindowTarget {
        hwnd: hwnd.0,
        pid,
        title,
        process_name,
        width,
        height,
    });
    WIN_BOOL(1)
}

pub fn list_windows() -> Vec<WindowTarget> {
    let mut windows_list: Vec<WindowTarget> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_window_cb),
            WIN_LPARAM(&mut windows_list as *mut Vec<WindowTarget> as isize),
        );
    }
    windows_list
}

/// Capturador Win32 GDI com reutilizacao integral de buffers (fallback quando DXGI/HW encoder nao disponivel).
pub struct GdiScreenCapturer {
    src_dc: HDC,
    mem_dc: HDC,
    bitmap: HBITMAP,
    cap_w: i32,
    cap_h: i32,
    bgra_buf: Vec<u8>,
    x_map: Vec<usize>,
    cached_x_map_dims: (usize, usize),
}

unsafe impl Send for GdiScreenCapturer {}

impl GdiScreenCapturer {
    pub fn new(width: i32, height: i32) -> Result<Self, String> {
        unsafe {
            let src_dc = GetDC(null_mut());
            if src_dc.is_null() {
                return Err("Falha ao obter Device Context da tela.".to_string());
            }
            let mem_dc = CreateCompatibleDC(src_dc);
            if mem_dc.is_null() {
                ReleaseDC(null_mut(), src_dc);
                return Err("Falha ao criar DC compativel.".to_string());
            }
            let bitmap = CreateCompatibleBitmap(src_dc, width, height);
            if bitmap.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(null_mut(), src_dc);
                return Err("Falha ao criar bitmap de captura.".to_string());
            }
            SelectObject(mem_dc, bitmap as _);
            let byte_len = (width as usize) * (height as usize) * 4;
            Ok(Self {
                src_dc,
                mem_dc,
                bitmap,
                cap_w: width,
                cap_h: height,
                bgra_buf: vec![0u8; byte_len],
                x_map: Vec::new(),
                cached_x_map_dims: (0, 0),
            })
        }
    }

    /// Captura a regiao do monitor especificado, escala para `(out_w, out_h)` (inclusive upscale para 4K)
    /// e grava pixels top-down em `out_bgra` sem allocs extras no loop.
    pub fn capture_monitor_bgra_into(
        &mut self,
        monitor: &MonitorTarget,
        out_w: u32,
        out_h: u32,
        out_bgra: &mut Vec<u8>,
    ) -> bool {
        if monitor.width != self.cap_w || monitor.height != self.cap_h {
            if let Ok(rebuilt) = Self::new(monitor.width, monitor.height) {
                *self = rebuilt;
            } else {
                return false;
            }
        }

        unsafe {
            if BitBlt(
                self.mem_dc,
                0,
                0,
                self.cap_w,
                self.cap_h,
                self.src_dc,
                monitor.x,
                monitor.y,
                SRCCOPY,
            ) == 0
            {
                return false;
            }

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.cap_w,
                    biHeight: -self.cap_h, // Top-down para evitar flip vertical
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [std::mem::zeroed()],
            };

            let rows = GetDIBits(
                self.mem_dc,
                self.bitmap,
                0,
                self.cap_h as u32,
                self.bgra_buf.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            );
            if rows == 0 {
                return false;
            }
        }

        self.scale_captured_bgra(out_w, out_h, out_bgra)
    }

    /// Captura o conteudo da janela especificada via Win32 `PrintWindow` (fallback quando WGC nao esta disponivel).
    pub fn capture_window_bgra_into(
        &mut self,
        hwnd: isize,
        out_w: u32,
        out_h: u32,
        out_bgra: &mut Vec<u8>,
    ) -> bool {
        let win_hwnd = HWND(hwnd);
        if unsafe { !IsWindow(win_hwnd).as_bool() || IsIconic(win_hwnd).as_bool() } {
            return false;
        }
        let mut rc = WIN_RECT::default();
        if unsafe { GetWindowRect(win_hwnd, &mut rc) }.is_err() {
            return false;
        }
        let win_w = (rc.right - rc.left).max(16);
        let win_h = (rc.bottom - rc.top).max(16);
        if win_w != self.cap_w || win_h != self.cap_h {
            if let Ok(rebuilt) = Self::new(win_w, win_h) {
                *self = rebuilt;
            } else {
                return false;
            }
        }

        unsafe {
            let ok = PrintWindow(hwnd, self.mem_dc, PW_RENDERFULLCONTENT) != 0
                || PrintWindow(hwnd, self.mem_dc, 0) != 0;
            if !ok {
                return false;
            }

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.cap_w,
                    biHeight: -self.cap_h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [std::mem::zeroed()],
            };

            let rows = GetDIBits(
                self.mem_dc,
                self.bitmap,
                0,
                self.cap_h as u32,
                self.bgra_buf.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            );
            if rows == 0 {
                return false;
            }
        }

        self.scale_captured_bgra(out_w, out_h, out_bgra)
    }

    fn scale_captured_bgra(&mut self, out_w: u32, out_h: u32, out_bgra: &mut Vec<u8>) -> bool {
        let target_w = (out_w & !1).clamp(16, 3840) as usize;
        let target_h = (out_h & !1).clamp(16, 2160) as usize;
        let needed = target_w * target_h * 4;
        if out_bgra.len() != needed {
            out_bgra.resize(needed, 255);
        }

        let sw = self.cap_w as usize;
        let sh = self.cap_h as usize;

        if target_w == sw && target_h == sh {
            out_bgra.copy_from_slice(&self.bgra_buf[..needed]);
            return true;
        }

        if self.cached_x_map_dims != (sw, target_w) {
            self.x_map.resize(target_w, 0);
            for dx in 0..target_w {
                self.x_map[dx] = (dx * sw) / target_w;
            }
            self.cached_x_map_dims = (sw, target_w);
        }

        let src_ptr = self.bgra_buf.as_ptr() as *const u32;
        let dst_ptr = out_bgra.as_mut_ptr() as *mut u32;
        for dy in 0..target_h {
            let sy = (dy * sh) / target_h;
            let src_row_off = sy * sw;
            let dst_row_off = dy * target_w;
            unsafe {
                let s_row = src_ptr.add(src_row_off);
                let d_row = dst_ptr.add(dst_row_off);
                for (dx, &sx) in self.x_map.iter().enumerate() {
                    std::ptr::write_unaligned(d_row.add(dx), std::ptr::read_unaligned(s_row.add(sx)));
                }
            }
        }
        true
    }
}

/// Captura uma miniatura RGBA leve de um monitor (`max_w x max_h`) para preview no seletor de compartilhamento.
pub fn capture_monitor_thumbnail_rgba(
    monitor: &MonitorTarget,
    max_w: u32,
    max_h: u32,
) -> Option<(usize, usize, Vec<u8>)> {
    if monitor.width < 16 || monitor.height < 16 {
        return None;
    }
    let mut cap = GdiScreenCapturer::new(monitor.width, monitor.height).ok()?;
    let mut bgra = Vec::new();
    let (tw, th) = compute_preview_dimensions(monitor.width as u32, monitor.height as u32, max_w, max_h);
    if !cap.capture_monitor_bgra_into(monitor, tw, th, &mut bgra) {
        return None;
    }
    make_preview_rgba_from_bgra(&bgra, tw, th, max_w, max_h)
}

/// Captura uma miniatura RGBA leve de uma janela (`max_w x max_h`) via `PrintWindow` para preview no seletor.
pub fn capture_window_thumbnail_rgba(
    hwnd: isize,
    max_w: u32,
    max_h: u32,
) -> Option<(usize, usize, Vec<u8>)> {
    let win_hwnd = HWND(hwnd);
    if unsafe { !IsWindow(win_hwnd).as_bool() || IsIconic(win_hwnd).as_bool() } {
        return None;
    }
    let mut rc = WIN_RECT::default();
    if unsafe { GetWindowRect(win_hwnd, &mut rc) }.is_err() {
        return None;
    }
    let win_w = (rc.right - rc.left).max(16);
    let win_h = (rc.bottom - rc.top).max(16);
    let mut cap = GdiScreenCapturer::new(win_w, win_h).ok()?;
    let (tw, th) = compute_preview_dimensions(win_w as u32, win_h as u32, max_w, max_h);
    let mut bgra = Vec::new();
    if !cap.capture_window_bgra_into(hwnd, tw, th, &mut bgra) {
        return None;
    }
    make_preview_rgba_from_bgra(&bgra, tw, th, max_w, max_h)
}

impl Drop for GdiScreenCapturer {
    fn drop(&mut self) {
        unsafe {
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap as _);
            }
            if !self.mem_dc.is_null() {
                DeleteDC(self.mem_dc);
            }
            if !self.src_dc.is_null() {
                ReleaseDC(null_mut(), self.src_dc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_target_capture_dimensions_honors_config_and_aspect() {
        // Monitor 1080p com configuracao 2160p 4K deve capturar/emitir em 3840x2160 (sem clampar em 1920x1080)
        assert_eq!(
            compute_target_capture_dimensions(1920, 1080, 3840, 2160),
            (3840, 2160)
        );
        // Monitor 4K nativo com configuracao 4K
        assert_eq!(
            compute_target_capture_dimensions(3840, 2160, 3840, 2160),
            (3840, 2160)
        );
        // Monitor 4K com configuracao 1080p
        assert_eq!(
            compute_target_capture_dimensions(3840, 2160, 1920, 1080),
            (1920, 1080)
        );
        // Ultrawide 21:9 (2560x1080) em configuracao 4K preserva proporcao e lados pares
        let (uw_w, uw_h) = compute_target_capture_dimensions(2560, 1080, 3840, 2160);
        assert_eq!((uw_w, uw_h), (3840, 1620));
        assert_eq!(uw_w % 2, 0);
        assert_eq!(uw_h % 2, 0);

        // Monitor retrato 9:16 em configuracao 4K
        let (pw, ph) = compute_target_capture_dimensions(1080, 1920, 3840, 2160);
        assert_eq!(ph, 2160);
        assert_eq!(pw % 2, 0);
        assert_eq!(ph % 2, 0);
        assert!(pw <= 3840);
    }

    #[test]
    fn test_compute_preview_dimensions_bounds() {
        let (w, h) = compute_preview_dimensions(3840, 2160, 480, 270);
        assert_eq!((w, h), (480, 270));
        let (sw, sh) = compute_preview_dimensions(320, 180, 480, 270);
        assert_eq!((sw, sh), (320, 180));
    }

    #[test]
    fn test_list_windows_returns_valid_entries() {
        let wins = list_windows();
        let own_pid = std::process::id();
        for w in wins {
            assert_ne!(w.hwnd, 0);
            assert_ne!(w.pid, 0);
            assert_ne!(w.pid, own_pid);
            assert!(!w.title.trim().is_empty());
            assert!(w.width >= 64 && w.height >= 64);
        }
    }

    #[test]
    fn test_make_preview_rgba_from_bgra_scales_and_swaps_channels() {
        let (sw, sh) = (320u32, 180u32);
        let mut bgra = vec![0u8; (sw * sh * 4) as usize];
        for px in bgra.chunks_exact_mut(4) {
            px[0] = 10; // B
            px[1] = 20; // G
            px[2] = 30; // R
            px[3] = 255;
        }
        let (pw, ph, rgba) = make_preview_rgba_from_bgra(&bgra, sw, sh, 256, 144).expect("preview");
        assert_eq!((pw, ph), (256, 144));
        assert_eq!(rgba.len(), 256 * 144 * 4);
        assert_eq!(&rgba[0..4], &[30, 20, 10, 255]);
    }

    #[test]
    fn test_make_preview_rgba_box_filters_and_preserves_1to1() {
        let mut bgra = vec![0u8; 4 * 4 * 4];
        for px in bgra.chunks_exact_mut(4) {
            px[0] = 40;
            px[1] = 80;
            px[2] = 120;
            px[3] = 255;
        }
        // Alter first 2x2 block to white and black checkerboard to verify box average
        bgra[0..4].copy_from_slice(&[0, 0, 0, 255]);
        bgra[4..8].copy_from_slice(&[200, 200, 200, 255]);
        bgra[16..20].copy_from_slice(&[200, 200, 200, 255]);
        bgra[20..24].copy_from_slice(&[0, 0, 0, 255]);
        // 1:1 returns exact pixel (src >= 16 triggers no downscale when <= max)
        let mut big = vec![0u8; 16 * 16 * 4];
        big[0..4].copy_from_slice(&[10, 50, 90, 255]);
        let (pw1, ph1, rgba1) = make_preview_rgba_from_bgra(&big, 16, 16, 1280, 720).expect("1:1");
        assert_eq!((pw1, ph1), (16, 16));
        assert_eq!(&rgba1[0..4], &[90, 50, 10, 255]);

        // 32x32 -> 16x16 box filter averages first 2x2 block
        let mut src32 = vec![0u8; 32 * 32 * 4];
        src32[0..4].copy_from_slice(&[0, 0, 0, 255]);
        src32[4..8].copy_from_slice(&[200, 200, 200, 255]);
        src32[32 * 4..32 * 4 + 4].copy_from_slice(&[200, 200, 200, 255]);
        src32[32 * 4 + 4..32 * 4 + 8].copy_from_slice(&[0, 0, 0, 255]);
        let (pw2, ph2, rgba2) = make_preview_rgba_from_bgra(&src32, 32, 32, 16, 16).expect("2x2 box");
        assert_eq!((pw2, ph2), (16, 16));
        assert_eq!(&rgba2[0..4], &[100, 100, 100, 255]);
    }
}
