//! Integracao nativa leve com a Bandeja do Sistema (Shell_NotifyIconW), instancia unica,
//! autostart no Registro do Windows e controle do comportamento do botao Fechar (X).

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

const WM_NULL: u32 = 0x0000;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_COMMAND: u32 = 0x0111;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_CONTEXTMENU: u32 = 0x007B;
const WM_USER: u32 = 0x0400;
const WM_APP: u32 = 0x8000;
const WM_TRAY_CALLBACK: u32 = WM_USER + 42;
const WM_APP_SHOW_MAIN: u32 = WM_APP + 1;

const GWLP_USERDATA: i32 = -21;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const SW_HIDE: i32 = 0;
const SW_SHOWNORMAL: i32 = 1;
const SW_SHOW: i32 = 5;
const SW_RESTORE: i32 = 9;

const IMAGE_ICON: u32 = 1;
const LR_DEFAULTCOLOR: u32 = 0x0000;
const LR_SHARED: u32 = 0x8000;
const IDI_APPLICATION: usize = 32512;
const SM_CXSMICON: i32 = 49;
const SM_CYSMICON: i32 = 50;

const NIM_ADD: u32 = 0x0000_0000;
const NIM_MODIFY: u32 = 0x0000_0001;
const NIM_DELETE: u32 = 0x0000_0002;
const NIF_MESSAGE: u32 = 0x0000_0001;
const NIF_ICON: u32 = 0x0000_0002;
const NIF_TIP: u32 = 0x0000_0004;

const MF_STRING: u32 = 0x0000_0000;
const MF_CHECKED: u32 = 0x0000_0008;
const MF_SEPARATOR: u32 = 0x0000_0800;
const TPM_LEFTALIGN: u32 = 0x0000;
const TPM_RIGHTBUTTON: u32 = 0x0002;
const TPM_BOTTOMALIGN: u32 = 0x0020;

const IDM_SHOW: usize = 1001;
const IDM_AUTOSTART: usize = 1002;
const IDM_CLOSE_TRAY: usize = 1003;
const IDM_QUIT: usize = 1004;

const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const HKEY_CURRENT_USER: isize = -2147483647_i32 as isize;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;

