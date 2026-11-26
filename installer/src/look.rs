//! Visual do instalador: escala por DPI, paleta do Go Live e o desenho da barra lateral
//! (logo + etapas) e do rodape. Os controles continuam sendo os nativos do Windows; aqui so
//! fica o que e pintado a mao e as cores que o WM_CTLCOLOR* devolve.

use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct PaintStruct {
    pub hdc: isize,
    pub f_erase: i32,
    pub rc_paint: Rect,
    pub f_restore: i32,
    pub f_inc_update: i32,
    pub rgb_reserved: [u8; 32],
}

#[link(name = "user32")]
extern "system" {
    pub fn BeginPaint(hwnd: isize, ps: *mut PaintStruct) -> isize;
    pub fn EndPaint(hwnd: isize, ps: *const PaintStruct) -> i32;
    fn FillRect(hdc: isize, rc: *const Rect, brush: isize) -> i32;
    fn DrawTextW(hdc: isize, text: *const u16, len: i32, rc: *mut Rect, format: u32) -> i32;
    fn DrawIconEx(hdc: isize, x: i32, y: i32, icon: isize, cx: i32, cy: i32, step: u32, brush: isize, flags: u32) -> i32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    pub fn InvalidateRect(hwnd: isize, rc: *const Rect, erase: i32) -> i32;
    pub fn GetClientRect(hwnd: isize, rc: *mut Rect) -> i32;
    pub fn AdjustWindowRectEx(rc: *mut Rect, style: u32, menu: i32, ex_style: u32) -> i32;
    pub fn GetDlgCtrlID(hwnd: isize) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateSolidBrush(color: u32) -> isize;
    fn CreatePen(style: i32, width: i32, color: u32) -> isize;
    fn SelectObject(hdc: isize, obj: isize) -> isize;
    fn DeleteObject(obj: isize) -> i32;
    pub fn SetTextColor(hdc: isize, color: u32) -> u32;
    pub fn SetBkColor(hdc: isize, color: u32) -> u32;
    fn SetBkMode(hdc: isize, mode: i32) -> i32;
    fn GetDeviceCaps(hdc: isize, index: i32) -> i32;
    fn Ellipse(hdc: isize, l: i32, t: i32, r: i32, b: i32) -> i32;
    fn GetStockObject(i: i32) -> isize;
}

const LOGPIXELSY: i32 = 90;
const TRANSPARENT: i32 = 1;
const DT_LEFT: u32 = 0x0;
const DT_CENTER: u32 = 0x1;
const DT_VCENTER: u32 = 0x4;
const DT_SINGLELINE: u32 = 0x20;
const DT_NOPREFIX: u32 = 0x800;
const DT_END_ELLIPSIS: u32 = 0x8000;
const DI_NORMAL: u32 = 0x3;
const NULL_PEN: i32 = 8;

// --- Paleta (a mesma da sala nova do site) -------------------------------------

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub const SIDEBAR_BG: u32 = rgb(13, 17, 23); // #0d1117
const SIDEBAR_TEXT: u32 = rgb(240, 246, 252);
const SIDEBAR_MUTED: u32 = rgb(139, 148, 158);
const SIDEBAR_LINE: u32 = rgb(48, 54, 61);
const ACCENT: u32 = rgb(16, 185, 129); // emerald-500
const ACCENT_DONE: u32 = rgb(5, 150, 105); // emerald-600
pub const CONTENT_BG: u32 = rgb(255, 255, 255);
pub const FOOTER_BG: u32 = rgb(246, 248, 250);
const FOOTER_LINE: u32 = rgb(209, 217, 224);
pub const TEXT_STRONG: u32 = rgb(31, 35, 40);
pub const TEXT_MUTED: u32 = rgb(89, 99, 110);
pub const TEXT_ACCENT: u32 = rgb(4, 120, 87); // emerald-700

// --- Medidas em unidades de 96 DPI (escaladas por `s`) ---------------------------

pub const SIDEBAR_W: i32 = 200;
pub const CONTENT_W: i32 = 534;
pub const CLIENT_H: i32 = 430;
pub const FOOTER_Y: i32 = 364;

