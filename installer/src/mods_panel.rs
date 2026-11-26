//! A lista de modificacoes da etapa 2 numa area com barra de rolagem: cabe qualquer numero
//! de modificacoes (o catalogo vem da API) sem espremer a janela.
//!
//! E uma janela filha nativa com `WS_VSCROLL`. Os controles ficam dentro dela; rolar move todos
//! juntos (`ScrollWindowEx` + `SW_SCROLLCHILDREN`). Cliques e cores dos controles sao repassados
//! para a janela do instalador, que continua tratando tudo como antes.

use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

static PANEL: AtomicIsize = AtomicIsize::new(0);
static CONTENT_H: AtomicI32 = AtomicI32::new(0);
static POS: AtomicI32 = AtomicI32::new(0);

#[repr(C)]
struct WndClassExW {
    cb_size: u32,
    style: u32,
    lpfn_wnd_proc: unsafe extern "system" fn(isize, u32, usize, isize) -> isize,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: isize,
    h_icon: isize,
    h_cursor: isize,
    hbr_background: isize,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
    h_icon_sm: isize,
}

#[repr(C)]
struct ScrollInfo {
    cb_size: u32,
    f_mask: u32,
    n_min: i32,
    n_max: i32,
    n_page: u32,
    n_pos: i32,
    n_track_pos: i32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(lpwcx: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: isize,
        menu: isize,
        inst: isize,
        param: *const std::ffi::c_void,
    ) -> isize;
    fn DefWindowProcW(h: isize, msg: u32, w: usize, l: isize) -> isize;
    fn SendMessageW(h: isize, msg: u32, w: usize, l: isize) -> isize;
    fn GetParent(h: isize) -> isize;
    fn GetDlgItem(h: isize, id: i32) -> isize;
    fn LoadCursorW(inst: isize, name: *const u16) -> isize;
    fn SetScrollInfo(h: isize, bar: i32, si: *const ScrollInfo, redraw: i32) -> i32;
    fn GetScrollInfo(h: isize, bar: i32, si: *mut ScrollInfo) -> i32;
    fn ScrollWindowEx(
        h: isize,
        dx: i32,
        dy: i32,
        scroll: *const std::ffi::c_void,
        clip: *const std::ffi::c_void,
        rgn: isize,
        update: *mut std::ffi::c_void,
        flags: u32,
    ) -> i32;
}

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const WS_EX_CONTROLPARENT: u32 = 0x0001_0000;
const WM_COMMAND: u32 = 0x0111;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const WM_CTLCOLORBTN: u32 = 0x0135;
const WM_VSCROLL: u32 = 0x0115;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
const SB_VERT: i32 = 1;
const SIF_RANGE: u32 = 0x1;
const SIF_PAGE: u32 = 0x2;
const SIF_POS: u32 = 0x4;
const SIF_TRACKPOS: u32 = 0x10;
const SW_SCROLLCHILDREN: u32 = 0x1;
const SW_INVALIDATE: u32 = 0x2;
const SW_ERASE: u32 = 0x4;
const IDC_ARROW: usize = 32512;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Cria o painel (medidas ja escaladas para o DPI). Chamado uma vez.
pub unsafe fn create(parent: isize, inst: isize, x: i32, y: i32, w: i32, h: i32, id: usize, background: isize) -> isize {
    let class = wide("GoLiveLiteModsPanel");
    let wc = WndClassExW {
        cb_size: std::mem::size_of::<WndClassExW>() as u32,
        style: 0,
        lpfn_wnd_proc: panel_proc,
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: inst,
        h_icon: 0,
        h_cursor: LoadCursorW(0, IDC_ARROW as *const u16),
        hbr_background: background,
        lpsz_menu_name: std::ptr::null(),
        lpsz_class_name: class.as_ptr(),
        h_icon_sm: 0,
    };
    let _ = RegisterClassExW(&wc);
    let name = wide("");
    let hwnd = CreateWindowExW(
        WS_EX_CONTROLPARENT,
        class.as_ptr(),
        name.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN,
        x,
        y,
        w,
        h,
        parent,
        id as isize,
        inst,
        std::ptr::null(),
    );
    PANEL.store(hwnd, Ordering::Relaxed);
    POS.store(0, Ordering::Relaxed);
    hwnd
}

pub fn handle() -> isize {
    PANEL.load(Ordering::Relaxed)
}