const TRAY_WND_CLASS: &str = "GoLiveLiteDesktopTrayMsgWnd";
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "Go Live Lite - Desktop";
const LEGACY_RUN_VALUE_NAME: &str = "GoLiveLite";
const MUTEX_NAME: &str = "Local\\GoLiveLiteDesktop_SingleInstance_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    ShowWindow,
    ToggleAutostart,
    ToggleCloseToTray,
    QuitApp,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: Point,
}

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
struct NotifyIconDataW {
    cb_size: u32,
    h_wnd: isize,
    u_id: u32,
    u_flags: u32,
    u_callback_message: u32,
    h_icon: isize,
    sz_tip: [u16; 128],
    dw_state: u32,
    dw_state_mask: u32,
    sz_info: [u16; 256],
    u_version_or_timeout: u32,
    sz_info_title: [u16; 64],
    dw_info_flags: u32,
    guid_item: [u8; 16],
    h_balloon_icon: isize,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(lp_module_name: *const u16) -> isize;
    fn CreateMutexW(lp_mutex_attributes: *const c_void, b_initial_owner: i32, lp_name: *const u16) -> isize;
    fn GetLastError() -> u32;
    fn CloseHandle(h_object: isize) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(lpwcx: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        dw_ex_style: u32,
        lp_class_name: *const u16,
        lp_window_name: *const u16,
        dw_style: u32,
        x: i32,
        y: i32,
        n_width: i32,
        n_height: i32,
        h_wnd_parent: isize,
        h_menu: isize,
        h_instance: isize,
        lp_param: *const c_void,
    ) -> isize;
    fn DefWindowProcW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize;
    fn DestroyWindow(hwnd: isize) -> i32;
    fn PostQuitMessage(n_exit_code: i32);
    fn GetMessageW(lp_msg: *mut Msg, hwnd: isize, w_msg_filter_min: u32, w_msg_filter_max: u32) -> i32;
    fn TranslateMessage(lp_msg: *const Msg) -> i32;
    fn DispatchMessageW(lp_msg: *const Msg) -> isize;
    fn PostMessageW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16) -> isize;
    fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn SetWindowLongPtrW(hwnd: isize, n_index: i32, dw_new_long: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: isize, n_index: i32) -> isize;
    fn RegisterWindowMessageW(lp_string: *const u16) -> u32;
    fn LoadImageW(
        h_inst: isize,
        name: *const u16,
        u_type: u32,
        cx: i32,
        cy: i32,
        fu_load: u32,
    ) -> isize;
    fn GetSystemMetrics(n_index: i32) -> i32;
    fn DestroyIcon(h_icon: isize) -> i32;
    fn CreatePopupMenu() -> isize;
    fn AppendMenuW(h_menu: isize, u_flags: u32, u_id_new_item: usize, lp_new_item: *const u16) -> i32;
    fn SetMenuDefaultItem(h_menu: isize, u_item: u32, f_by_pos: u32) -> i32;
    fn TrackPopupMenu(
        h_menu: isize,
        u_flags: u32,
        x: i32,
        y: i32,
        n_reserved: i32,
        h_wnd: isize,
        prc_rect: *const c_void,
    ) -> i32;
    fn DestroyMenu(h_menu: isize) -> i32;
    fn GetCursorPos(lp_point: *mut Point) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(dw_message: u32, lp_data: *const NotifyIconDataW) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(
        h_key: isize,
        lp_sub_key: *const u16,
        reserved: u32,
        lp_class: *const u16,
        dw_options: u32,
        sam_desired: u32,
        lp_security_attributes: *const c_void,
        phk_result: *mut isize,
        lpdw_disposition: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        h_key: isize,
        lp_value_name: *const u16,
        reserved: u32,
        dw_type: u32,
        lp_data: *const u8,
        cb_data: u32,
    ) -> i32;
    fn RegDeleteValueW(h_key: isize, lp_value_name: *const u16) -> i32;
    fn RegCloseKey(h_key: isize) -> i32;
}

fn to_wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Garante apenas uma instancia do GoLive Lite rodando por sessao de usuario.
/// Se ja existir e esta nova abertura nao for `--hidden`, restaura a janela existente.
pub struct SingleInstanceGuard {
    handle: isize,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

pub fn try_acquire_single_instance(start_hidden: bool) -> Option<SingleInstanceGuard> {
    let name_w = to_wide_z(MUTEX_NAME);
    let handle = unsafe { CreateMutexW(null(), 1, name_w.as_ptr()) };
    if handle == 0 {
        return Some(SingleInstanceGuard { handle: 0 });
    }
    let err = unsafe { GetLastError() };
    if err == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        if !start_hidden {
            let class_w = to_wide_z(TRAY_WND_CLASS);
            let existing = unsafe { FindWindowW(class_w.as_ptr(), null()) };
            if existing != 0 {
                unsafe {
                    let _ = PostMessageW(existing, WM_APP_SHOW_MAIN, 0, 0);
                }
            } else {
                restore_main_window_win32();
            }
        }
        return None;
    }
    Some(SingleInstanceGuard { handle })
}

pub fn format_autostart_command(exe_path: &Path) -> String {
    format!("\"{}\" --hidden", exe_path.display())
}