static SCALE_PCT: AtomicI32 = AtomicI32::new(100);

/// Le o DPI do sistema uma vez (o manifesto declara DPI-aware, entao nada e esticado pelo
/// Windows: sem isso tudo ficava minusculo em telas a 125-150%).
pub fn init_dpi() {
    unsafe {
        let dc = GetDC(0);
        if dc != 0 {
            let dpi = GetDeviceCaps(dc, LOGPIXELSY);
            ReleaseDC(0, dc);
            if dpi > 0 {
                SCALE_PCT.store((dpi * 100 / 96).clamp(100, 300), Ordering::Relaxed);
            }
        }
    }
}

/// Escala uma medida de 96 DPI para o DPI da tela.
pub fn s(v: i32) -> i32 {
    (v * SCALE_PCT.load(Ordering::Relaxed) + 50) / 100
}

/// Pincel solido criado uma vez por cor (vive ate o processo acabar; sao 3).
fn cached_brush(slot: &AtomicIsize, color: u32) -> isize {
    let cur = slot.load(Ordering::Relaxed);
    if cur != 0 {
        return cur;
    }
    let b = unsafe { CreateSolidBrush(color) };
    slot.store(b, Ordering::Relaxed);
    b
}

static BRUSH_CONTENT: AtomicIsize = AtomicIsize::new(0);
static BRUSH_FOOTER: AtomicIsize = AtomicIsize::new(0);
static BRUSH_SIDEBAR: AtomicIsize = AtomicIsize::new(0);

pub fn content_brush() -> isize {
    cached_brush(&BRUSH_CONTENT, CONTENT_BG)
}
pub fn footer_brush() -> isize {
    cached_brush(&BRUSH_FOOTER, FOOTER_BG)
}
fn sidebar_brush() -> isize {
    cached_brush(&BRUSH_SIDEBAR, SIDEBAR_BG)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

unsafe fn text(hdc: isize, s: &str, font: isize, color: u32, mut rc: Rect, flags: u32) {
    let old = SelectObject(hdc, font);
    SetTextColor(hdc, color);
    let w = wide(s);
    DrawTextW(hdc, w.as_ptr(), w.len() as i32, &mut rc, flags | DT_NOPREFIX);
    SelectObject(hdc, old);
}

unsafe fn fill(hdc: isize, rc: Rect, color: u32) {
    let b = CreateSolidBrush(color);
    FillRect(hdc, &rc, b);
    DeleteObject(b);
}

unsafe fn circle(hdc: isize, cx: i32, cy: i32, r: i32, color: u32) {
    let b = CreateSolidBrush(color);
    let old_b = SelectObject(hdc, b);
    let old_p = SelectObject(hdc, GetStockObject(NULL_PEN));
    Ellipse(hdc, cx - r, cy - r, cx + r + 1, cy + r + 1);
    SelectObject(hdc, old_p);
    SelectObject(hdc, old_b);
    DeleteObject(b);
}

unsafe fn ring(hdc: isize, cx: i32, cy: i32, r: i32, color: u32) {
    let pen = CreatePen(0, s(1).max(1), color);
    let old_p = SelectObject(hdc, pen);
    let old_b = SelectObject(hdc, GetStockObject(5 /* NULL_BRUSH */));
    Ellipse(hdc, cx - r, cy - r, cx + r + 1, cy + r + 1);
    SelectObject(hdc, old_b);
    SelectObject(hdc, old_p);
    DeleteObject(pen);
}

pub struct Fonts {
    pub semibold: isize,
    pub regular: isize,
    pub small: isize,
}

/// Onde a pessoa esta no fluxo, para a lista de etapas da barra lateral.
pub struct StepsView<'a> {
    pub steps: &'a [&'a str],
    /// Indice da etapa atual; `steps.len()` = tudo concluido.
    pub current: usize,
}

