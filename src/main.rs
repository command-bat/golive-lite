#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod diag_log;
mod app;
mod config;
mod media;
mod net;
mod ui;
mod win_tray;

use app::GoLiveLiteApp;
use std::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(lp_lib_file_name: *const u8) -> isize;
    fn GetModuleHandleA(lp_module_name: *const u8) -> isize;
    fn GetProcAddress(h_module: isize, lp_proc_name: *const u8) -> *const c_void;
}

/// Configura DPI awareness de forma compativel com Windows 11/10/8.1/7 sem import estatico de Win10+.
fn init_dpi_awareness() {
    unsafe {
        let user32 = GetModuleHandleA(b"user32.dll\0".as_ptr());
        let user32 = if user32 != 0 {
            user32
        } else {
            LoadLibraryA(b"user32.dll\0".as_ptr())
        };
        if user32 != 0 {
            let p_ctx = GetProcAddress(user32, b"SetProcessDpiAwarenessContext\0".as_ptr());
            if !p_ctx.is_null() {
                let func: unsafe extern "system" fn(isize) -> i32 = std::mem::transmute(p_ctx);
                if func(-4isize) != 0 {
                    return;
                }
            }
        }

        let shcore = LoadLibraryA(b"shcore.dll\0".as_ptr());
        if shcore != 0 {
            let p_aw = GetProcAddress(shcore, b"SetProcessDpiAwareness\0".as_ptr());
            if !p_aw.is_null() {
                let func: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(p_aw);
                if func(2) >= 0 {
                    return;
                }
            }
        }

        if user32 != 0 {
            let p_old = GetProcAddress(user32, b"SetProcessDPIAware\0".as_ptr());
            if !p_old.is_null() {
                let func: unsafe extern "system" fn() -> i32 = std::mem::transmute(p_old);
                let _ = func();
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let start_hidden = std::env::args()
        .skip(1)
        .any(|a| matches!(a.as_str(), "--hidden" | "--tray" | "--autostart"));

    // Builds de desenvolvimento podem rodar ao lado do app instalado (prints/testes de layout)
    // com GOLIVE_LITE_ALLOW_MULTI=1. Nunca no release.
    let allow_multi = cfg!(debug_assertions) && std::env::var_os("GOLIVE_LITE_ALLOW_MULTI").is_some();
    let _single_instance = if allow_multi {
        None
    } else {
        let Some(guard) = win_tray::try_acquire_single_instance(start_hidden) else {
            return Ok(());
        };
        Some(guard)
    };

    diag_log::init_logging();
    init_dpi_awareness();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Go Live - Lite")
            .with_inner_size([1160.0, 720.0])
            .with_min_inner_size([780.0, 520.0])
            .with_visible(!start_hidden)
            .with_icon(std::sync::Arc::new(crate::ui::app_icon::build_window_icon())),
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 0,
        dithering: false,
        ..Default::default()
    };

    let res = eframe::run_native(
        "Go Live - Lite",
        options,
        Box::new(move |cc| Ok(Box::new(GoLiveLiteApp::new(cc, start_hidden)))),
    );
    diag_info!("shutdown", "Go Live - Lite encerrado cleanly={}", res.is_ok());
    res
}