/// Um controle de dentro do painel (`GetDlgItem` da janela principal nao enxerga eles).
pub fn find(id: usize) -> isize {
    let p = handle();
    if p == 0 {
        0
    } else {
        unsafe { GetDlgItem(p, id as i32) }
    }
}

/// Altura total do conteudo (px escalados) e altura visivel: ajusta a barra.
pub unsafe fn set_content_height(content_h: i32, visible_h: i32) {
    CONTENT_H.store(content_h, Ordering::Relaxed);
    let si = ScrollInfo {
        cb_size: std::mem::size_of::<ScrollInfo>() as u32,
        f_mask: SIF_RANGE | SIF_PAGE | SIF_POS,
        n_min: 0,
        n_max: (content_h - 1).max(0),
        n_page: visible_h.max(1) as u32,
        n_pos: 0,
        n_track_pos: 0,
    };
    SetScrollInfo(handle(), SB_VERT, &si, 1);
}

unsafe fn scroll_to(hwnd: isize, target: i32) {
    let mut si = ScrollInfo {
        cb_size: std::mem::size_of::<ScrollInfo>() as u32,
        f_mask: SIF_RANGE | SIF_PAGE | SIF_POS,
        n_min: 0,
        n_max: 0,
        n_page: 0,
        n_pos: 0,
        n_track_pos: 0,
    };
    GetScrollInfo(hwnd, SB_VERT, &mut si);
    let max_pos = (si.n_max - si.n_page as i32 + 1).max(0);
    let new_pos = target.clamp(0, max_pos);
    let old = POS.load(Ordering::Relaxed);
    if new_pos == old {
        return;
    }
    POS.store(new_pos, Ordering::Relaxed);
    si.f_mask = SIF_POS;
    si.n_pos = new_pos;
    SetScrollInfo(hwnd, SB_VERT, &si, 1);
    ScrollWindowEx(
        hwnd,
        0,
        old - new_pos,
        std::ptr::null(),
        std::ptr::null(),
        0,
        std::ptr::null_mut(),
        SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE,
    );
}

/// Rola pela roda do mouse (o instalador repassa a roda para ca quando esta na etapa 2).
pub unsafe fn wheel(w_param: usize) {
    let hwnd = handle();
    if hwnd == 0 {
        return;
    }
    let delta = ((w_param >> 16) & 0xFFFF) as u16 as i16 as i32;
    let step = crate::look::s(20) * 3;
    scroll_to(hwnd, POS.load(Ordering::Relaxed) - delta * step / 120);
}

unsafe extern "system" fn panel_proc(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize {
    match msg {
        // Cliques e cores: quem decide e a janela do instalador.
        WM_COMMAND | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => SendMessageW(GetParent(hwnd), msg, w_param, l_param),
        WM_MOUSEWHEEL => {
            wheel(w_param);
            0
        }
        WM_VSCROLL => {
            let line = crate::look::s(20);
            let mut si = ScrollInfo {
                cb_size: std::mem::size_of::<ScrollInfo>() as u32,
                f_mask: SIF_PAGE | SIF_TRACKPOS | SIF_POS,
                n_min: 0,
                n_max: 0,
                n_page: 0,
                n_pos: 0,
                n_track_pos: 0,
            };
            GetScrollInfo(hwnd, SB_VERT, &mut si);
            let pos = POS.load(Ordering::Relaxed);
            let target = match (w_param & 0xFFFF) as u32 {
                0 => pos - line,                  // SB_LINEUP
                1 => pos + line,                  // SB_LINEDOWN
                2 => pos - si.n_page as i32,      // SB_PAGEUP
                3 => pos + si.n_page as i32,      // SB_PAGEDOWN
                4 | 5 => si.n_track_pos,          // SB_THUMBPOSITION / SB_THUMBTRACK
                6 => 0,                           // SB_TOP
                7 => CONTENT_H.load(Ordering::Relaxed), // SB_BOTTOM
                _ => pos,
            };
            scroll_to(hwnd, target);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

#[link(name = "user32")]
extern "system" {
    fn RedrawWindow(h: isize, rect: *const std::ffi::c_void, rgn: isize, flags: u32) -> i32;
}

/// Repinta o painel e tudo dentro dele (ao entrar na etapa: os textos so apareciam depois de rolar).
pub fn redraw() {
    let h = handle();
    if h != 0 {
        // RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW
        unsafe { RedrawWindow(h, std::ptr::null(), 0, 0x1 | 0x4 | 0x80 | 0x100) };
    }
}