/// Pinta a barra lateral e a faixa do rodape. Os controles filhos se pintam sozinhos.
pub unsafe fn paint(hwnd: isize, icon: isize, fonts: &Fonts, view: &StepsView<'_>) {
    let mut ps: PaintStruct = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc == 0 {
        return;
    }
    let mut client = Rect::default();
    GetClientRect(hwnd, &mut client);
    SetBkMode(hdc, TRANSPARENT);

    // Barra lateral
    let side = Rect { left: 0, top: 0, right: s(SIDEBAR_W), bottom: client.bottom };
    FillRect(hdc, &side, sidebar_brush());
    let pad = s(24);
    if icon != 0 {
        DrawIconEx(hdc, pad, s(28), icon, s(40), s(40), 0, 0, DI_NORMAL);
    }
    text(
        hdc,
        "Go Live - Lite",
        fonts.semibold,
        SIDEBAR_TEXT,
        Rect { left: pad, top: s(78), right: side.right - s(12), bottom: s(98) },
        DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    text(
        hdc,
        "Instalador",
        fonts.small,
        SIDEBAR_MUTED,
        Rect { left: pad, top: s(98), right: side.right - s(12), bottom: s(116) },
        DT_LEFT | DT_SINGLELINE,
    );
    fill(hdc, Rect { left: pad, top: s(132), right: side.right - pad, bottom: s(132) + s(1).max(1) }, SIDEBAR_LINE);

    // Etapas
    let r = s(11);
    let mut y = s(160);
    for (i, label) in view.steps.iter().enumerate() {
        let cx = pad + r;
        let cy = y + r;
        if i + 1 < view.steps.len() {
            // Linha ligando ao proximo circulo.
            let col = if i < view.current { ACCENT_DONE } else { SIDEBAR_LINE };
            fill(hdc, Rect { left: cx, top: cy + r + s(3), right: cx + s(1).max(1), bottom: cy + s(40) - r - s(3) }, col);
        }
        let (label_color, font) = if i < view.current {
            circle(hdc, cx, cy, r, ACCENT_DONE);
            text(hdc, "✓", fonts.semibold, SIDEBAR_TEXT, Rect { left: cx - r, top: cy - r, right: cx + r + 1, bottom: cy + r + 1 }, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            (SIDEBAR_MUTED, fonts.regular)
        } else if i == view.current {
            circle(hdc, cx, cy, r, ACCENT);
            text(hdc, &(i + 1).to_string(), fonts.semibold, SIDEBAR_BG, Rect { left: cx - r, top: cy - r, right: cx + r + 1, bottom: cy + r + 1 }, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            (SIDEBAR_TEXT, fonts.semibold)
        } else {
            ring(hdc, cx, cy, r, SIDEBAR_LINE);
            text(hdc, &(i + 1).to_string(), fonts.small, SIDEBAR_MUTED, Rect { left: cx - r, top: cy - r, right: cx + r + 1, bottom: cy + r + 1 }, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            (SIDEBAR_MUTED, fonts.regular)
        };
        text(
            hdc,
            label,
            font,
            label_color,
            Rect { left: cx + r + s(12), top: cy - s(10), right: side.right - s(8), bottom: cy + s(10) },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        y += s(40);
    }

    // Rodape da barra: versao e site
    text(
        hdc,
        &format!("Versão {}", env!("CARGO_PKG_VERSION")),
        fonts.small,
        SIDEBAR_MUTED,
        Rect { left: pad, top: client.bottom - s(36), right: side.right - s(8), bottom: client.bottom - s(16) },
        DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
    );

    // Faixa dos botoes
    let footer = Rect { left: side.right, top: s(FOOTER_Y), right: client.right, bottom: client.bottom };
    FillRect(hdc, &footer, footer_brush());
    fill(hdc, Rect { left: side.right, top: footer.top, right: client.right, bottom: footer.top + s(1).max(1) }, FOOTER_LINE);

    EndPaint(hwnd, &ps);
}

/// Repinta so a barra lateral (troca de etapa) sem piscar o conteudo.
pub fn invalidate_sidebar(hwnd: isize) {
    unsafe {
        let mut client = Rect::default();
        GetClientRect(hwnd, &mut client);
        let rc = Rect { left: 0, top: 0, right: s(SIDEBAR_W), bottom: client.bottom };
        InvalidateRect(hwnd, &rc, 0);
    }
}