/// Grava ou remove o valor `GoLiveLite` em `HKCU\...\Run` para iniciar escondido na bandeja.
pub fn sync_autostart_registry(enabled: bool) -> Result<(), String> {
    let subkey_w = to_wide_z(RUN_SUBKEY);
    let val_name_w = to_wide_z(RUN_VALUE_NAME);
    let mut hkey: isize = 0;

    let open_res = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut hkey,
            null_mut(),
        )
    };
    if open_res != 0 || hkey == 0 {
        return Err(format!("falha ao abrir chave Run (erro {})", open_res));
    }

    let legacy_w = to_wide_z(LEGACY_RUN_VALUE_NAME);
    unsafe {
        let _ = RegDeleteValueW(hkey, legacy_w.as_ptr());
    }

    let result = if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("falha ao localizar executavel: {e}"))?;
        let cmd = format_autostart_command(&exe);
        let cmd_w = to_wide_z(&cmd);
        let byte_len = (cmd_w.len() * 2) as u32;
        let set_res = unsafe {
            RegSetValueExW(
                hkey,
                val_name_w.as_ptr(),
                0,
                REG_SZ,
                cmd_w.as_ptr() as *const u8,
                byte_len,
            )
        };
        if set_res == 0 {
            Ok(())
        } else {
            Err(format!("falha ao gravar autostart (erro {})", set_res))
        }
    } else {
        let del_res = unsafe { RegDeleteValueW(hkey, val_name_w.as_ptr()) };
        if del_res == 0 || del_res == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("falha ao remover autostart (erro {})", del_res))
        }
    };

    unsafe {
        let _ = RegCloseKey(hkey);
    }
    result
}

pub fn restore_main_window_win32() {
    let title_w = to_wide_z("Go Live - Lite");
    let hwnd = unsafe { FindWindowW(null(), title_w.as_ptr()) };
    if hwnd != 0 {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn hide_main_window_win32() {
    let title_w = to_wide_z("Go Live - Lite");
    let hwnd = unsafe { FindWindowW(null(), title_w.as_ptr()) };
    if hwnd != 0 {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

struct TrayShared {
    autostart: AtomicBool,
    close_to_tray: AtomicBool,
    tray_hwnd: AtomicIsize,
}

struct TrayWndState {
    shared: Arc<TrayShared>,
    tx: mpsc::UnboundedSender<TrayEvent>,
    ctx: egui::Context,
    taskbar_created_msg: u32,
    h_icon: isize,
    owns_icon: bool,
    tip_w: Vec<u16>,
}

pub struct TrayController {
    shared: Arc<TrayShared>,
}

impl TrayController {
    pub fn spawn(
        ctx: egui::Context,
        autostart_init: bool,
        close_to_tray_init: bool,
    ) -> (Self, mpsc::UnboundedReceiver<TrayEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(TrayShared {
            autostart: AtomicBool::new(autostart_init),
            close_to_tray: AtomicBool::new(close_to_tray_init),
            tray_hwnd: AtomicIsize::new(0),
        });
        let shared_th = Arc::clone(&shared);

        let _ = std::thread::Builder::new()
            .name("golive-tray".into())
            .spawn(move || run_tray_loop(shared_th, tx, ctx));

        (Self { shared }, rx)
    }

    pub fn set_flags(&self, autostart: bool, close_to_tray: bool) {
        self.shared.autostart.store(autostart, Ordering::Relaxed);
        self.shared
            .close_to_tray
            .store(close_to_tray, Ordering::Relaxed);
    }

    pub fn shutdown(&self) {
        let hwnd = self.shared.tray_hwnd.swap(0, Ordering::AcqRel);
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
    }
}

fn run_tray_loop(
    shared: Arc<TrayShared>,
    tx: mpsc::UnboundedSender<TrayEvent>,
    ctx: egui::Context,
) {
    let h_inst = unsafe { GetModuleHandleW(null()) };
    let class_w = to_wide_z(TRAY_WND_CLASS);

    let wc = WndClassExW {
        cb_size: size_of::<WndClassExW>() as u32,
        style: 0,
        lpfn_wnd_proc: tray_wnd_proc,
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: h_inst,
        h_icon: 0,
        h_cursor: 0,
        hbr_background: 0,
        lpsz_menu_name: null(),
        lpsz_class_name: class_w.as_ptr(),
        h_icon_sm: 0,
    };
    unsafe {
        let _ = RegisterClassExW(&wc);
    }

    let wnd_name_w = to_wide_z("Go Live - Lite Tray");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_w.as_ptr(),
            wnd_name_w.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            h_inst,
            null(),
        )
    };
    if hwnd == 0 {
        return;
    }

    let sm_cx = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let sm_cy = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(16);
    let mut owns_icon = true;
    let mut h_icon = unsafe {
        LoadImageW(
            h_inst,
            1usize as *const u16,
            IMAGE_ICON,
            sm_cx,
            sm_cy,
            LR_DEFAULTCOLOR,
        )
    };
    if h_icon == 0 {
        owns_icon = false;
        h_icon = unsafe {
            LoadImageW(
                0,
                IDI_APPLICATION as *const u16,
                IMAGE_ICON,
                0,
                0,
                LR_SHARED,
            )
        };
    }

    let tb_msg_w = to_wide_z("TaskbarCreated");
    let taskbar_created_msg = unsafe { RegisterWindowMessageW(tb_msg_w.as_ptr()) };

    let mut tip = vec![0u16; 128];
    for (dst, src) in tip.iter_mut().zip("Go Live - Lite".encode_utf16()) {
        *dst = src;
    }

    let state = Box::new(TrayWndState {
        shared: Arc::clone(&shared),
        tx,
        ctx,
        taskbar_created_msg,
        h_icon,
        owns_icon,
        tip_w: tip,
    });
    let state_ptr = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
    }
    shared.tray_hwnd.store(hwnd, Ordering::Release);

    unsafe {
        add_or_update_tray_icon(hwnd, &*state_ptr, NIM_ADD);
    }

    let mut msg: Msg = unsafe { zeroed() };
    loop {
        let ret = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    shared.tray_hwnd.store(0, Ordering::Release);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let boxed = Box::from_raw(state_ptr);
        if boxed.owns_icon && boxed.h_icon != 0 {
            let _ = DestroyIcon(boxed.h_icon);
        }
        drop(boxed);
    }
}

unsafe fn add_or_update_tray_icon(hwnd: isize, st: &TrayWndState, action: u32) {
    let mut nid: NotifyIconDataW = zeroed();
    nid.cb_size = size_of::<NotifyIconDataW>() as u32;
    nid.h_wnd = hwnd;
    nid.u_id = 1;
    nid.u_flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.u_callback_message = WM_TRAY_CALLBACK;
    nid.h_icon = st.h_icon;
    nid.sz_tip.copy_from_slice(&st.tip_w[..128]);
    let _ = Shell_NotifyIconW(action, &nid);
    let _ = NIM_MODIFY;
}

unsafe fn remove_tray_icon(hwnd: isize) {
    let mut nid: NotifyIconDataW = zeroed();
    nid.cb_size = size_of::<NotifyIconDataW>() as u32;
    nid.h_wnd = hwnd;
    nid.u_id = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn show_tray_context_menu(hwnd: isize, st: &TrayWndState) {
    let hmenu = CreatePopupMenu();
    if hmenu == 0 {
        return;
    }

    let show_w = to_wide_z("Abrir Go Live - Lite");
    let auto_w = to_wide_z("Iniciar com o Windows (escondido)");
    let close_w = to_wide_z("Fechar (X) minimiza para a bandeja");
    let quit_w = to_wide_z("Sair do Go Live - Lite");

    let auto_flags = MF_STRING
        | if st.shared.autostart.load(Ordering::Relaxed) {
            MF_CHECKED
        } else {
            0
        };
    let close_flags = MF_STRING
        | if st.shared.close_to_tray.load(Ordering::Relaxed) {
            MF_CHECKED
        } else {
            0
        };

    let _ = AppendMenuW(hmenu, MF_STRING, IDM_SHOW, show_w.as_ptr());
    let _ = SetMenuDefaultItem(hmenu, IDM_SHOW as u32, 0);
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, null());
    let _ = AppendMenuW(hmenu, auto_flags, IDM_AUTOSTART, auto_w.as_ptr());
    let _ = AppendMenuW(hmenu, close_flags, IDM_CLOSE_TRAY, close_w.as_ptr());
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, null());
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_QUIT, quit_w.as_ptr());

    let mut pt = Point { x: 0, y: 0 };
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(
        hmenu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    );
    let _ = PostMessageW(hwnd, WM_NULL, 0, 0);
    let _ = DestroyMenu(hmenu);
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: isize,
    msg: u32,
    w_param: usize,
    l_param: isize,
) -> isize {
    let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const TrayWndState;
    if !st_ptr.is_null() {
        let st = &*st_ptr;
        if st.taskbar_created_msg != 0 && msg == st.taskbar_created_msg {
            add_or_update_tray_icon(hwnd, st, NIM_ADD);
            return 0;
        }

        match msg {
            WM_APP_SHOW_MAIN => {
                restore_main_window_win32();
                let _ = st.tx.send(TrayEvent::ShowWindow);
                st.ctx.request_repaint();
                return 0;
            }
            WM_TRAY_CALLBACK => {
                let mouse_msg = (l_param as u32) & 0xFFFF;
                match mouse_msg {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                        restore_main_window_win32();
                        let _ = st.tx.send(TrayEvent::ShowWindow);
                        st.ctx.request_repaint();
                    }
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        show_tray_context_menu(hwnd, st);
                    }
                    _ => {}
                }
                return 0;
            }
            WM_COMMAND => {
                match w_param & 0xFFFF {
                    IDM_SHOW => {
                        restore_main_window_win32();
                        let _ = st.tx.send(TrayEvent::ShowWindow);
                        st.ctx.request_repaint();
                    }
                    IDM_AUTOSTART => {
                        let _ = st.tx.send(TrayEvent::ToggleAutostart);
                        st.ctx.request_repaint();
                    }
                    IDM_CLOSE_TRAY => {
                        let _ = st.tx.send(TrayEvent::ToggleCloseToTray);
                        st.ctx.request_repaint();
                    }
                    IDM_QUIT => {
                        let _ = st.tx.send(TrayEvent::QuitApp);
                        st.ctx.request_repaint();
                    }
                    _ => {}
                }
                return 0;
            }
            WM_CLOSE => {
                remove_tray_icon(hwnd);
                let _ = DestroyWindow(hwnd);
                return 0;
            }
            WM_DESTROY => {
                remove_tray_icon(hwnd);
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
    } else if msg == WM_CLOSE {
        let _ = DestroyWindow(hwnd);
        return 0;
    } else if msg == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }

    DefWindowProcW(hwnd, msg, w_param, l_param)
}

#[cfg(windows)]
pub fn trim_process_working_set() {
    extern "C" {
        fn _heapmin() -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(lp_lib_file_name: *const u8) -> isize;
        fn GetProcAddress(h_module: isize, lp_proc_name: *const u8) -> *const std::ffi::c_void;
        fn GetCurrentProcess() -> isize;
        fn GetProcessHeap() -> isize;
        fn GetProcessHeaps(number_of_heaps: u32, process_heaps: *mut isize) -> u32;
        fn HeapCompact(h_heap: isize, dw_flags: u32) -> usize;
        fn SetProcessWorkingSetSizeEx(
            h_process: isize,
            dw_minimum_working_set_size: usize,
            dw_maximum_working_set_size: usize,
            flags: u32,
        ) -> i32;
    }
    type K32EmptyWorkingSetFn = unsafe extern "system" fn(isize) -> i32;
    unsafe {
        let _ = _heapmin();
        let def_heap = GetProcessHeap();
        if def_heap != 0 {
            let _ = HeapCompact(def_heap, 0);
        }
        let mut heaps = [0isize; 32];
        let count = GetProcessHeaps(heaps.len() as u32, heaps.as_mut_ptr()) as usize;
        for &h in heaps.iter().take(count.min(heaps.len())) {
            if h != 0 && h != def_heap {
                let _ = HeapCompact(h, 0);
            }
        }
        let handle = GetCurrentProcess();
        let _ = SetProcessWorkingSetSizeEx(handle, usize::MAX, usize::MAX, 0);
        let k32 = LoadLibraryA(b"kernel32.dll\0".as_ptr());
        if k32 != 0 {
            let empty_ws = GetProcAddress(k32, b"K32EmptyWorkingSet\0".as_ptr());
            if !empty_ws.is_null() {
                let f: K32EmptyWorkingSetFn = std::mem::transmute(empty_ws);
                let _ = f(handle);
            }
        }
    }
}

#[cfg(not(windows))]
pub fn trim_process_working_set() {}

/// Midias fluindo agora (videos recebidos / enviados). Gravadas pelo app a cada frame (um
/// store atomico) e lidas pela amostra de uso abaixo.
pub static MEDIA_IN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static MEDIA_OUT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Tempo de CPU do processo e o relogio na amostra anterior (100 ns), para a media de CPU.
static LAST_CPU_SAMPLE: std::sync::Mutex<Option<(u64, u64)>> = std::sync::Mutex::new(None);

/// Mensagem `lite-stats` para o servidor: working set (MB), CPU media desde a amostra anterior
/// (% da maquina inteira) e midias recebidas/enviadas. Vai para as metricas do admin, onde e
/// comparada com a meta do projeto (50 MB sem midia, +50 MB por midia).
#[cfg(windows)]
pub fn lite_usage_sample() -> Option<serde_json::Value> {
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
        fn GetProcessTimes(
            process: isize,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn GetSystemTimeAsFileTime(time: *mut FileTime);
    }
    let ft = |t: FileTime| ((t.high as u64) << 32) | t.low as u64;
    unsafe {
        let process = GetCurrentProcess();
        let mut mem = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        if K32GetProcessMemoryInfo(process, &mut mem, mem.cb) == 0 {
            return None;
        }
        let ram_mb = mem.working_set_size as f64 / (1024.0 * 1024.0);

        let (mut c, mut e, mut k, mut u, mut now) = Default::default();
        let mut cpu = None;
        if GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) != 0 {
            GetSystemTimeAsFileTime(&mut now);
            let busy = ft(k) + ft(u);
            let wall = ft(now);
            if let Ok(mut last) = LAST_CPU_SAMPLE.lock() {
                if let Some((last_busy, last_wall)) = *last {
                    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                    let dt = wall.saturating_sub(last_wall) as f64;
                    if dt > 0.0 {
                        let pct = busy.saturating_sub(last_busy) as f64 / dt / cores * 100.0;
                        cpu = Some((pct.clamp(0.0, 100.0) * 10.0).round() / 10.0);
                    }
                }
                *last = Some((busy, wall));
            }
        }
        use std::sync::atomic::Ordering::Relaxed;
        Some(serde_json::json!({
            "type": "lite-stats",
            "ramMb": (ram_mb * 10.0).round() / 10.0,
            "cpu": cpu,
            "mediaIn": MEDIA_IN.load(Relaxed),
            "mediaOut": MEDIA_OUT.load(Relaxed),
        }))
    }
}

#[cfg(not(windows))]
pub fn lite_usage_sample() -> Option<serde_json::Value> {
    None
}

pub fn schedule_delayed_memory_trim(ctx: Option<egui::Context>) {
    if let Some(ref c) = ctx {
        c.request_repaint();
    }
    let _ = std::thread::Builder::new()
        .name("golive-mem-trim".to_string())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(220));
            if let Some(ref c) = ctx {
                c.request_repaint();
            }
            trim_process_working_set();
            std::thread::sleep(std::time::Duration::from_millis(550));
            trim_process_working_set();
        });
}

#[cfg(test)]
mod tests {
    use super::format_autostart_command;
    use std::path::Path;

    #[test]
    fn test_format_autostart_command_quotes_path_with_hidden_flag() {
        let cmd = format_autostart_command(Path::new(r"C:\Users\Joao\AppData\Local\Programs\Go Live Lite - Desktop\golive-lite.exe"));
        assert_eq!(
            cmd,
            r#""C:\Users\Joao\AppData\Local\Programs\Go Live Lite - Desktop\golive-lite.exe" --hidden"#
        );
    }
}
