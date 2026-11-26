#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod look;
mod mods_panel;

use flate2::read::ZlibDecoder;
use std::ffi::c_void;
use std::fs;
use std::io::Read;
use std::mem::{size_of, zeroed};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

static PAYLOAD_CORE_ZLIB: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/golive-lite-core.zlib"));
static PAYLOAD_MODS_ZLIB: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/golive-lite-mods.zlib"));
static PAYLOAD_UNINS_ZLIB: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/golive-lite-unins.zlib"));
const BUILD_MODS_BASE_URL: Option<&str> = option_env!("GOLIVE_LITE_MODS_BASE_URL");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const APP_DISPLAY_NAME: &str = "Go Live - Lite";
const INTERNAL_DIR_NAME: &str = "Go Live Lite - Desktop";
const LEGACY_DIR_NAME: &str = "GoLiveLite";

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "Go Live Lite - Desktop";
const LEGACY_RUN_VALUE_NAME: &str = "GoLiveLite";

const UNINSTALL_SUBKEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Go Live Lite - Desktop";
const LEGACY_UNINSTALL_SUBKEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\GoLiveLite";

const HKEY_CURRENT_USER: isize = -2147483647_i32 as isize;
const KEY_READ: u32 = 0x20019;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;
const ERROR_FILE_NOT_FOUND: i32 = 2;

const WS_OVERLAPPED: u32 = 0x0000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;

const BS_PUSHBUTTON: u32 = 0x0000_0000;
const BS_DEFPUSHBUTTON: u32 = 0x0000_0001;
const BS_AUTOCHECKBOX: u32 = 0x0000_0003;
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_READONLY: u32 = 0x0800;
const SS_LEFT: u32 = 0x0000;
const SS_ENDELLIPSIS: u32 = 0x4000;

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_SETFONT: u32 = 0x0030;
const WM_SETICON: u32 = 0x0080;
const ICON_SMALL: usize = 0;
const ICON_BIG: usize = 1;
const WM_COMMAND: u32 = 0x0111;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const WM_CTLCOLORBTN: u32 = 0x0135;
const WM_PAINT: u32 = 0x000F;
const WM_USER: u32 = 0x0400;
const WM_APP: u32 = 0x8000;

const EM_SETSEL: u32 = 0x00B1;
const EM_SCROLLCARET: u32 = 0x00B7;
const EN_CHANGE: usize = 0x0300;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const BST_UNCHECKED: usize = 0;

const PBM_SETRANGE32: u32 = WM_USER + 6;
const PBM_SETPOS: u32 = WM_USER + 2;

const WM_APP_PROGRESS: u32 = WM_APP + 10;
const WM_APP_STATUS: u32 = WM_APP + 11;
const WM_APP_LOG: u32 = WM_APP + 12;
const WM_APP_DONE: u32 = WM_APP + 13;
const WM_APP_ERROR: u32 = WM_APP + 14;

const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SW_SHOWNORMAL: i32 = 1;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_ICONINFORMATION: u32 = 0x0000_0040;

const IDC_ARROW: usize = 32512;
const IMAGE_ICON: u32 = 1;
const LR_SHARED: u32 = 0x8000;
const CLEARTYPE_QUALITY: u32 = 5;
const FW_NORMAL: i32 = 400;
const GWLP_USERDATA: i32 = -21;

const BIF_RETURNONLYFSDIRS: u32 = 0x0000_0001;
const BIF_NEWDIALOGSTYLE: u32 = 0x0000_0040;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const CLSCTX_INPROC_SERVER: u32 = 0x1;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Control IDs comuns + Paginas 1..5
const IDC_TITLE: usize = 100;
const IDC_SUBTITLE: usize = 101;
const IDC_STEP_BADGE: usize = 102;
const IDC_BACK_BTN: usize = 103;
const IDC_NEXT_BTN: usize = 104;
const IDC_CANCEL_BTN: usize = 105;

// Pagina 1 (Pasta + Desinstalar se existente)
const IDC_P1_LBL: usize = 201;
const IDC_P1_DIR_EDIT: usize = 202;
const IDC_P1_BROWSE_BTN: usize = 203;
const IDC_P1_DETECT_LBL: usize = 204;
const IDC_P1_UNINSTALL_BTN: usize = 205;

// Pagina 2 (Modificacoes)
const IDC_P2_HINT: usize = 301;
const IDC_P2_MOD_BASE: usize = 1000; // 1000.. (dentro do painel com rolagem)
const IDC_P2_MOD_DESC_BASE: usize = 1100; // 1100..
const IDC_P2_SUMMARY: usize = 350;

// Pagina 3 (Download / Progresso + Detalhes)
const IDC_P3_STATUS_LBL: usize = 401;
const IDC_P3_PROGRESS: usize = 402;
const IDC_P3_DETAILS_BTN: usize = 403;
const IDC_P3_DETAILS_EDIT: usize = 404;

// Pagina 4 (Pronto / Configuracoes)
const IDC_P4_SUMMARY: usize = 501;
const IDC_P4_CHK_DESKTOP: usize = 502;
const IDC_P4_CHK_AUTOSTART: usize = 503;
const IDC_P4_CHK_CLOSE_TRAY: usize = 504;
const IDC_P4_CHK_LAUNCH: usize = 505;

// Pagina 5 (Confirmacao de Desinstalacao)
const IDC_P5_DESC: usize = 601;

/// Modificações que o executavel "mods" ja traz compiladas. Sao as unicas aceitas sem internet
/// (para manter as de quem ja tinha instalado) e por `--mods=` quando o catalogo nao carregou.
const BUILTIN_MODS: &[&str] = &["ferramentas", "permissoes_chamada", "perfil", "amigos"];

/// API do Go Live: o catalogo de modificacoes vem dela (o admin cria/edita/desativa sem
/// precisar de um setup novo). `GOLIVE_LITE_API_BASE` troca o servidor para testes.
const DEFAULT_LITE_API_BASE: &str = "https://apigolive.nemtudo.me";

fn lite_api_base() -> String {
    std::env::var("GOLIVE_LITE_API_BASE")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
        .unwrap_or_else(|| DEFAULT_LITE_API_BASE.to_string())
}

// --- Metricas (admin do Go Live: downloads, instalacoes, desinstalacoes, modificacoes) ---
//
// O setup gera o id da instalacao (32 hex) e grava no config.json do app; o app manda o
// mesmo id no register, e o desinstalador avisa a API com ele. Tudo opcional: sem internet
// nada e enviado e nada trava (timeouts curtos, sempre fora da thread da janela).

fn is_valid_install_id(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// O id ja gravado no config.json do app (o antigo "inst-<hex>" vira so o hex, como no app).
fn read_install_id() -> Option<String> {
    let raw = fs::read_to_string(config_file_path()).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let id = val.get("install_id")?.as_str()?.trim();
    let id = id.strip_prefix("inst-").unwrap_or(id);
    is_valid_install_id(id).then(|| id.to_string())
}

fn new_install_id() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let digest = sha256_bytes(format!("{}:{}:{:p}", t, std::process::id(), &t).as_bytes());
    digest[..16].iter().map(|b| format!("{:02x}", b)).collect()
}

/// Garante um id valido no config.json (mesclando, sem mexer no resto) e devolve.
fn ensure_install_id() -> Option<String> {
    if let Some(id) = read_install_id() {
        return Some(id);
    }
    let path = config_file_path();
    let mut obj = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(serde_json::Value::Object(map)) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    let id = new_install_id();
    obj.insert("install_id".into(), serde_json::Value::String(id.clone()));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(obj)).ok()?;
    fs::write(&path, json).ok()?;
    Some(id)
}

/// `GOLIVE_LITE_NO_METRICS=1` (testes, instalacoes de desenvolvimento) nao envia nada.
fn metrics_disabled() -> bool {
    cfg!(test) || std::env::var_os("GOLIVE_LITE_NO_METRICS").is_some()
}

/// Um evento do setup para as metricas (`POST /lite/setup-event`). Bloqueia no maximo ~3 s:
/// chame fora da thread da janela.
fn send_setup_event(event: &str, mods: &[String], install_id: Option<&str>) {
    if metrics_disabled() {
        return;
    }
    let body = serde_json::json!({
        "event": event,
        "mods": mods,
        "installId": install_id,
        "version": env!("CARGO_PKG_VERSION"),
    });
    let url = format!("{}/lite/setup-event", lite_api_base());
    let _ = winhttp_post_json(3_000, &url, &body.to_string());
}

/// O desinstalador avisa a API (so conta ids que ela ja viu). Sem internet, segue sem avisar.
fn send_uninstall_event() {
    if metrics_disabled() {
        return;
    }
    if let Some(id) = read_install_id() {
        let url = format!("{}/lite/uninstall?installId={}", lite_api_base(), id);
        let _ = winhttp_post_json(3_000, &url, "{}");
    }
}

/// Uma modificacao do catalogo (`GET /lite/modifications`).
#[derive(Debug, Clone, PartialEq)]
struct ModEntry {
    id: String,
    name: String,
    description: String,
    version: String,
    /// "[em breve]": aparece desabilitada, nada e baixado.
    coming_soon: bool,
    /// Arquivo extra da modificacao (opcional), salvo em `mods/<id>/<file_name>`.
    download_url: Option<String>,
    sha256: Option<String>,
    file_name: Option<String>,
}

struct Catalog {
    /// `false` = sem internet/API fora: a etapa de modificacoes e pulada.
    online: bool,
    mods: Vec<ModEntry>,
}

static CATALOG: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();

fn is_valid_mod_id(id: &str) -> bool {
    (2..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Le a resposta da API. Entradas invalidas sao puladas (nunca derrubam o setup).
fn parse_remote_catalog(raw: &[u8]) -> Option<Vec<ModEntry>> {
    let val: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let arr = val.get("modifications")?.as_array()?;
    let s = |v: &serde_json::Value, k: &str| -> Option<String> {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
    };
    let mut out = Vec::new();
    for m in arr {
        let Some(id) = s(m, "id").filter(|id| is_valid_mod_id(id)) else {
            continue;
        };
        let status = s(m, "status").unwrap_or_else(|| "active".to_string());
        if status == "disabled" || out.iter().any(|e: &ModEntry| e.id == id) {
            continue;
        }
        let download_url = s(m, "downloadUrl").filter(|u| u.starts_with("https://"));
        let file_name = s(m, "fileName").filter(|f| {
            !f.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) && !f.starts_with('.')
        });
        out.push(ModEntry {
            name: s(m, "name").unwrap_or_else(|| id.clone()).chars().take(40).collect(),
            description: s(m, "description").unwrap_or_default().chars().take(140).collect(),
            version: s(m, "version").unwrap_or_else(|| APP_VERSION.to_string()),
            coming_soon: status == "coming-soon",
            download_url,
            sha256: s(m, "sha256").map(|h| h.to_ascii_lowercase()),
            file_name,
            id,
        });
    }
    Some(out)
}

/// Repositorio do GitHub das releases (definido no build, como no app).
const GITHUB_REPO: Option<&str> = option_env!("GOLIVE_LITE_GITHUB_REPO");

/// O catalogo vem da ultima release do GitHub (`modifications.json`, publicado junto com os
/// executaveis); a API do Go Live fica como reserva.
fn fetch_remote_catalog() -> Option<Vec<ModEntry>> {
    if let Some(repo) = GITHUB_REPO.map(str::trim).filter(|r| r.split('/').count() == 2) {
        let url = format!("https://github.com/{}/releases/latest/download/modifications.json", repo);
        if let Some(mods) = winhttp_download_bytes_with_timeout(4_000, &url, &mut |_, _| {})
            .ok()
            .and_then(|raw| parse_remote_catalog(&raw))
        {
            return Some(mods);
        }
    }
    let url = format!("{}/lite/modifications", lite_api_base());
    let raw = winhttp_download_bytes_with_timeout(4_000, &url, &mut |_, _| {}).ok()?;
    parse_remote_catalog(&raw)
}

/// O catalogo (buscado uma vez; bloqueia no maximo ~4s se ainda estiver buscando).
fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| match fetch_remote_catalog() {
        Some(mods) => Catalog { online: true, mods },
        None => Catalog { online: false, mods: Vec::new() },
    })
}

/// Comeca a buscar o catalogo em segundo plano enquanto a pessoa ve a primeira etapa.
fn prefetch_catalog() {
    let _ = std::thread::Builder::new()
        .name("lite-catalog".into())
        .spawn(|| {
            // Metricas: "abriu o setup" (so com a API no ar; offline nao tenta de novo).
            if catalog().online {
                send_setup_event("open", &[], None);
            }
        });
}

/// Metrica do fim da instalacao: primeira instalacao, atualizacao/reparo (ja existia na
/// pasta), feita sem internet, ou falha. Bloqueia ate ~3 s; chame fora da thread da janela.
fn report_install_result(ok: bool, existed: bool, mods: &[String], no_registry: bool) {
    if !ok {
        send_setup_event("fail", mods, None);
        return;
    }
    let id = if no_registry { None } else { ensure_install_id() };
    let event = if !catalog().online {
        "offline"
    } else if existed {
        "update"
    } else {
        "install"
    };
    send_setup_event(event, mods, id.as_deref());
}

/// Se esta versao do Go Live - Lite consegue instalar a modificacao: ela vem compilada no
/// executavel (`BUILTIN_MODS`) ou tem um arquivo proprio para baixar. Uma ativada no admin antes
/// de existir no app nao faria nada, entao aparece como "em breve" ate o app ter.
fn mod_is_supported(m: &ModEntry) -> bool {
    BUILTIN_MODS.contains(&m.id.as_str()) || m.download_url.is_some()
}

/// Modificações instalaveis agora (nao as "em breve").
fn installable_mods() -> Vec<&'static ModEntry> {
    CATALOG
        .get()
        .map(|c| c.mods.iter().filter(|m| !m.coming_soon && mod_is_supported(m)).collect())
        .unwrap_or_default()
}

fn coming_soon_mods() -> Vec<&'static ModEntry> {
    CATALOG
        .get()
        .map(|c| c.mods.iter().filter(|m| m.coming_soon || !mod_is_supported(m)).collect())
        .unwrap_or_default()
}

fn mod_display_name(id: &str) -> String {
    CATALOG
        .get()
        .and_then(|c| c.mods.iter().find(|m| m.id == id))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// Limites da etapa 2 (a lista rola; o limite so protege contra um catalogo absurdo).
const MAX_PAGE2_MODS: usize = 64;
const MAX_PAGE2_SOON: usize = 64;
const IDC_P2_PANEL: usize = 358;
const IDC_P2_SOON_LBL: usize = 359;
const IDC_P2_SOON_BASE: usize = 360; // um controle so: a lista corrida "Em breve"

#[repr(C)]
struct InitCommonControlsEx {
    dw_size: u32,
    dw_icc: u32,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt_x: i32,
    pt_y: i32,
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
struct BrowseInfoW {
    hwnd_owner: isize,
    pidl_root: *const c_void,
    psz_display_name: *mut u16,
    lpsz_title: *const u16,
    ul_flags: u32,
    lpfn: *const c_void,
    l_param: isize,
    i_image: i32,
}

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const CLSID_SHELL_LINK: Guid = Guid {
    data1: 0x0002_1401,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_ISHELL_LINK_W: Guid = Guid {
    data1: 0x0002_14F9,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_IPERSIST_FILE: Guid = Guid {
    data1: 0x0000_010B,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(lp_module_name: *const u16) -> isize;
}

#[link(name = "comctl32")]
extern "system" {
    fn InitCommonControlsEx(picce: *const InitCommonControlsEx) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateFontW(
        c_height: i32,
        c_width: i32,
        c_escapement: i32,
        c_orientation: i32,
        c_weight: i32,
        b_italic: u32,
        b_underline: u32,
        b_strike_out: u32,
        i_char_set: u32,
        i_out_precision: u32,
        i_clip_precision: u32,
        i_quality: u32,
        i_pitch_and_family: u32,
        psz_face_name: *const u16,
    ) -> isize;
    fn DeleteObject(ho: isize) -> i32;
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
    fn DefWindowProcW(h_wnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize;
    fn DestroyWindow(h_wnd: isize) -> i32;
    fn PostQuitMessage(n_exit_code: i32);
    fn GetMessageW(
        lp_msg: *mut Msg,
        h_wnd: isize,
        w_msg_filter_min: u32,
        w_msg_filter_max: u32,
    ) -> i32;
    fn TranslateMessage(lp_msg: *const Msg) -> i32;
    fn DispatchMessageW(lp_msg: *const Msg) -> isize;
    fn IsDialogMessageW(h_dlg: isize, lp_msg: *const Msg) -> i32;
    fn ShowWindow(h_wnd: isize, n_cmd_show: i32) -> i32;
    fn UpdateWindow(h_wnd: isize) -> i32;
    fn SendMessageW(h_wnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize;
    fn PostMessageW(h_wnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    fn GetDlgItem(h_dlg: isize, n_id_dlg_item: i32) -> isize;
    fn GetWindowTextW(h_wnd: isize, lp_string: *mut u16, n_max_count: i32) -> i32;
    fn SetWindowTextW(h_wnd: isize, lp_string: *const u16) -> i32;
    fn GetSystemMetrics(n_index: i32) -> i32;
    fn LoadCursorW(h_instance: isize, lp_cursor_name: *const u16) -> isize;
    fn LoadImageW(
        h_inst: isize,
        name: *const u16,
        img_type: u32,
        cx: i32,
        cy: i32,
        fu_load: u32,
    ) -> isize;
    fn DestroyIcon(h_icon: isize) -> i32;
    fn MessageBoxW(h_wnd: isize, lp_text: *const u16, lp_caption: *const u16, u_type: u32) -> i32;
    fn SetWindowLongPtrW(h_wnd: isize, n_index: i32, dw_new_long: isize) -> isize;
    fn GetWindowLongPtrW(h_wnd: isize, n_index: i32) -> isize;
    fn EnableWindow(h_wnd: isize, b_enable: i32) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn SHBrowseForFolderW(lpbi: *mut BrowseInfoW) -> *mut c_void;
    fn SHGetPathFromIDListW(pidl: *const c_void, psz_path: *mut u16) -> i32;
    fn SHChangeNotify(w_event_id: i32, u_flags: u32, dw_item1: *const c_void, dw_item2: *const c_void);
}

const SHCNE_UPDATEITEM: i32 = 0x0000_2000;
const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
const SHCNF_IDLIST: u32 = 0x0000;
const SHCNF_PATHW: u32 = 0x0005;

#[repr(C)]
#[allow(non_snake_case)]
struct UrlComponentsW {
    dwStructSize: u32,
    lpszScheme: *mut u16,
    dwSchemeLength: u32,
    nScheme: i32,
    lpszHostName: *mut u16,
    dwHostNameLength: u32,
    nPort: u16,
    lpszUserName: *mut u16,
    dwUserNameLength: u32,
    lpszPassword: *mut u16,
    dwPasswordLength: u32,
    lpszUrlPath: *mut u16,
    dwUrlPathLength: u32,
    lpszExtraInfo: *mut u16,
    dwExtraInfoLength: u32,
}

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        psz_agent_w: *const u16,
        dw_access_type: u32,
        psz_proxy_w: *const u16,
        psz_proxy_bypass_w: *const u16,
        dw_flags: u32,
    ) -> *mut c_void;
    fn WinHttpConnect(
        h_session: *mut c_void,
        pswz_server_name: *const u16,
        n_server_port: u16,
        dw_reserved: u32,
    ) -> *mut c_void;
    fn WinHttpOpenRequest(
        h_connect: *mut c_void,
        pwsz_verb: *const u16,
        pwsz_object_name: *const u16,
        pwsz_version: *const u16,
        pwsz_referrer: *const u16,
        ppwsz_accept_types: *const *const u16,
        dw_flags: u32,
    ) -> *mut c_void;
    fn WinHttpSendRequest(
        h_request: *mut c_void,
        lpsz_headers: *const u16,
        dw_headers_length: u32,
        lp_optional: *const c_void,
        dw_optional_length: u32,
        dw_total_length: u32,
        dw_context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(h_request: *mut c_void, lp_reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        h_request: *mut c_void,
        dw_info_level: u32,
        pwsz_name: *const u16,
        lp_buffer: *mut c_void,
        lpdw_buffer_length: *mut u32,
        lpdw_index: *mut u32,
    ) -> i32;
    fn WinHttpQueryDataAvailable(h_request: *mut c_void, lpdw_number_of_bytes_available: *mut u32) -> i32;
    fn WinHttpReadData(
        h_request: *mut c_void,
        lp_buffer: *mut c_void,
        dw_number_of_bytes_to_read: u32,
        lpdw_number_of_bytes_read: *mut u32,
    ) -> i32;
    fn WinHttpCloseHandle(h_internet: *mut c_void) -> i32;
    fn WinHttpSetTimeouts(
        h_internet: *mut c_void,
        n_resolve_timeout: i32,
        n_connect_timeout: i32,
        n_send_timeout: i32,
        n_receive_timeout: i32,
    ) -> i32;
    fn WinHttpCrackUrl(
        pwsz_url: *const u16,
        dw_url_length: u32,
        dw_flags: u32,
        lp_url_components: *mut UrlComponentsW,
    ) -> i32;
}

const WINHTTP_ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
const WINHTTP_QUERY_CONTENT_LENGTH: u32 = 5;
const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const INTERNET_SCHEME_HTTPS: i32 = 2;

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(pv_reserved: *mut c_void, dw_co_init: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        rclsid: *const Guid,
        p_unk_outer: *mut c_void,
        dw_cls_context: u32,
        riid: *const Guid,
        ppv: *mut *mut c_void,
    ) -> i32;
    fn CoTaskMemFree(pv: *mut c_void);
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        h_key: isize,
        lp_sub_key: *const u16,
        ul_options: u32,
        sam_desired: u32,
        phk_result: *mut isize,
    ) -> i32;
    fn RegQueryValueExW(
        h_key: isize,
        lp_value_name: *const u16,
        lp_reserved: *mut u32,
        lp_type: *mut u32,
        lp_data: *mut u8,
        lpcb_data: *mut u32,
    ) -> i32;
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
    fn RegDeleteKeyW(h_key: isize, lp_sub_key: *const u16) -> i32;
    fn RegCloseKey(h_key: isize) -> i32;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupOptions {
    uninstall: bool,
    silent: bool,
    no_registry: bool,
    install_dir: PathBuf,
    desktop_shortcut: bool,
    start_with_windows: bool,
    minimize_to_tray_on_close: bool,
    launch_after: bool,
    selected_modifications: Vec<String>,
    mods_specified: bool,
}

impl Default for SetupOptions {
    fn default() -> Self {
        Self {
            uninstall: false,
            silent: false,
            no_registry: false,
            install_dir: default_install_dir(),
            desktop_shortcut: true,
            start_with_windows: false,
            minimize_to_tray_on_close: true,
            launch_after: true,
            selected_modifications: Vec::new(),
            mods_specified: false,
        }
    }
}

fn default_install_dir() -> PathBuf {
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        if !local_app.trim().is_empty() {
            return PathBuf::from(local_app)
                .join("Programs")
                .join(INTERNAL_DIR_NAME);
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.trim().is_empty() {
            return PathBuf::from(profile)
                .join("AppData")
                .join("Local")
                .join("Programs")
                .join(INTERNAL_DIR_NAME);
        }
    }
    PathBuf::from(INTERNAL_DIR_NAME)
}

fn parse_bool_flag(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn normalize_mod_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for raw in ids {
        let trimmed = raw.trim().to_ascii_lowercase();
        // Aceita as embutidas e as instalaveis do catalogo da API (se ja carregou).
        let known = BUILTIN_MODS.contains(&trimmed.as_str())
            || installable_mods().iter().any(|m| m.id == trimmed);
        if known && !out.contains(&trimmed) {
            out.push(trimmed);
        }
    }
    out
}

fn parse_setup_args(args: &[String], current_exe: Option<&Path>) -> SetupOptions {
    let mut opts = SetupOptions::default();

    let mut forced_install = false;
    let mut forced_uninstall = false;
    let mut dir_overridden = false;

    if let Some(exe) = current_exe {
        if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
            if name.eq_ignore_ascii_case("unins000.exe") {
                opts.uninstall = true;
                if let Some(parent) = exe.parent() {
                    if !parent.as_os_str().is_empty() {
                        opts.install_dir = parent.to_path_buf();
                    }
                }
            }
        }
    }

    for arg in args {
        let a = arg.trim();
        if a.eq_ignore_ascii_case("--uninstall") || a.eq_ignore_ascii_case("/uninstall") {
            forced_uninstall = true;
        } else if a.eq_ignore_ascii_case("--install") {
            forced_install = true;
        } else if a.eq_ignore_ascii_case("--silent") || a.eq_ignore_ascii_case("/S") {
            opts.silent = true;
        } else if a.eq_ignore_ascii_case("--no-registry") {
            opts.no_registry = true;
        } else if let Some(rest) = a.strip_prefix("--dir=") {
            let trimmed = rest.trim().trim_matches('"');
            if !trimmed.is_empty() {
                opts.install_dir = PathBuf::from(trimmed);
                dir_overridden = true;
            }
        } else if let Some(rest) = a.strip_prefix("--mods=") {
            opts.mods_specified = true;
            opts.selected_modifications = normalize_mod_ids(rest.split(',').map(|s| s.to_string()));
        } else if let Some(rest) = a.strip_prefix("--autostart=") {
            if let Some(b) = parse_bool_flag(rest) {
                opts.start_with_windows = b;
            }
        } else if let Some(rest) = a.strip_prefix("--close-to-tray=") {
            if let Some(b) = parse_bool_flag(rest) {
                opts.minimize_to_tray_on_close = b;
            }
        } else if let Some(rest) = a.strip_prefix("--desktop-shortcut=") {
            if let Some(b) = parse_bool_flag(rest) {
                opts.desktop_shortcut = b;
            }
        } else if let Some(rest) = a.strip_prefix("--launch=") {
            if let Some(b) = parse_bool_flag(rest) {
                opts.launch_after = b;
            }
        }
    }

    if forced_install {
        opts.uninstall = false;
        if !dir_overridden {
            opts.install_dir = default_install_dir();
        }
    } else if forced_uninstall {
        opts.uninstall = true;
    }

    if opts.silent && !args.iter().any(|a| a.starts_with("--launch=")) {
        opts.launch_after = false;
    }

    opts
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_slice_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn decompress_zlib_bytes(compressed: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = ZlibDecoder::new(compressed);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| format!("Falha ao descompactar binario embutido: {}", e))?;
    Ok(out)
}

fn merge_config_settings(
    existing_json: &str,
    start_with_windows: bool,
    minimize_to_tray_on_close: bool,
    enabled_modifications: &[String],
) -> String {
    let mut obj = match serde_json::from_str::<serde_json::Value>(existing_json) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    obj.insert(
        "start_with_windows".to_string(),
        serde_json::Value::Bool(start_with_windows),
    );
    obj.insert(
        "minimize_to_tray_on_close".to_string(),
        serde_json::Value::Bool(minimize_to_tray_on_close),
    );
    obj.insert(
        "enabled_modifications".to_string(),
        serde_json::Value::Array(
            enabled_modifications
                .iter()
                .map(|m| serde_json::Value::String(m.clone()))
                .collect(),
        ),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(obj))
        .unwrap_or_else(|_| "{}".to_string())
}

fn config_file_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.trim().is_empty() {
            return PathBuf::from(appdata)
                .join(INTERNAL_DIR_NAME)
                .join("config.json");
        }
    }
    PathBuf::from(INTERNAL_DIR_NAME).join("config.json")
}

fn legacy_config_file_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|appdata| {
            PathBuf::from(appdata)
                .join(LEGACY_DIR_NAME)
                .join("config.json")
        })
}

fn read_existing_config_flags() -> Option<(bool, bool)> {
    let raw = fs::read_to_string(config_file_path())
        .ok()
        .or_else(|| legacy_config_file_path().and_then(|p| fs::read_to_string(p).ok()))?;
    let val = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let auto = val
        .get("start_with_windows")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tray = val
        .get("minimize_to_tray_on_close")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some((auto, tray))
}

fn start_menu_shortcut_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    if appdata.trim().is_empty() {
        return None;
    }
    Some(
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Go Live - Lite.lnk"),
    )
}

fn legacy_start_menu_shortcut_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    if appdata.trim().is_empty() {
        return None;
    }
    Some(
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("GoLive Lite.lnk"),
    )
}

fn desktop_shortcut_path() -> Option<PathBuf> {
    let profile = std::env::var("USERPROFILE").ok()?;
    if profile.trim().is_empty() {
        return None;
    }
    Some(
        PathBuf::from(profile)
            .join("Desktop")
            .join("Go Live - Lite.lnk"),
    )
}

fn legacy_desktop_shortcut_path() -> Option<PathBuf> {
    let profile = std::env::var("USERPROFILE").ok()?;
    if profile.trim().is_empty() {
        return None;
    }
    Some(
        PathBuf::from(profile)
            .join("Desktop")
            .join("GoLive Lite.lnk"),
    )
}

fn cleanup_legacy_shortcuts() {
    if let Some(appdata) = std::env::var("APPDATA").ok().filter(|s| !s.trim().is_empty()) {
        let dir = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        for name in ["GoLive Lite.lnk", "GoLive - Lite.lnk", "Go Live Lite.lnk"] {
            let _ = fs::remove_file(dir.join(name));
        }
    }
    if let Some(profile) = std::env::var("USERPROFILE").ok().filter(|s| !s.trim().is_empty()) {
        let dir = PathBuf::from(profile).join("Desktop");
        for name in ["GoLive Lite.lnk", "GoLive - Lite.lnk", "Go Live Lite.lnk"] {
            let _ = fs::remove_file(dir.join(name));
        }
    }
}

fn notify_shell_icon_update(paths: &[&Path]) {
    unsafe {
        for p in paths {
            let wide = to_wide_null(&p.to_string_lossy());
            SHChangeNotify(
                SHCNE_UPDATEITEM,
                SHCNF_PATHW,
                wide.as_ptr() as *const c_void,
                null(),
            );
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, null(), null());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadKind {
    Core,
    Mods,
}

fn select_payload_kind(selected_mods: &[String]) -> PayloadKind {
    if selected_mods.is_empty() {
        PayloadKind::Core
    } else {
        PayloadKind::Mods
    }
}

fn build_download_artifact_url(base_url: &str, artifact_name: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    format!("{}/{}", trimmed, artifact_name)
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut padded = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (idx, word) in h.iter().enumerate() {
        out[idx * 4..idx * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256_bytes(data);
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Download dos arquivos grandes (executavel/modificacoes): tempo de sobra.
fn winhttp_download_bytes(
    url: &str,
    on_chunk: &mut dyn FnMut(usize, Option<u64>),
) -> Result<Vec<u8>, String> {
    winhttp_download_bytes_with_timeout(30_000, url, on_chunk)
}

fn winhttp_download_bytes_with_timeout(
    timeout_ms: i32,
    url: &str,
    on_chunk: &mut dyn FnMut(usize, Option<u64>),
) -> Result<Vec<u8>, String> {
    winhttp_request("GET", timeout_ms, url, None, on_chunk)
}

/// POST de um JSON pequeno (metricas do setup). A resposta e ignorada; so o erro de rede volta.
fn winhttp_post_json(timeout_ms: i32, url: &str, body: &str) -> Result<(), String> {
    winhttp_request("POST", timeout_ms, url, Some(body.as_bytes()), &mut |_, _| {}).map(|_| ())
}

fn winhttp_request(
    verb: &str,
    timeout_ms: i32,
    url: &str,
    body: Option<&[u8]>,
    on_chunk: &mut dyn FnMut(usize, Option<u64>),
) -> Result<Vec<u8>, String> {
    let url_w = to_wide_null(url);
    let mut host_buf = vec![0u16; 512];
    let mut path_buf = vec![0u16; 2048];
    let mut extra_buf = vec![0u16; 512];
    let mut comps = UrlComponentsW {
        dwStructSize: size_of::<UrlComponentsW>() as u32,
        lpszScheme: null_mut(),
        dwSchemeLength: 0,
        nScheme: 0,
        lpszHostName: host_buf.as_mut_ptr(),
        dwHostNameLength: host_buf.len() as u32,
        nPort: 0,
        lpszUserName: null_mut(),
        dwUserNameLength: 0,
        lpszPassword: null_mut(),
        dwPasswordLength: 0,
        lpszUrlPath: path_buf.as_mut_ptr(),
        dwUrlPathLength: path_buf.len() as u32,
        lpszExtraInfo: extra_buf.as_mut_ptr(),
        dwExtraInfoLength: extra_buf.len() as u32,
    };

    unsafe {
        if WinHttpCrackUrl(url_w.as_ptr(), 0, 0, &mut comps) == 0 {
            return Err(format!("URL de download invalida: {}", url));
        }
        let host = String::from_utf16_lossy(&host_buf[..comps.dwHostNameLength as usize]);
        let mut path = String::from_utf16_lossy(&path_buf[..comps.dwUrlPathLength as usize]);
        if comps.dwExtraInfoLength > 0 {
            path.push_str(&String::from_utf16_lossy(
                &extra_buf[..comps.dwExtraInfoLength as usize],
            ));
        }
        if path.is_empty() {
            path.push('/');
        }
        let host_w = to_wide_null(&host);
        let path_w = to_wide_null(&path);
        let agent_w = to_wide_null("GoLiveLiteSetup/1.0");
        let verb_w = to_wide_null(verb);
        let flags = if comps.nScheme == INTERNET_SCHEME_HTTPS {
            WINHTTP_FLAG_SECURE
        } else {
            0
        };

        let h_session = WinHttpOpen(
            agent_w.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            null(),
            null(),
            0,
        );
        if h_session.is_null() {
            return Err("Falha ao inicializar WinHTTP.".to_string());
        }
        // Sem isso o WinHTTP espera ate 60s para conectar: sem internet o setup "congelava".
        let _ = WinHttpSetTimeouts(h_session, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
        let h_conn = WinHttpConnect(h_session, host_w.as_ptr(), comps.nPort, 0);
        if h_conn.is_null() {
            let _ = WinHttpCloseHandle(h_session);
            return Err(format!("Falha ao conectar em {}.", host));
        }
        let h_req = WinHttpOpenRequest(
            h_conn,
            verb_w.as_ptr(),
            path_w.as_ptr(),
            null(),
            null(),
            null(),
            flags,
        );
        if h_req.is_null() {
            let _ = WinHttpCloseHandle(h_conn);
            let _ = WinHttpCloseHandle(h_session);
            return Err("Falha ao abrir requisicao HTTP.".to_string());
        }

        let res = (|| -> Result<Vec<u8>, String> {
            let headers_w = to_wide_null("Content-Type: application/json\r\n");
            let (hdr_ptr, hdr_len, body_ptr, body_len) = match body {
                Some(b) => (
                    headers_w.as_ptr(),
                    u32::MAX, // -1: cabecalho terminado em NUL
                    b.as_ptr() as *const c_void,
                    b.len() as u32,
                ),
                None => (null(), 0, null(), 0),
            };
            if WinHttpSendRequest(h_req, hdr_ptr, hdr_len, body_ptr, body_len, body_len, 0) == 0
                || WinHttpReceiveResponse(h_req, null_mut()) == 0
            {
                return Err(format!("Falha na requisicao para {}", url));
            }
            let mut status: u32 = 0;
            let mut status_sz = size_of::<u32>() as u32;
            let _ = WinHttpQueryHeaders(
                h_req,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut status as *mut u32) as *mut c_void,
                &mut status_sz,
                null_mut(),
            );
            if status >= 400 {
                return Err(format!("Servidor retornou HTTP {} ao baixar {}", status, url));
            }

            let mut content_len: u32 = 0;
            let mut cl_sz = size_of::<u32>() as u32;
            let total_opt = if WinHttpQueryHeaders(
                h_req,
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut content_len as *mut u32) as *mut c_void,
                &mut cl_sz,
                null_mut(),
            ) != 0
                && content_len > 0
            {
                Some(content_len as u64)
            } else {
                None
            };

            let mut out = Vec::new();
            let mut buf = [0u8; 32 * 1024];
            loop {
                let mut avail: u32 = 0;
                if WinHttpQueryDataAvailable(h_req, &mut avail) == 0 {
                    return Err("Falha ao consultar dados HTTP disponiveis.".to_string());
                }
                if avail == 0 {
                    break;
                }
                let to_read = (avail as usize).min(buf.len()) as u32;
                let mut read_n: u32 = 0;
                if WinHttpReadData(h_req, buf.as_mut_ptr() as *mut c_void, to_read, &mut read_n) == 0
                {
                    return Err("Falha ao ler pacote HTTP.".to_string());
                }
                if read_n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..read_n as usize]);
                on_chunk(out.len(), total_opt);
            }
            Ok(out)
        })();

        let _ = WinHttpCloseHandle(h_req);
        let _ = WinHttpCloseHandle(h_conn);
        let _ = WinHttpCloseHandle(h_session);
        res
    }
}

fn reg_get_string(subkey: &str, name: &str) -> Option<String> {
    let subkey_w = to_wide_null(subkey);
    let name_w = to_wide_null(name);
    unsafe {
        let mut hkey: isize = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut bytes = (buf.len() * size_of::<u16>()) as u32;
        let mut reg_type = 0u32;
        let rc = RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            null_mut(),
            &mut reg_type,
            buf.as_mut_ptr() as *mut u8,
            &mut bytes,
        );
        let _ = RegCloseKey(hkey);
        if rc == 0 && reg_type == REG_SZ {
            let s = wide_slice_to_string(&buf);
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn reg_set_string(subkey: &str, name: &str, value: &str) -> Result<(), String> {
    let subkey_w = to_wide_null(subkey);
    let name_w = to_wide_null(name);
    let val_w = to_wide_null(value);
    unsafe {
        let mut hkey: isize = 0;
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut hkey,
            null_mut(),
        );
        if rc != 0 {
            return Err(format!("RegCreateKeyExW falhou ({})", rc));
        }
        let byte_len = (val_w.len() * size_of::<u16>()) as u32;
        let rc2 = RegSetValueExW(
            hkey,
            name_w.as_ptr(),
            0,
            REG_SZ,
            val_w.as_ptr() as *const u8,
            byte_len,
        );
        let _ = RegCloseKey(hkey);
        if rc2 != 0 {
            return Err(format!("RegSetValueExW falhou ({})", rc2));
        }
    }
    Ok(())
}

fn reg_set_dword(subkey: &str, name: &str, value: u32) -> Result<(), String> {
    let subkey_w = to_wide_null(subkey);
    let name_w = to_wide_null(name);
    let bytes = value.to_le_bytes();
    unsafe {
        let mut hkey: isize = 0;
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut hkey,
            null_mut(),
        );
        if rc != 0 {
            return Err(format!("RegCreateKeyExW falhou ({})", rc));
        }
        let rc2 = RegSetValueExW(hkey, name_w.as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4);
        let _ = RegCloseKey(hkey);
        if rc2 != 0 {
            return Err(format!("RegSetValueExW(DWORD) falhou ({})", rc2));
        }
    }
    Ok(())
}

fn reg_delete_value(subkey: &str, name: &str) {
    let subkey_w = to_wide_null(subkey);
    let name_w = to_wide_null(name);
    unsafe {
        let mut hkey: isize = 0;
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut hkey,
            null_mut(),
        );
        if rc == 0 {
            let rc2 = RegDeleteValueW(hkey, name_w.as_ptr());
            let _ = RegCloseKey(hkey);
            if rc2 != 0 && rc2 != ERROR_FILE_NOT_FOUND {
                let _ = rc2;
            }
        }
    }
}

fn reg_delete_key(subkey: &str) {
    let subkey_w = to_wide_null(subkey);
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, subkey_w.as_ptr());
    }
}

#[inline]
unsafe fn com_vtable(obj: *mut c_void) -> *const *const c_void {
    *(obj as *const *const *const c_void)
}

unsafe fn com_release(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    let vtbl = com_vtable(obj);
    let func: extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtbl.add(2));
    func(obj);
}

fn create_shortcut(
    lnk_path: &Path,
    target_exe: &Path,
    work_dir: &Path,
    desc: &str,
) -> Result<(), String> {
    if let Some(parent) = lnk_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar pasta do atalho {}: {}", parent.display(), e))?;
    }

    let target_w = to_wide_null(&target_exe.to_string_lossy());
    let work_w = to_wide_null(&work_dir.to_string_lossy());
    let desc_w = to_wide_null(desc);
    let lnk_w = to_wide_null(&lnk_path.to_string_lossy());

    unsafe {
        let hr_init = CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED);
        let did_init = hr_init >= 0;

        let mut shell_link: *mut c_void = null_mut();
        let hr = CoCreateInstance(
            &CLSID_SHELL_LINK,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_ISHELL_LINK_W,
            &mut shell_link,
        );
        if hr < 0 || shell_link.is_null() {
            if did_init {
                CoUninitialize();
            }
            return Err(format!("CoCreateInstance(IShellLinkW) falhou: 0x{:08X}", hr));
        }

        let vtbl = com_vtable(shell_link);
        let set_path: extern "system" fn(*mut c_void, *const u16) -> i32 =
            std::mem::transmute(*vtbl.add(20));
        let set_work_dir: extern "system" fn(*mut c_void, *const u16) -> i32 =
            std::mem::transmute(*vtbl.add(9));
        let set_desc: extern "system" fn(*mut c_void, *const u16) -> i32 =
            std::mem::transmute(*vtbl.add(7));
        let set_icon: extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
            std::mem::transmute(*vtbl.add(17));
        let query_iface: extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 =
            std::mem::transmute(*vtbl.add(0));

        let _ = set_path(shell_link, target_w.as_ptr());
        let _ = set_work_dir(shell_link, work_w.as_ptr());
        let _ = set_desc(shell_link, desc_w.as_ptr());
        let _ = set_icon(shell_link, target_w.as_ptr(), 0);

        let mut persist_file: *mut c_void = null_mut();
        let hr_qi = query_iface(shell_link, &IID_IPERSIST_FILE, &mut persist_file);
        let save_res = if hr_qi >= 0 && !persist_file.is_null() {
            let pf_vtbl = com_vtable(persist_file);
            let save_fn: extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
                std::mem::transmute(*pf_vtbl.add(6));
            let hr_save = save_fn(persist_file, lnk_w.as_ptr(), 1);
            com_release(persist_file);
            if hr_save >= 0 {
                Ok(())
            } else {
                Err(format!("IPersistFile::Save falhou: 0x{:08X}", hr_save))
            }
        } else {
            Err(format!(
                "QueryInterface(IPersistFile) falhou: 0x{:08X}",
                hr_qi
            ))
        };

        com_release(shell_link);
        if did_init {
            CoUninitialize();
        }
        save_res
    }
}

fn is_existing_install_in_dir(dir: &Path) -> bool {
    dir.join("golive-lite.exe").is_file()
}

fn detect_existing_install_dir() -> Option<PathBuf> {
    for sub in [UNINSTALL_SUBKEY, LEGACY_UNINSTALL_SUBKEY] {
        if let Some(loc) = reg_get_string(sub, "InstallLocation") {
            let p = PathBuf::from(loc.trim());
            if is_existing_install_in_dir(&p) {
                return Some(p);
            }
        }
    }
    let def = default_install_dir();
    if is_existing_install_in_dir(&def) {
        return Some(def);
    }
    None
}

fn read_installed_modifications(install_dir: &Path) -> Vec<String> {
    let manifest_path = install_dir.join("modifications.json");
    if let Ok(raw) = fs::read_to_string(manifest_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(arr) = val.get("installed").and_then(|v| v.as_array()) {
                return arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| is_valid_mod_id(s))
                    .fold(Vec::new(), |mut acc, s| {
                        if !acc.contains(&s) {
                            acc.push(s);
                        }
                        acc
                    });
            }
        }
    }
    installed_mod_files(&install_dir.join("mods"))
}

/// Ids com `mods/<id>.mod.json` no disco (qualquer modificacao, mesmo uma que saiu do catalogo).
fn installed_mod_files(mods_dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(rd) = fs::read_dir(mods_dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(".mod.json") {
                if is_valid_mod_id(id) && !found.iter().any(|f| f == id) {
                    found.push(id.to_string());
                }
            }
        }
    }
    found.sort();
    found
}

/// Baixa o arquivo extra de uma modificacao (se o catalogo tiver um) e confere o SHA-256.
fn install_mod_payload(
    mods_dir: &Path,
    entry: &ModEntry,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let Some(ref url) = entry.download_url else {
        return Ok(());
    };
    let file_name = entry.file_name.clone().unwrap_or_else(|| format!("{}.bin", entry.id));
    let dir = mods_dir.join(&entry.id);
    fs::create_dir_all(&dir).map_err(|e| format!("Falha ao criar {}: {}", dir.display(), e))?;
    log(&format!("[Modificacao] Baixando {} ({})...", entry.name, url));
    let bytes = winhttp_download_bytes(url, &mut |_, _| {})
        .map_err(|e| format!("Falha ao baixar a modificação {}: {}", entry.name, e))?;
    if let Some(ref want) = entry.sha256 {
        let got = sha256_hex(&bytes);
        if &got != want {
            return Err(format!(
                "Arquivo da modificação {} corrompido (SHA-256 {} esperado {}).",
                entry.name, got, want
            ));
        }
    }
    let target = dir.join(&file_name);
    fs::write(&target, &bytes).map_err(|e| format!("Falha ao salvar {}: {}", target.display(), e))?;
    log(&format!("[Modificacao] {} ({} KB) salvo.", file_name, bytes.len() / 1024));
    Ok(())
}

fn write_modifications_state(
    install_dir: &Path,
    selected: &[String],
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let normalized = normalize_mod_ids(selected.iter().cloned());
    let mods_dir = install_dir.join("mods");
    if !normalized.is_empty() {
        fs::create_dir_all(&mods_dir).map_err(|e| {
            format!(
                "Falha ao criar pasta de modificações {}: {}",
                mods_dir.display(),
                e
            )
        })?;
    }

    // Tudo o que ja estava no disco + tudo o que foi pedido: o que nao foi pedido sai.
    let mut ids = installed_mod_files(&mods_dir);
    for id in &normalized {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }

    for id in &ids {
        let mod_file = mods_dir.join(format!("{}.mod.json", id));
        let want = normalized.contains(id);
        let had = mod_file.is_file();
        let name = mod_display_name(id);

        if want {
            let entry = CATALOG.get().and_then(|c| c.mods.iter().find(|m| &m.id == id));
            if let Some(entry) = entry {
                install_mod_payload(&mods_dir, entry, log)?;
            }
            let body = serde_json::json!({
                "id": id,
                "name": name,
                "version": entry.map(|e| e.version.as_str()).unwrap_or(APP_VERSION),
                "installed": true,
            });
            let json = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string());
            fs::write(&mod_file, json)
                .map_err(|e| format!("Falha ao registrar modificação {}: {}", name, e))?;
            log(&format!(
                "[Modificacao] {} {}.",
                name,
                if had { "atualizada" } else { "instalada" }
            ));
        } else if had {
            fs::remove_file(&mod_file)
                .map_err(|e| format!("Falha ao remover modificação {}: {}", name, e))?;
            let _ = fs::remove_dir_all(mods_dir.join(id));
            log(&format!("[Modificacao] {} removida.", name));
        }
    }

    if normalized.is_empty() && mods_dir.is_dir() {
        let _ = fs::remove_dir(&mods_dir);
    }

    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "app": INTERNAL_DIR_NAME,
        "installed": normalized,
    });
    let manifest_txt =
        serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());
    fs::write(install_dir.join("modifications.json"), manifest_txt).map_err(|e| {
        format!(
            "Falha ao salvar manifesto de modificações: {}",
            e
        )
    })?;
    Ok(())
}

fn perform_core_files_install(
    install_dir: &Path,
    selected_mods: &[String],
    progress: &mut dyn FnMut(u32, &str, &str),
) -> Result<(PathBuf, u64, bool), String> {
    progress(
        8,
        "Preparando pasta de instalação...",
        &format!("Destino: {}", install_dir.display()),
    );
    fs::create_dir_all(install_dir).map_err(|e| {
        format!(
            "Nao foi possivel criar a pasta {}: {}",
            install_dir.display(),
            e
        )
    })?;

    let payload_kind = select_payload_kind(selected_mods);
    let (artifact_name, embedded_zlib, payload_label) = match payload_kind {
        PayloadKind::Core => (
            "golive-lite-core.exe",
            PAYLOAD_CORE_ZLIB,
            "nucleo Ultra-Leve (0 modificações)",
        ),
        PayloadKind::Mods => (
            "golive-lite-mods.exe",
            PAYLOAD_MODS_ZLIB,
            "nucleo + suporte as modificações selecionadas",
        ),
    };

    let embedded_raw = if !embedded_zlib.is_empty() {
        decompress_zlib_bytes(embedded_zlib)?
    } else {
        Vec::new()
    };

    let base_url_opt = std::env::var("GOLIVE_LITE_MODS_BASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            BUILD_MODS_BASE_URL
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    let raw_exe = if embedded_raw.is_empty() {
        let base_url = base_url_opt.ok_or_else(|| {
            format!(
                "Este instalador online precisa de URL de download (GOLIVE_LITE_MODS_BASE_URL) para obter {}.",
                artifact_name
            )
        })?;
        let url = build_download_artifact_url(&base_url, artifact_name);
        progress(
            20,
            "Baixando arquivos selecionados...",
            &format!("Baixando {} ({})...", url, payload_label),
        );
        let downloaded = winhttp_download_bytes(&url, &mut |done, total| {
            let pct = if let Some(tot) = total.filter(|&t| t > 0) {
                20 + ((done as u64 * 35) / tot).min(35) as u32
            } else {
                35
            };
            progress(pct, "Baixando arquivos selecionados...", "");
        })?;
        if let Ok(sums_bytes) = winhttp_download_bytes(
            &build_download_artifact_url(&base_url, "SHA256SUMS.txt"),
            &mut |_, _| {},
        ) {
            let sums_txt = String::from_utf8_lossy(&sums_bytes);
            if let Some(expected_hash) = sums_txt.lines().find_map(|line| {
                let mut parts = line.split_whitespace();
                let hash = parts.next()?;
                let name = parts.next()?.trim_start_matches('*');
                if name.eq_ignore_ascii_case(artifact_name) {
                    Some(hash.to_ascii_lowercase())
                } else {
                    None
                }
            }) {
                let got_hash = sha256_hex(&downloaded);
                if got_hash != expected_hash {
                    return Err(format!(
                        "Falha na verificacao SHA-256 de {}: esperado {}, obtido {}.",
                        artifact_name, expected_hash, got_hash
                    ));
                }
                progress(
                    56,
                    "Verificando integridade...",
                    &format!("SHA-256 verificado para {}.", artifact_name),
                );
            }
        }
        downloaded
    } else {
        progress(
            22,
            "Descompactando binario selecionado...",
            &format!("Extraindo {} ({})...", artifact_name, payload_label),
        );
        embedded_raw
    };

    if raw_exe.is_empty() {
        return Err(
            "Este instalador não tem um executável válido para instalar.".to_string(),
        );
    }

    let target_exe = install_dir.join("golive-lite.exe");
    progress(
        58,
        "Gravando Go Live - Lite...",
        &format!(
            "Copiando {} ({:.2} MB)...",
            target_exe.display(),
            raw_exe.len() as f64 / (1024.0 * 1024.0)
        ),
    );
    fs::write(&target_exe, &raw_exe).map_err(|e| {
        format!(
            "Falha ao gravar {} (feche o Go Live - Lite antes de atualizar/reinstalar): {}",
            target_exe.display(),
            e
        )
    })?;

    let unins_exe = install_dir.join("unins000.exe");
    progress(
        70,
        "Instalando desinstalador...",
        &format!("Gravando {}...", unins_exe.display()),
    );
    let slim_unins_bytes = if !PAYLOAD_UNINS_ZLIB.is_empty() {
        decompress_zlib_bytes(PAYLOAD_UNINS_ZLIB).unwrap_or_default()
    } else {
        Vec::new()
    };
    let used_slim_unins = if !slim_unins_bytes.is_empty() {
        fs::write(&unins_exe, &slim_unins_bytes).map_err(|e| {
            format!("Falha ao gravar desinstalador {}: {}", unins_exe.display(), e)
        })?;
        true
    } else {
        if let Ok(cur_exe) = std::env::current_exe() {
            let same_dest = fs::canonicalize(&cur_exe)
                .ok()
                .zip(fs::canonicalize(&unins_exe).ok())
                .is_some_and(|(a, b)| a == b);
            if !same_dest {
                fs::copy(&cur_exe, &unins_exe).map_err(|e| {
                    format!("Falha ao copiar desinstalador {}: {}", unins_exe.display(), e)
                })?;
            }
        }
        false
    };

    progress(
        82,
        "Aplicando modificações selecionadas...",
        "Sincronizando modificações opcionais...",
    );
    let mut mod_logs = Vec::new();
    write_modifications_state(install_dir, selected_mods, &mut |line| {
        mod_logs.push(line.to_string());
    })?;
    for l in mod_logs {
        progress(90, "Aplicando modificações selecionadas...", &l);
    }

    if selected_mods.is_empty() {
        progress(
            95,
            "Arquivos principais prontos.",
            "Modo Ultra-Leve ativo (0 modificações no disco e na memoria).",
        );
    }

    progress(
        100,
        "Arquivos e modificações aplicados.",
        "Etapa de arquivos concluida com sucesso.",
    );
    Ok((target_exe, raw_exe.len() as u64, used_slim_unins))
}

fn finalize_install_preferences(
    opts: &SetupOptions,
    exe_size_bytes: u64,
    used_slim_unins: bool,
) -> Result<PathBuf, String> {
    let target_exe = opts.install_dir.join("golive-lite.exe");
    let unins_exe = opts.install_dir.join("unins000.exe");
    // Remove qualquer pasta residual de logs em instalacoes antigas
    for old_logs in [
        opts.install_dir.join("logs"),
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(INTERNAL_DIR_NAME)
            .join("logs"),
    ] {
        if old_logs.is_dir() {
            let _ = fs::remove_dir_all(&old_logs);
        }
    }

    if !opts.no_registry {
        let cfg_path = config_file_path();
        if let Some(parent) = cfg_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let existing = fs::read_to_string(&cfg_path)
            .ok()
            .or_else(|| legacy_config_file_path().and_then(|p| fs::read_to_string(p).ok()))
            .unwrap_or_default();
        let merged = merge_config_settings(
            &existing,
            opts.start_with_windows,
            opts.minimize_to_tray_on_close,
            &opts.selected_modifications,
        );
        fs::write(&cfg_path, merged).map_err(|e| {
            format!("Falha ao atualizar configuracao {}: {}", cfg_path.display(), e)
        })?;

        reg_delete_value(RUN_SUBKEY, LEGACY_RUN_VALUE_NAME);
        if opts.start_with_windows {
            let cmd = format!("\"{}\" --hidden", target_exe.display());
            reg_set_string(RUN_SUBKEY, RUN_VALUE_NAME, &cmd)?;
        } else {
            reg_delete_value(RUN_SUBKEY, RUN_VALUE_NAME);
        }

        cleanup_legacy_shortcuts();
        if let Some(old_sm) = legacy_start_menu_shortcut_path() {
            let _ = fs::remove_file(old_sm);
        }
        let sm_lnk_opt = start_menu_shortcut_path();
        if let Some(ref sm_lnk) = sm_lnk_opt {
            create_shortcut(sm_lnk, &target_exe, &opts.install_dir, APP_DISPLAY_NAME)?;
        }

        if let Some(old_dt) = legacy_desktop_shortcut_path() {
            let _ = fs::remove_file(old_dt);
        }
        let dt_lnk_opt = desktop_shortcut_path();
        if opts.desktop_shortcut {
            if let Some(ref dt_lnk) = dt_lnk_opt {
                create_shortcut(dt_lnk, &target_exe, &opts.install_dir, APP_DISPLAY_NAME)?;
            }
        } else if let Some(ref dt_lnk) = dt_lnk_opt {
            let _ = fs::remove_file(dt_lnk);
        }

        let est_kb = ((exe_size_bytes + 1023) / 1024).min(u32::MAX as u64) as u32;
        let uninstall_cmd = format!("\"{}\" --uninstall", unins_exe.display());
        reg_delete_key(LEGACY_UNINSTALL_SUBKEY);
        reg_set_string(UNINSTALL_SUBKEY, "DisplayName", APP_DISPLAY_NAME)?;
        reg_set_string(UNINSTALL_SUBKEY, "DisplayVersion", APP_VERSION)?;
        reg_set_string(UNINSTALL_SUBKEY, "Publisher", "Go Live")?;
        reg_set_string(
            UNINSTALL_SUBKEY,
            "InstallLocation",
            &opts.install_dir.to_string_lossy(),
        )?;
        reg_set_string(
            UNINSTALL_SUBKEY,
            "DisplayIcon",
            &format!("{},0", target_exe.display()),
        )?;
        reg_set_string(UNINSTALL_SUBKEY, "UninstallString", &uninstall_cmd)?;
        if used_slim_unins {
            reg_set_dword(UNINSTALL_SUBKEY, "NoModify", 1)?;
            reg_delete_value(UNINSTALL_SUBKEY, "ModifyPath");
        } else {
            reg_set_dword(UNINSTALL_SUBKEY, "NoModify", 0)?;
            reg_set_string(
                UNINSTALL_SUBKEY,
                "ModifyPath",
                &format!("\"{}\" --install", unins_exe.display()),
            )?;
        }
        reg_set_dword(UNINSTALL_SUBKEY, "NoRepair", 1)?;
        reg_set_dword(UNINSTALL_SUBKEY, "EstimatedSize", est_kb)?;

        let mut notify_paths: Vec<&Path> = vec![target_exe.as_path(), unins_exe.as_path()];
        if let Some(ref sm) = sm_lnk_opt {
            notify_paths.push(sm.as_path());
        }
        if opts.desktop_shortcut {
            if let Some(ref dt) = dt_lnk_opt {
                notify_paths.push(dt.as_path());
            }
        }
        notify_shell_icon_update(&notify_paths);
    }

    if opts.launch_after {
        let _ = std::process::Command::new(&target_exe).spawn();
    }

    Ok(target_exe)
}

fn perform_install(opts: &SetupOptions) -> Result<PathBuf, String> {
    let mut noop = |_pct: u32, _st: &str, _log: &str| {};
    let existed = is_existing_install_in_dir(&opts.install_dir);
    let res = perform_core_files_install(&opts.install_dir, &opts.selected_modifications, &mut noop)
        .and_then(|(_exe, bytes_len, used_slim_unins)| {
            finalize_install_preferences(opts, bytes_len, used_slim_unins)
        });
    report_install_result(res.is_ok(), existed, &opts.selected_modifications, opts.no_registry);
    res
}

fn perform_uninstall_with_progress(
    opts: &SetupOptions,
    progress: &mut dyn FnMut(u32, &str, &str),
) -> Result<(), String> {
    // Metricas: avisa a API antes de apagar qualquer coisa (o id fica no config.json, que a
    // desinstalacao nao apaga; reinstalar reaproveita o mesmo id e desfaz o aviso).
    progress(8, "Finalizando...", "");
    send_uninstall_event();
    progress(
        15,
        "Removendo binario principal...",
        &format!("Pasta alvo: {}", opts.install_dir.display()),
    );
    let target_exe = opts.install_dir.join("golive-lite.exe");
    if target_exe.exists() {
        fs::remove_file(&target_exe).map_err(|e| {
            format!(
                "Nao foi possivel remover {} (feche o Go Live - Lite primeiro): {}",
                target_exe.display(),
                e
            )
        })?;
        progress(35, "Removendo modificações...", "Binario golive-lite.exe removido.");
    }

    let mods_manifest = opts.install_dir.join("modifications.json");
    if mods_manifest.exists() {
        let _ = fs::remove_file(&mods_manifest);
        progress(50, "Removendo modificações...", "Manifesto de modificações removido.");
    }
    let mods_dir = opts.install_dir.join("mods");
    if mods_dir.is_dir() {
        let _ = fs::remove_dir_all(&mods_dir);
        progress(65, "Removendo modificações...", "Pasta mods/ removida.");
    }
    let logs_dir = opts.install_dir.join("logs");
    if logs_dir.is_dir() {
        let _ = fs::remove_dir_all(&logs_dir);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let appdata_logs = PathBuf::from(appdata).join(INTERNAL_DIR_NAME).join("logs");
        if appdata_logs.is_dir() {
            let _ = fs::remove_dir_all(&appdata_logs);
        }
    }

    if !opts.no_registry {
        progress(80, "Limpando atalhos e registro...", "Removendo autostart e entrada do Windows...");
        reg_delete_value(RUN_SUBKEY, RUN_VALUE_NAME);
        reg_delete_value(RUN_SUBKEY, LEGACY_RUN_VALUE_NAME);
        reg_delete_key(UNINSTALL_SUBKEY);
        reg_delete_key(LEGACY_UNINSTALL_SUBKEY);
        cleanup_legacy_shortcuts();
        if let Some(sm_lnk) = start_menu_shortcut_path() {
            let _ = fs::remove_file(sm_lnk);
        }
        if let Some(old_sm) = legacy_start_menu_shortcut_path() {
            let _ = fs::remove_file(old_sm);
        }
        if let Some(dt_lnk) = desktop_shortcut_path() {
            let _ = fs::remove_file(dt_lnk);
        }
        if let Some(old_dt) = legacy_desktop_shortcut_path() {
            let _ = fs::remove_file(old_dt);
        }
        notify_shell_icon_update(&[]);
    }

    let unins_exe = opts.install_dir.join("unins000.exe");
    // Remove qualquer pasta residual de logs em instalacoes antigas
    for old_logs in [
        opts.install_dir.join("logs"),
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(INTERNAL_DIR_NAME)
            .join("logs"),
    ] {
        if old_logs.is_dir() {
            let _ = fs::remove_dir_all(&old_logs);
        }
    }
    let running_self_unins = std::env::current_exe()
        .ok()
        .zip(fs::canonicalize(&unins_exe).ok())
        .zip(
            std::env::current_exe()
                .ok()
                .and_then(|p| fs::canonicalize(p).ok()),
        )
        .is_some_and(|((_, unins_canon), cur_canon)| unins_canon == cur_canon);

    if running_self_unins && !opts.no_registry {
        let cmdline = format!(
            "ping 127.0.0.1 -n 2 > nul & del /f /q \"{}\" & rmdir \"{}\"",
            unins_exe.display(),
            opts.install_dir.display()
        );
        let _ = std::process::Command::new("cmd.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["/C", &cmdline])
            .spawn();
    } else if unins_exe.exists() {
        let _ = fs::remove_file(&unins_exe);
        let _ = fs::remove_dir(&opts.install_dir);
    } else {
        let _ = fs::remove_dir(&opts.install_dir);
    }

    progress(100, "Desinstalacao concluida!", "Go Live - Lite foi removido com sucesso.");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardPage {
    Folder,
    Modifications,
    Progress,
    DoneConfig,
    UninstallConfirm,
}

struct WizardState {
    opts: SetupOptions,
    page: WizardPage,
    existing_in_dir: bool,
    existing_mods: Vec<String>,
    worker_busy: bool,
    worker_finished: bool,
    worker_was_uninstall: bool,
    installed_exe_bytes: u64,
    used_slim_unins: bool,
    details_expanded: bool,
    details_log: String,
    /// Checkboxes da etapa 2 ja criados (dependem do catalogo da API).
    page2_built: bool,
    font_regular: isize,
    font_bold: isize,
    font_title: isize,
    /// Rotulos pequenos (etapa, barra lateral).
    font_small: isize,
    header_icon: isize,
    wnd_icon_big: isize,
    wnd_icon_sm: isize,
}

fn ctrl_text_path(hwnd: isize, id: usize) -> PathBuf {
    let mut buf = vec![0u16; 1024];
    unsafe {
        let item = dlg_item(hwnd, id);
        let n = GetWindowTextW(item, buf.as_mut_ptr(), buf.len() as i32);
        let s = if n > 0 {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        };
        PathBuf::from(s.trim())
    }
}

fn set_ctrl_text(hwnd: isize, id: usize, txt: &str) {
    let w = to_wide_null(txt);
    unsafe {
        let item = dlg_item(hwnd, id);
        if item != 0 {
            let _ = SetWindowTextW(item, w.as_ptr());
        }
    }
}

/// Cria (uma vez) os checkboxes da etapa 2 a partir do catalogo da API, que carregou em
/// segundo plano enquanto a pessoa estava na etapa 1.
fn ensure_page2_controls(hwnd: isize, st: &mut WizardState) {
    if st.page2_built {
        return;
    }
    st.page2_built = true;
    unsafe {
        let h_inst = GetModuleHandleW(null());
        // Area com rolagem entre a dica (y 106) e o resumo (y 338): todas as modificacoes do
        // catalogo cabem, quantas forem. Largura 486 = conteudo + a barra de rolagem.
        const PANEL_X: i32 = 24;
        const PANEL_Y: i32 = 128;
        const PANEL_W: i32 = 486;
        const PANEL_H: i32 = 206;
        let panel = mods_panel::create(
            hwnd,
            h_inst,
            look::s(look::SIDEBAR_W + PANEL_X),
            look::s(PANEL_Y),
            look::s(PANEL_W),
            look::s(PANEL_H),
            IDC_P2_PANEL,
            look::content_brush(),
        );
        // Conteudo sem a largura da barra (~18 px).
        let inner_w = PANEL_W - 22;
        let mut y = 2;
        for (idx, m) in installable_mods().iter().take(MAX_PAGE2_MODS).enumerate() {
            spawn_in_panel(
                panel, h_inst, "BUTTON", &m.name,
                BS_AUTOCHECKBOX | WS_TABSTOP, 0, y, inner_w, 20, IDC_P2_MOD_BASE + idx, st.font_bold,
            );
            // Uma linha, "..." no fim quando nao cabe (nunca cortada no meio da palavra).
            spawn_in_panel(
                panel, h_inst, "STATIC", &m.description,
                SS_LEFT | SS_ENDELLIPSIS, 20, y + 21, inner_w - 20, 18, IDC_P2_MOD_DESC_BASE + idx, st.font_regular,
            );
            y += 44;
        }
        // "Em breve": os nomes numa lista corrida (quebra de linha automatica), dentro da rolagem.
        let soon = coming_soon_mods();
        if !soon.is_empty() {
            spawn_in_panel(
                panel, h_inst, "STATIC", "Em breve (ainda não disponíveis)", SS_LEFT, 0, y + 6, inner_w, 18, IDC_P2_SOON_LBL, st.font_bold,
            );
            let names: Vec<String> = soon.iter().take(MAX_PAGE2_SOON).map(|m| m.name.clone()).collect();
            // Altura pela quantidade de texto (~60 caracteres por linha de 18 px).
            let text = names.join("  ·  ");
            let lines = (text.chars().count() as i32 / 60 + 1).max(1);
            spawn_in_panel(
                panel, h_inst, "STATIC", &text,
                SS_LEFT, 0, y + 28, inner_w, lines * 18 + 4, IDC_P2_SOON_BASE, st.font_regular,
            );
            y += 28 + lines * 18 + 8;
        }
        mods_panel::set_content_height(look::s(y), look::s(PANEL_H));
    }
}

fn page2_mod_count() -> usize {
    installable_mods().len().min(MAX_PAGE2_MODS)
}

fn show_ctrl(hwnd: isize, id: usize, visible: bool) {
    unsafe {
        let item = dlg_item(hwnd, id);
        if item != 0 {
            let _ = ShowWindow(item, if visible { SW_SHOW } else { SW_HIDE });
        }
    }
}

fn enable_ctrl(hwnd: isize, id: usize, enabled: bool) {
    unsafe {
        let item = dlg_item(hwnd, id);
        if item != 0 {
            let _ = EnableWindow(item, if enabled { 1 } else { 0 });
        }
    }
}

fn is_checked(hwnd: isize, id: usize) -> bool {
    unsafe {
        let item = dlg_item(hwnd, id);
        SendMessageW(item, BM_GETCHECK, 0, 0) as usize == BST_CHECKED
    }
}

fn set_checked(hwnd: isize, id: usize, checked: bool) {
    unsafe {
        let item = dlg_item(hwnd, id);
        if item != 0 {
            SendMessageW(
                item,
                BM_SETCHECK,
                if checked { BST_CHECKED } else { BST_UNCHECKED },
                0,
            );
        }
    }
}

fn append_details_line(hwnd: isize, st: &mut WizardState, line: &str) {
    if !st.details_log.is_empty() {
        st.details_log.push_str("\r\n");
    }
    st.details_log.push_str(line);
    set_ctrl_text(hwnd, IDC_P3_DETAILS_EDIT, &st.details_log);
    unsafe {
        let edit = GetDlgItem(hwnd, IDC_P3_DETAILS_EDIT as i32);
        if edit != 0 {
            let len = st.details_log.encode_utf16().count();
            SendMessageW(edit, EM_SETSEL, len, len as isize);
            SendMessageW(edit, EM_SCROLLCARET, 0, 0);
        }
    }
}

fn browse_folder_dialog(owner: isize) -> Option<PathBuf> {
    let mut display_buf = [0u16; 260];
    let title_w = to_wide_null("Escolha a pasta de instalação do Go Live - Lite:");
    let mut bi = BrowseInfoW {
        hwnd_owner: owner,
        pidl_root: null(),
        psz_display_name: display_buf.as_mut_ptr(),
        lpsz_title: title_w.as_ptr(),
        ul_flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: null(),
        l_param: 0,
        i_image: 0,
    };
    unsafe {
        let _ = CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED);
        let pidl = SHBrowseForFolderW(&mut bi);
        if pidl.is_null() {
            CoUninitialize();
            return None;
        }
        let mut path_buf = [0u16; 1024];
        let ok = SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr()) != 0;
        CoTaskMemFree(pidl);
        CoUninitialize();
        if ok {
            let s = wide_slice_to_string(&path_buf);
            if !s.trim().is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    None
}

unsafe fn spawn_ctrl(
    parent: isize,
    h_inst: isize,
    class_name: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    font: isize,
) -> isize {
    let cls_w = to_wide_null(class_name);
    let txt_w = to_wide_null(text);
    let hwnd = CreateWindowExW(
        ex_style,
        cls_w.as_ptr(),
        txt_w.as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        look::s(look::SIDEBAR_W + x),
        look::s(y),
        look::s(w),
        look::s(h),
        parent,
        id as isize,
        h_inst,
        null(),
    );
    if hwnd != 0 && font != 0 {
        let _ = SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
    }
    hwnd
}

fn refresh_page1_detection(hwnd: isize, st: &mut WizardState) {
    let dir = ctrl_text_path(hwnd, IDC_P1_DIR_EDIT);
    if !dir.as_os_str().is_empty() {
        st.opts.install_dir = dir.clone();
    }
    st.existing_in_dir = is_existing_install_in_dir(&st.opts.install_dir);
    let any_existing = st.existing_in_dir || detect_existing_install_dir().is_some();

    if st.existing_in_dir {
        st.existing_mods = read_installed_modifications(&st.opts.install_dir);
        let mods_txt = if st.existing_mods.is_empty() {
            "modo ultraleve, sem modificações".to_string()
        } else {
            st.existing_mods.join(", ")
        };
        set_ctrl_text(
            hwnd,
            IDC_P1_DETECT_LBL,
            &format!(
                "Instalação existente detectada. Você pode atualizar ou adicionar/remover modificações ({}).",
                mods_txt
            ),
        );
    } else {
        st.existing_mods.clear();
        set_ctrl_text(
            hwnd,
            IDC_P1_DETECT_LBL,
            "Nova instalação. Na próxima etapa você poderá escolher modificações opcionais.",
        );
    }

    if st.page == WizardPage::Folder {
        show_ctrl(hwnd, IDC_P1_UNINSTALL_BTN, any_existing);
    }
}

fn refresh_page2_summary(hwnd: isize, st: &mut WizardState) {
    let mut selected = Vec::new();
    for (idx, spec) in installable_mods().into_iter().take(MAX_PAGE2_MODS).enumerate() {
        let chk = is_checked(hwnd, IDC_P2_MOD_BASE + idx);
        if chk {
            selected.push(spec.id.to_string());
        }
        let status_tag = if st.existing_in_dir {
            let had = st.existing_mods.iter().any(|m| m == &spec.id);
            match (had, chk) {
                (true, true) => "  · já instalada",
                (false, true) => "  · será instalada",
                (true, false) => "  · será removida",
                (false, false) => "",
            }
        } else {
            ""
        };
        set_ctrl_text(
            hwnd,
            IDC_P2_MOD_BASE + idx,
            &format!("{}{}", spec.name, status_tag),
        );
    }
    st.opts.selected_modifications = selected;

    let txt = if st.opts.selected_modifications.is_empty() {
        "Modo ultraleve: nenhuma modificação, o máximo de desempenho.".to_string()
    } else {
        format!(
            "{} modificação(ões) selecionada(s).",
            st.opts.selected_modifications.len()
        )
    };
    set_ctrl_text(hwnd, IDC_P2_SUMMARY, &txt);
}

/// Etapas da barra lateral e qual e a atual (`len` = tudo concluido).
fn sidebar_steps(st: &WizardState) -> (Vec<&'static str>, usize) {
    let uninstall_flow = matches!(st.page, WizardPage::UninstallConfirm)
        || (matches!(st.page, WizardPage::Progress) && st.worker_was_uninstall);
    if uninstall_flow {
        let current = match st.page {
            WizardPage::UninstallConfirm => 0,
            _ if st.worker_finished => 3,
            _ => 1,
        };
        return (vec!["Confirmar", "Remoção", "Concluído"], current);
    }
    let current = match st.page {
        WizardPage::Folder => 0,
        WizardPage::Modifications => 1,
        WizardPage::Progress => 2,
        WizardPage::DoneConfig => 3,
        WizardPage::UninstallConfirm => 0,
    };
    (vec!["Pasta", "Modificações", "Instalação", "Concluído"], current)
}

fn show_page(hwnd: isize, st: &mut WizardState, page: WizardPage) {
    st.page = page;
    look::invalidate_sidebar(hwnd);

    let p1 = [
        IDC_P1_LBL,
        IDC_P1_DIR_EDIT,
        IDC_P1_BROWSE_BTN,
        IDC_P1_DETECT_LBL,
        IDC_P1_UNINSTALL_BTN,
    ];
    for id in p1 {
        show_ctrl(hwnd, id, false);
    }
    show_ctrl(hwnd, IDC_P2_HINT, false);
    show_ctrl(hwnd, IDC_P2_PANEL, false);
    show_ctrl(hwnd, IDC_P2_SUMMARY, false);
    for idx in 0..MAX_PAGE2_MODS {
        show_ctrl(hwnd, IDC_P2_MOD_BASE + idx, false);
        show_ctrl(hwnd, IDC_P2_MOD_DESC_BASE + idx, false);
    }
    show_ctrl(hwnd, IDC_P2_SOON_LBL, false);
    // A lista "Em breve" e um controle so (os ids seguintes sao de outras etapas).
    show_ctrl(hwnd, IDC_P2_SOON_BASE, false);
    for id in [
        IDC_P3_STATUS_LBL,
        IDC_P3_PROGRESS,
        IDC_P3_DETAILS_BTN,
        IDC_P3_DETAILS_EDIT,
    ] {
        show_ctrl(hwnd, id, false);
    }
    for id in [
        IDC_P4_SUMMARY,
        IDC_P4_CHK_DESKTOP,
        IDC_P4_CHK_AUTOSTART,
        IDC_P4_CHK_CLOSE_TRAY,
        IDC_P4_CHK_LAUNCH,
    ] {
        show_ctrl(hwnd, id, false);
    }
    show_ctrl(hwnd, IDC_P5_DESC, false);

    match page {
        WizardPage::Folder => {
            set_ctrl_text(hwnd, IDC_STEP_BADGE, "ETAPA 1 DE 4");
            set_ctrl_text(hwnd, IDC_TITLE, "Instalar Go Live - Lite");
            set_ctrl_text(
                hwnd,
                IDC_SUBTITLE,
                "O jeito mais leve de usar o Go Live no Windows.",
            );
            for id in [IDC_P1_LBL, IDC_P1_DIR_EDIT, IDC_P1_BROWSE_BTN, IDC_P1_DETECT_LBL] {
                show_ctrl(hwnd, id, true);
            }
            refresh_page1_detection(hwnd, st);

            show_ctrl(hwnd, IDC_BACK_BTN, false);
            show_ctrl(hwnd, IDC_NEXT_BTN, true);
            enable_ctrl(hwnd, IDC_NEXT_BTN, true);
            set_ctrl_text(hwnd, IDC_NEXT_BTN, "Avançar");
            show_ctrl(hwnd, IDC_CANCEL_BTN, true);
            enable_ctrl(hwnd, IDC_CANCEL_BTN, true);
        }
        WizardPage::Modifications => {
            ensure_page2_controls(hwnd, st);
            set_ctrl_text(hwnd, IDC_STEP_BADGE, "ETAPA 2 DE 4");
            set_ctrl_text(hwnd, IDC_TITLE, "Escolha as modificações");
            set_ctrl_text(
                hwnd,
                IDC_SUBTITLE,
                "Recursos opcionais. Dá para mudar depois rodando o instalador de novo.",
            );
            show_ctrl(hwnd, IDC_P2_HINT, true);
            show_ctrl(hwnd, IDC_P2_PANEL, true);
            show_ctrl(hwnd, IDC_P2_SUMMARY, true);

            if st.existing_in_dir && !st.opts.mods_specified {
                st.opts.selected_modifications = st.existing_mods.clone();
            }
            for (idx, spec) in installable_mods().into_iter().take(MAX_PAGE2_MODS).enumerate() {
                show_ctrl(hwnd, IDC_P2_MOD_BASE + idx, true);
                show_ctrl(hwnd, IDC_P2_MOD_DESC_BASE + idx, true);
                let checked = st.opts.selected_modifications.iter().any(|id| id == &spec.id);
                set_checked(hwnd, IDC_P2_MOD_BASE + idx, checked);
            }
            show_ctrl(hwnd, IDC_P2_SOON_LBL, true);
            show_ctrl(hwnd, IDC_P2_SOON_BASE, true);
            refresh_page2_summary(hwnd, st);
            mods_panel::redraw();

            show_ctrl(hwnd, IDC_BACK_BTN, true);
            enable_ctrl(hwnd, IDC_BACK_BTN, true);
            set_ctrl_text(hwnd, IDC_BACK_BTN, "Voltar");
            show_ctrl(hwnd, IDC_NEXT_BTN, true);
            enable_ctrl(hwnd, IDC_NEXT_BTN, true);
            set_ctrl_text(
                hwnd,
                IDC_NEXT_BTN,
                if st.existing_in_dir {
                    "Atualizar"
                } else {
                    "Instalar"
                },
            );
            show_ctrl(hwnd, IDC_CANCEL_BTN, true);
            enable_ctrl(hwnd, IDC_CANCEL_BTN, true);
        }
        WizardPage::Progress => {
            set_ctrl_text(
                hwnd,
                IDC_STEP_BADGE,
                if st.worker_was_uninstall {
                    "DESINSTALAÇÃO"
                } else {
                    "ETAPA 3 DE 4"
                },
            );
            set_ctrl_text(
                hwnd,
                IDC_TITLE,
                if st.worker_was_uninstall {
                    "Desinstalando Go Live - Lite"
                } else if st.existing_in_dir {
                    "Atualizando Go Live - Lite"
                } else {
                    "Instalando Go Live - Lite"
                },
            );
            set_ctrl_text(
                hwnd,
                IDC_SUBTITLE,
                "Aguarde enquanto os arquivos e as modificações são preparados.",
            );
            show_ctrl(hwnd, IDC_P3_STATUS_LBL, true);
            show_ctrl(hwnd, IDC_P3_PROGRESS, true);
            show_ctrl(hwnd, IDC_P3_DETAILS_BTN, true);
            show_ctrl(hwnd, IDC_P3_DETAILS_EDIT, st.details_expanded);
            set_ctrl_text(
                hwnd,
                IDC_P3_DETAILS_BTN,
                if st.details_expanded {
                    "Ocultar detalhes"
                } else {
                    "Ver detalhes"
                },
            );

            show_ctrl(hwnd, IDC_BACK_BTN, false);
            show_ctrl(hwnd, IDC_NEXT_BTN, true);
            enable_ctrl(hwnd, IDC_NEXT_BTN, st.worker_finished);
            set_ctrl_text(
                hwnd,
                IDC_NEXT_BTN,
                if st.worker_was_uninstall {
                    "Concluir"
                } else {
                    "Avançar"
                },
            );
            show_ctrl(hwnd, IDC_CANCEL_BTN, !st.worker_finished);
            enable_ctrl(hwnd, IDC_CANCEL_BTN, !st.worker_finished);
        }
        WizardPage::DoneConfig => {
            set_ctrl_text(hwnd, IDC_STEP_BADGE, "ETAPA 4 DE 4");
            set_ctrl_text(hwnd, IDC_TITLE, "Tudo pronto!");
            set_ctrl_text(
                hwnd,
                IDC_SUBTITLE,
                "Ajuste as preferências abaixo e clique em Concluir.",
            );

            if let Some((auto, tray)) = read_existing_config_flags() {
                st.opts.start_with_windows = auto;
                st.opts.minimize_to_tray_on_close = tray;
            }

            let mods_txt = if st.opts.selected_modifications.is_empty() {
                "Nenhuma (modo ultraleve)".to_string()
            } else {
                st.opts.selected_modifications.join(", ")
            };
            set_ctrl_text(
                hwnd,
                IDC_P4_SUMMARY,
                &format!(
                    "Pasta: {}\r\nModificacoes ativas: {}",
                    st.opts.install_dir.display(),
                    mods_txt
                ),
            );
            set_checked(hwnd, IDC_P4_CHK_DESKTOP, st.opts.desktop_shortcut);
            set_checked(hwnd, IDC_P4_CHK_AUTOSTART, st.opts.start_with_windows);
            set_checked(hwnd, IDC_P4_CHK_CLOSE_TRAY, st.opts.minimize_to_tray_on_close);
            set_checked(hwnd, IDC_P4_CHK_LAUNCH, st.opts.launch_after);

            for id in [
                IDC_P4_SUMMARY,
                IDC_P4_CHK_DESKTOP,
                IDC_P4_CHK_AUTOSTART,
                IDC_P4_CHK_CLOSE_TRAY,
                IDC_P4_CHK_LAUNCH,
            ] {
                show_ctrl(hwnd, id, true);
            }

            show_ctrl(hwnd, IDC_BACK_BTN, false);
            show_ctrl(hwnd, IDC_NEXT_BTN, true);
            enable_ctrl(hwnd, IDC_NEXT_BTN, true);
            set_ctrl_text(hwnd, IDC_NEXT_BTN, "Concluir");
            show_ctrl(hwnd, IDC_CANCEL_BTN, false);
        }
        WizardPage::UninstallConfirm => {
            set_ctrl_text(hwnd, IDC_STEP_BADGE, "DESINSTALAÇÃO");
            set_ctrl_text(hwnd, IDC_TITLE, "Desinstalar Go Live - Lite");
            set_ctrl_text(
                hwnd,
                IDC_SUBTITLE,
                "Remove o aplicativo, as modificações e os atalhos.",
            );
            set_ctrl_text(
                hwnd,
                IDC_P5_DESC,
                &format!(
                    "Isso removerá o Go Live - Lite e todas as modificações instaladas da pasta:\r\n\r\n{}\r\n\r\nTambém serão removidos os atalhos e a inicialização automática.",
                    st.opts.install_dir.display()
                ),
            );
            show_ctrl(hwnd, IDC_P5_DESC, true);

            show_ctrl(hwnd, IDC_BACK_BTN, true);
            enable_ctrl(hwnd, IDC_BACK_BTN, true);
            set_ctrl_text(hwnd, IDC_BACK_BTN, "Voltar");
            show_ctrl(hwnd, IDC_NEXT_BTN, true);
            enable_ctrl(hwnd, IDC_NEXT_BTN, true);
            set_ctrl_text(hwnd, IDC_NEXT_BTN, "Desinstalar");
            show_ctrl(hwnd, IDC_CANCEL_BTN, true);
            enable_ctrl(hwnd, IDC_CANCEL_BTN, true);
        }
    }
}

fn start_background_worker(hwnd: isize, st: &mut WizardState, uninstall: bool) {
    st.worker_busy = true;
    st.worker_finished = false;
    st.worker_was_uninstall = uninstall;
    st.details_log.clear();
    set_ctrl_text(hwnd, IDC_P3_DETAILS_EDIT, "");
    unsafe {
        let bar = GetDlgItem(hwnd, IDC_P3_PROGRESS as i32);
        if bar != 0 {
            SendMessageW(bar, PBM_SETRANGE32, 0, 100);
            SendMessageW(bar, PBM_SETPOS, 5, 0);
        }
    }
    show_page(hwnd, st, WizardPage::Progress);

    let install_dir = st.opts.install_dir.clone();
    let mods = st.opts.selected_modifications.clone();
    let no_reg = st.opts.no_registry;

    std::thread::spawn(move || {
        let mut emit = |pct: u32, status: &str, log_line: &str| unsafe {
            let _ = PostMessageW(hwnd, WM_APP_PROGRESS, pct.min(100) as usize, 0);
            let st_ptr = Box::into_raw(Box::new(status.to_string()));
            let _ = PostMessageW(hwnd, WM_APP_STATUS, 0, st_ptr as isize);
            if !log_line.trim().is_empty() {
                let lg_ptr = Box::into_raw(Box::new(log_line.to_string()));
                let _ = PostMessageW(hwnd, WM_APP_LOG, 0, lg_ptr as isize);
            }
        };

        if uninstall {
            let mut u_opts = SetupOptions::default();
            u_opts.uninstall = true;
            u_opts.install_dir = install_dir;
            u_opts.no_registry = no_reg;
            match perform_uninstall_with_progress(&u_opts, &mut emit) {
                Ok(()) => unsafe {
                    let _ = PostMessageW(hwnd, WM_APP_DONE, 0, 0);
                },
                Err(err) => unsafe {
                    let ptr = Box::into_raw(Box::new(err));
                    let _ = PostMessageW(hwnd, WM_APP_ERROR, 0, ptr as isize);
                },
            }
        } else {
            let existed = is_existing_install_in_dir(&install_dir);
            let ok = match perform_core_files_install(&install_dir, &mods, &mut emit) {
                Ok((_exe, bytes_len, slim_unins)) => unsafe {
                    let flag: usize = if slim_unins { 3 } else { 1 };
                    let _ = PostMessageW(hwnd, WM_APP_DONE, flag, bytes_len as isize);
                    true
                },
                Err(err) => unsafe {
                    let ptr = Box::into_raw(Box::new(err));
                    let _ = PostMessageW(hwnd, WM_APP_ERROR, 0, ptr as isize);
                    false
                },
            };
            // Depois de avisar a janela: a metrica nunca atrasa o "Pronto".
            report_install_result(ok, existed, &mods, no_reg);
        }
    });
}

unsafe extern "system" fn wizard_wnd_proc(
    hwnd: isize,
    msg: u32,
    w_param: usize,
    l_param: isize,
) -> isize {
    match msg {
        WM_CREATE => {
            let st_ptr = *(l_param as *const isize);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, st_ptr);
            let st = &mut *(st_ptr as *mut WizardState);
            let h_inst = GetModuleHandleW(null());

            st.header_icon = LoadImageW(h_inst, 1 as *const u16, IMAGE_ICON, look::s(40), look::s(40), 0);
            st.wnd_icon_big = LoadImageW(h_inst, 1 as *const u16, IMAGE_ICON, 32, 32, 0);
            st.wnd_icon_sm = LoadImageW(h_inst, 1 as *const u16, IMAGE_ICON, 16, 16, 0);
            if st.wnd_icon_big != 0 {
                let _ = SendMessageW(hwnd, WM_SETICON, ICON_BIG, st.wnd_icon_big);
            }
            if st.wnd_icon_sm != 0 {
                let _ = SendMessageW(hwnd, WM_SETICON, ICON_SMALL, st.wnd_icon_sm);
            }

            // Segoe UI (a fonte do Windows), semibold nos titulos, tudo escalado pelo DPI.
            let segoe_w = to_wide_null("Segoe UI");
            let font = |px: i32, weight: i32| {
                CreateFontW(
                    -look::s(px), 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, CLEARTYPE_QUALITY, 0, segoe_w.as_ptr(),
                )
            };
            st.font_title = font(24, 600);
            st.font_bold = font(14, 600);
            st.font_regular = font(14, FW_NORMAL);
            st.font_small = font(12, 600);

            // Cabecalho do conteudo (o logo fica na barra lateral, pintada em look::paint).
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 22, 486, 16, IDC_STEP_BADGE, st.font_small,
            );
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 40, 486, 34, IDC_TITLE, st.font_title,
            );
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 76, 486, 20, IDC_SUBTITLE, st.font_regular,
            );

            // Pagina 1: Pasta de Instalacao
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "Pasta de instalação",
                SS_LEFT, 0, 24, 108, 460, 20, IDC_P1_LBL, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "EDIT", &st.opts.install_dir.to_string_lossy(),
                ES_AUTOHSCROLL | WS_TABSTOP, WS_EX_CLIENTEDGE, 24, 134, 382, 28, IDC_P1_DIR_EDIT, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Procurar…",
                BS_PUSHBUTTON | WS_TABSTOP, 0, 414, 134, 96, 28, IDC_P1_BROWSE_BTN, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 174, 486, 44, IDC_P1_DETECT_LBL, st.font_regular,
            );
            // Acao secundaria: no rodape, a esquerda (os botoes de navegacao ficam a direita).
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Desinstalar",
                BS_PUSHBUTTON | WS_TABSTOP, 0, 24, 382, 110, 32, IDC_P1_UNINSTALL_BTN, st.font_regular,
            );

            // Pagina 2: Modificacoes
            spawn_ctrl(
                hwnd, h_inst, "STATIC",
                "Marque só o que você vai usar. Quanto menos, mais leve.",
                SS_LEFT, 0, 24, 106, 486, 18, IDC_P2_HINT, st.font_regular,
            );
            // Os checkboxes das modificacoes sao criados por ensure_page2_controls (catalogo da API).
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 338, 486, 18, IDC_P2_SUMMARY, st.font_bold,
            );

            // Pagina 3: Progresso + Ver detalhes
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "Preparando...", SS_LEFT, 0, 24, 106, 486, 22, IDC_P3_STATUS_LBL, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "msctls_progress32", "", 0, 0, 24, 136, 486, 10, IDC_P3_PROGRESS, 0,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Ver detalhes",
                BS_PUSHBUTTON | WS_TABSTOP, 0, 24, 160, 126, 28, IDC_P3_DETAILS_BTN, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "EDIT", "",
                ES_MULTILINE | ES_READONLY | WS_VSCROLL | ES_AUTOVSCROLL | WS_TABSTOP,
                WS_EX_CLIENTEDGE, 24, 198, 486, 150, IDC_P3_DETAILS_EDIT, st.font_regular,
            );

            // Pagina 4: Pronto / Configuracoes finais
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 106, 486, 44, IDC_P4_SUMMARY, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Criar atalho na Área de Trabalho",
                BS_AUTOCHECKBOX | WS_TABSTOP, 0, 24, 164, 460, 24, IDC_P4_CHK_DESKTOP, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Iniciar com o Windows (escondido na bandeja)",
                BS_AUTOCHECKBOX | WS_TABSTOP, 0, 24, 194, 460, 24, IDC_P4_CHK_AUTOSTART, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Fechar (X) minimiza para a bandeja",
                BS_AUTOCHECKBOX | WS_TABSTOP, 0, 24, 224, 460, 24, IDC_P4_CHK_CLOSE_TRAY, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Abrir Go Live - Lite ao concluir",
                BS_AUTOCHECKBOX | WS_TABSTOP, 0, 24, 254, 460, 24, IDC_P4_CHK_LAUNCH, st.font_regular,
            );

            // Pagina 5: Confirmar desinstalacao
            spawn_ctrl(
                hwnd, h_inst, "STATIC", "", SS_LEFT, 0, 24, 112, 486, 130, IDC_P5_DESC, st.font_regular,
            );

            // Barra de navegacao inferior
            // Barra de navegacao no rodape, alinhada a direita: Voltar, Avancar (padrao), Cancelar.
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Voltar",
                BS_PUSHBUTTON | WS_TABSTOP, 0, 196, 382, 96, 32, IDC_BACK_BTN, st.font_regular,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Avançar",
                BS_DEFPUSHBUTTON | WS_TABSTOP, 0, 300, 382, 112, 32, IDC_NEXT_BTN, st.font_bold,
            );
            spawn_ctrl(
                hwnd, h_inst, "BUTTON", "Cancelar",
                BS_PUSHBUTTON | WS_TABSTOP, 0, 420, 382, 90, 32, IDC_CANCEL_BTN, st.font_regular,
            );

            let start_page = st.page;
            show_page(hwnd, st, start_page);
            0
        }
        // Roda do mouse na etapa de modificacoes: rola a lista (mesmo com o foco em um botao).
        mods_panel::WM_MOUSEWHEEL => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            if !st_ptr.is_null() && matches!((*st_ptr).page, WizardPage::Modifications) {
                mods_panel::wheel(w_param);
                return 0;
            }
            DefWindowProcW(hwnd, msg, w_param, l_param)
        }
        WM_PAINT => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            if st_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, w_param, l_param);
            }
            let st = &*st_ptr;
            let (steps, current) = sidebar_steps(st);
            let fonts = look::Fonts {
                semibold: st.font_bold,
                regular: st.font_regular,
                small: st.font_small,
            };
            look::paint(hwnd, st.header_icon, &fonts, &look::StepsView { steps: &steps, current });
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            // Fundo de cada controle = o que esta atras dele (conteudo branco ou rodape cinza),
            // e a cor do texto por papel: titulo forte, etapa em verde, o resto em cinza.
            let hdc = w_param as isize;
            let id = look::GetDlgCtrlID(l_param) as usize;
            let in_footer = matches!(id, IDC_BACK_BTN | IDC_NEXT_BTN | IDC_CANCEL_BTN | IDC_P1_UNINSTALL_BTN);
            let (bg, brush) = if in_footer {
                (look::FOOTER_BG, look::footer_brush())
            } else {
                (look::CONTENT_BG, look::content_brush())
            };
            let fg = match id {
                IDC_STEP_BADGE => look::TEXT_ACCENT,
                IDC_SUBTITLE | IDC_P1_DETECT_LBL | IDC_P2_HINT | IDC_P3_STATUS_LBL | IDC_P2_SOON_BASE => look::TEXT_MUTED,
                _ if (IDC_P2_MOD_DESC_BASE..IDC_P2_MOD_DESC_BASE + MAX_PAGE2_MODS).contains(&id) => look::TEXT_MUTED,
                _ => look::TEXT_STRONG,
            };
            look::SetTextColor(hdc, fg);
            look::SetBkColor(hdc, bg);
            brush
        }
        WM_APP_PROGRESS => {
            let bar = GetDlgItem(hwnd, IDC_P3_PROGRESS as i32);
            if bar != 0 {
                SendMessageW(bar, PBM_SETPOS, w_param, 0);
            }
            0
        }
        WM_APP_STATUS => {
            if l_param != 0 {
                let txt = Box::from_raw(l_param as *mut String);
                set_ctrl_text(hwnd, IDC_P3_STATUS_LBL, &txt);
            }
            0
        }
        WM_APP_LOG => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            if l_param != 0 {
                let line = Box::from_raw(l_param as *mut String);
                if !st_ptr.is_null() {
                    append_details_line(hwnd, &mut *st_ptr, &line);
                }
            }
            0
        }
        WM_APP_DONE => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            if !st_ptr.is_null() {
                let st = &mut *st_ptr;
                st.worker_busy = false;
                st.worker_finished = true;
                let was_install = (w_param & 1) != 0;
                if was_install {
                    st.installed_exe_bytes = l_param as u64;
                    st.used_slim_unins = (w_param & 2) != 0;
                    if !st.details_expanded {
                        show_page(hwnd, st, WizardPage::DoneConfig);
                        return 0;
                    }
                }
                show_page(hwnd, st, WizardPage::Progress);
            }
            0
        }
        WM_APP_ERROR => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            if l_param != 0 {
                let err = Box::from_raw(l_param as *mut String);
                set_ctrl_text(hwnd, IDC_P3_STATUS_LBL, &err);
                if !st_ptr.is_null() {
                    let st = &mut *st_ptr;
                    st.worker_busy = false;
                    append_details_line(hwnd, st, &format!("ERRO: {}", err));
                    enable_ctrl(hwnd, IDC_CANCEL_BTN, true);
                    show_ctrl(hwnd, IDC_CANCEL_BTN, true);
                }
                let msg_w = to_wide_null(&err);
                let cap_w = to_wide_null("Erro no instalador");
                let _ = MessageBoxW(hwnd, msg_w.as_ptr(), cap_w.as_ptr(), MB_OK | MB_ICONERROR);
            }
            0
        }
        WM_COMMAND => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            let ctl_id = w_param & 0xFFFF;
            let notify_code = (w_param >> 16) & 0xFFFF;
            if !st_ptr.is_null() {
                let st = &mut *st_ptr;

                if ctl_id == IDC_P1_DIR_EDIT && notify_code == EN_CHANGE && st.page == WizardPage::Folder {
                    refresh_page1_detection(hwnd, st);
                    return 0;
                }

                if (IDC_P2_MOD_BASE..IDC_P2_MOD_BASE + page2_mod_count()).contains(&ctl_id) {
                    refresh_page2_summary(hwnd, st);
                    return 0;
                }

                match ctl_id {
                    IDC_P1_BROWSE_BTN => {
                        if let Some(chosen) = browse_folder_dialog(hwnd) {
                            let final_path = if chosen
                                .file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| {
                                    n.eq_ignore_ascii_case(INTERNAL_DIR_NAME)
                                        || n.eq_ignore_ascii_case(LEGACY_DIR_NAME)
                                })
                            {
                                chosen
                            } else {
                                chosen.join(INTERNAL_DIR_NAME)
                            };
                            set_ctrl_text(hwnd, IDC_P1_DIR_EDIT, &final_path.to_string_lossy());
                            refresh_page1_detection(hwnd, st);
                        }
                    }
                    IDC_P1_UNINSTALL_BTN => {
                        if !st.existing_in_dir {
                            if let Some(detected) = detect_existing_install_dir() {
                                st.opts.install_dir = detected;
                            }
                        }
                        show_page(hwnd, st, WizardPage::UninstallConfirm);
                    }
                    IDC_P3_DETAILS_BTN => {
                        st.details_expanded = !st.details_expanded;
                        show_ctrl(hwnd, IDC_P3_DETAILS_EDIT, st.details_expanded);
                        set_ctrl_text(
                            hwnd,
                            IDC_P3_DETAILS_BTN,
                            if st.details_expanded {
                                "Ocultar detalhes"
                            } else {
                                "Ver detalhes"
                            },
                        );
                    }
                    IDC_BACK_BTN => match st.page {
                        WizardPage::Modifications | WizardPage::UninstallConfirm => {
                            show_page(hwnd, st, WizardPage::Folder);
                        }
                        _ => {}
                    },
                    IDC_NEXT_BTN => match st.page {
                        WizardPage::Folder => {
                            refresh_page1_detection(hwnd, st);
                            set_ctrl_text(hwnd, IDC_P1_DETECT_LBL, "Buscando modificações disponíveis…");
                            // Ja vem carregado do prefetch; espera no maximo ~4s se ainda nao.
                            if catalog().online {
                                show_page(hwnd, st, WizardPage::Modifications);
                            } else {
                                // Sem internet: pula a etapa e instala o Go Live - Lite padrao.
                                // Quem ja tinha modificacoes embutidas continua com elas.
                                st.opts.selected_modifications = if st.existing_in_dir {
                                    st.existing_mods
                                        .iter()
                                        .filter(|m| BUILTIN_MODS.contains(&m.as_str()))
                                        .cloned()
                                        .collect()
                                } else {
                                    Vec::new()
                                };
                                start_background_worker(hwnd, st, false);
                                append_details_line(
                                    hwnd,
                                    st,
                                    "[Info] Sem internet: etapa de modificações pulada, instalando o Go Live - Lite padrão.",
                                );
                            }
                        }
                        WizardPage::Modifications => {
                            refresh_page2_summary(hwnd, st);
                            start_background_worker(hwnd, st, false);
                        }
                        WizardPage::Progress => {
                            if st.worker_finished {
                                if st.worker_was_uninstall {
                                    let _ = DestroyWindow(hwnd);
                                } else {
                                    show_page(hwnd, st, WizardPage::DoneConfig);
                                }
                            }
                        }
                        WizardPage::DoneConfig => {
                            st.opts.desktop_shortcut = is_checked(hwnd, IDC_P4_CHK_DESKTOP);
                            st.opts.start_with_windows = is_checked(hwnd, IDC_P4_CHK_AUTOSTART);
                            st.opts.minimize_to_tray_on_close =
                                is_checked(hwnd, IDC_P4_CHK_CLOSE_TRAY);
                            st.opts.launch_after = is_checked(hwnd, IDC_P4_CHK_LAUNCH);
                            match finalize_install_preferences(
                                &st.opts,
                                st.installed_exe_bytes,
                                st.used_slim_unins,
                            ) {
                                Ok(_) => {
                                    let _ = DestroyWindow(hwnd);
                                }
                                Err(err) => {
                                    let msg_w = to_wide_null(&err);
                                    let cap_w = to_wide_null("Erro ao salvar as configurações");
                                    let _ = MessageBoxW(
                                        hwnd,
                                        msg_w.as_ptr(),
                                        cap_w.as_ptr(),
                                        MB_OK | MB_ICONERROR,
                                    );
                                }
                            }
                        }
                        WizardPage::UninstallConfirm => {
                            start_background_worker(hwnd, st, true);
                        }
                    },
                    IDC_CANCEL_BTN => {
                        if !st.worker_busy {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    _ => {}
                }
            }
            0
        }
        WM_CLOSE => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            let busy = !st_ptr.is_null() && (*st_ptr).worker_busy;
            if !busy {
                let _ = DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            let st_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WizardState;
            if !st_ptr.is_null() {
                let st = &mut *st_ptr;
                if st.font_regular != 0 {
                    let _ = DeleteObject(st.font_regular);
                    st.font_regular = 0;
                }
                if st.font_bold != 0 {
                    let _ = DeleteObject(st.font_bold);
                    st.font_bold = 0;
                }
                if st.font_title != 0 {
                    let _ = DeleteObject(st.font_title);
                    st.font_title = 0;
                }
                if st.header_icon != 0 {
                    let _ = DestroyIcon(st.header_icon);
                    st.header_icon = 0;
                }
                if st.wnd_icon_big != 0 {
                    let _ = DestroyIcon(st.wnd_icon_big);
                    st.wnd_icon_big = 0;
                }
                if st.wnd_icon_sm != 0 {
                    let _ = DestroyIcon(st.wnd_icon_sm);
                    st.wnd_icon_sm = 0;
                }
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

fn run_wizard_gui(mut opts: SetupOptions) -> i32 {
    if !opts.uninstall {
        prefetch_catalog();
    }
    if !opts.uninstall && !opts.mods_specified {
        if let Some(detected) = detect_existing_install_dir() {
            if opts.install_dir == default_install_dir() {
                opts.install_dir = detected;
            }
        }
    }

    unsafe {
        let icc = InitCommonControlsEx {
            dw_size: size_of::<InitCommonControlsEx>() as u32,
            dw_icc: 0x0000_0020 | 0x0000_4000, // ICC_PROGRESS_CLASS | ICC_STANDARD_CLASSES
        };
        let _ = InitCommonControlsEx(&icc);

        let h_inst = GetModuleHandleW(null());
        let cls_w = to_wide_null("GoLiveLiteSetupWizardWnd");
        let title_w = to_wide_null(if opts.uninstall {
            "Desinstalar Go Live - Lite"
        } else {
            "Instalador do Go Live - Lite"
        });

        let h_icon = LoadImageW(h_inst, 1 as *const u16, IMAGE_ICON, 32, 32, LR_SHARED);
        let h_icon_sm = LoadImageW(h_inst, 1 as *const u16, IMAGE_ICON, 16, 16, LR_SHARED);
        let wc = WndClassExW {
            cb_size: size_of::<WndClassExW>() as u32,
            style: 0,
            lpfn_wnd_proc: wizard_wnd_proc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: h_inst,
            h_icon,
            h_cursor: LoadCursorW(0, IDC_ARROW as *const u16),
            hbr_background: look::content_brush(),
            lpsz_menu_name: null(),
            lpsz_class_name: cls_w.as_ptr(),
            h_icon_sm,
        };
        if RegisterClassExW(&wc) == 0 {
            return 1;
        }

        // Tamanho da area cliente (barra lateral + conteudo) escalado pelo DPI da tela.
        look::init_dpi();
        let mut wr = look::Rect {
            left: 0,
            top: 0,
            right: look::s(look::SIDEBAR_W + look::CONTENT_W),
            bottom: look::s(look::CLIENT_H),
        };
        let _ = look::AdjustWindowRectEx(&mut wr, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, 0, 0);
        let win_w = wr.right - wr.left;
        let win_h = wr.bottom - wr.top;
        let scr_w = GetSystemMetrics(SM_CXSCREEN);
        let scr_h = GetSystemMetrics(SM_CYSCREEN);
        let x = ((scr_w - win_w) / 2).max(40);
        let y = ((scr_h - win_h) / 2).max(40);

        let start_page = if opts.uninstall {
            WizardPage::UninstallConfirm
        } else {
            WizardPage::Folder
        };
        let existing_in_dir = is_existing_install_in_dir(&opts.install_dir);
        let existing_mods = if existing_in_dir {
            read_installed_modifications(&opts.install_dir)
        } else {
            Vec::new()
        };

        let mut state = Box::new(WizardState {
            opts,
            page: start_page,
            existing_in_dir,
            existing_mods,
            worker_busy: false,
            worker_finished: false,
            worker_was_uninstall: false,
            installed_exe_bytes: 0,
            used_slim_unins: false,
            details_expanded: false,
            details_log: String::new(),
            page2_built: false,
            font_regular: 0,
            font_bold: 0,
            font_title: 0,
            font_small: 0,
            header_icon: 0,
            wnd_icon_big: 0,
            wnd_icon_sm: 0,
        });
        let hwnd = CreateWindowExW(
            0,
            cls_w.as_ptr(),
            title_w.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x,
            y,
            win_w,
            win_h,
            0,
            0,
            h_inst,
            (&mut *state as *mut WizardState) as *const c_void,
        );
        if hwnd == 0 {
            return 1;
        }

        let _ = ShowWindow(hwnd, SW_SHOWNORMAL.max(SW_SHOW));
        let _ = UpdateWindow(hwnd);

        let mut msg: Msg = zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }
        0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cur_exe = std::env::current_exe().ok();
    let opts = parse_setup_args(&args, cur_exe.as_deref());

    if opts.silent {
        let res = if opts.uninstall {
            let mut noop = |_pct: u32, _st: &str, _lg: &str| {};
            perform_uninstall_with_progress(&opts, &mut noop)
        } else {
            perform_install(&opts).map(|_| ())
        };
        if let Err(err) = res {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return;
    }

    let _ = MB_ICONINFORMATION;
    let code = run_wizard_gui(opts);
    if code != 0 {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decompress_zlib_bytes, merge_config_settings, parse_setup_args,
        read_installed_modifications, write_modifications_state, INTERNAL_DIR_NAME,
    };
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn test_zlib_roundtrip_and_config_merge() {
        let sample = b"MZ-golive-lite-binary-sample-1234567890";
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(sample).unwrap();
        let compressed = enc.finish().unwrap();
        let restored = decompress_zlib_bytes(&compressed).unwrap();
        assert_eq!(restored, sample);

        let existing = r#"{
  "display_name": "Admin",
  "account_token": "tok_secret_123",
  "start_with_windows": false,
  "minimize_to_tray_on_close": true
}"#;
        let mods = vec!["ferramentas".to_string(), "perfil".to_string()];
        let merged = merge_config_settings(existing, true, false, &mods);
        let parsed: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(parsed["display_name"], "Admin");
        assert_eq!(parsed["account_token"], "tok_secret_123");
        assert_eq!(parsed["start_with_windows"], true);
        assert_eq!(parsed["minimize_to_tray_on_close"], false);
        assert_eq!(
            parsed["enabled_modifications"],
            serde_json::json!(["ferramentas", "perfil"])
        );
    }

    #[test]
    fn test_cli_args_parsing() {
        let install_args = vec![
            "--silent".to_string(),
            "--no-registry".to_string(),
            "--dir=C:\\Temp\\GoLiveTest".to_string(),
            "--mods=ferramentas,perfil,desconhecida".to_string(),
            "--autostart=1".to_string(),
            "--close-to-tray=0".to_string(),
            "--desktop-shortcut=0".to_string(),
        ];
        let opts = parse_setup_args(
            &install_args,
            Some(Path::new("C:\\Downloads\\GoLiveLite-Setup.exe")),
        );
        assert!(!opts.uninstall);
        assert!(opts.silent);
        assert!(opts.no_registry);
        assert!(opts.mods_specified);
        assert_eq!(
            opts.selected_modifications,
            vec!["ferramentas".to_string(), "perfil".to_string()]
        );
        assert_eq!(opts.install_dir, Path::new("C:\\Temp\\GoLiveTest"));
        assert!(opts.start_with_windows);
        assert!(!opts.minimize_to_tray_on_close);
        assert!(!opts.desktop_shortcut);
        assert!(!opts.launch_after);

        let unins_opts = parse_setup_args(
            &["--silent".to_string()],
            Some(Path::new("C:\\Apps\\Go Live Lite - Desktop\\unins000.exe")),
        );
        assert!(unins_opts.uninstall);
        assert!(unins_opts.silent);
        assert_eq!(
            unins_opts.install_dir,
            Path::new("C:\\Apps\\Go Live Lite - Desktop")
        );
        assert_eq!(INTERNAL_DIR_NAME, "Go Live Lite - Desktop");
    }

    #[test]
    fn test_modifications_install_update_and_remove_cycle() {
        let tmp = std::env::temp_dir().join(format!("golive_mods_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut logs = Vec::new();
        write_modifications_state(
            &tmp,
            &["ferramentas".to_string(), "perfil".to_string()],
            &mut |l| logs.push(l.to_string()),
        )
        .unwrap();
        assert!(tmp.join("mods/ferramentas.mod.json").is_file());
        assert!(tmp.join("mods/perfil.mod.json").is_file());
        assert!(!tmp.join("mods/permissoes_chamada.mod.json").is_file());
        assert_eq!(
            read_installed_modifications(&tmp),
            vec!["ferramentas".to_string(), "perfil".to_string()]
        );

        // Atualiza instalação existente trocando para "permissoes_chamada"
        write_modifications_state(
            &tmp,
            &["permissoes_chamada".to_string()],
            &mut |l| logs.push(l.to_string()),
        )
        .unwrap();
        assert!(!tmp.join("mods/ferramentas.mod.json").is_file());
        assert!(!tmp.join("mods/perfil.mod.json").is_file());
        assert!(tmp.join("mods/permissoes_chamada.mod.json").is_file());
        assert_eq!(
            read_installed_modifications(&tmp),
            vec!["permissoes_chamada".to_string()]
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_remote_catalog_filters_and_sanitizes() {
        let raw = br#"{"modifications":[
            {"id":"ferramentas","name":"Ferramentas","description":"x","version":"1.2.0","status":"active"},
            {"id":"temas","name":"Temas","status":"coming-soon"},
            {"id":"velha","name":"Velha","status":"disabled"},
            {"id":"../hack","name":"Ruim"},
            {"id":"extra","name":"Extra","downloadUrl":"http://inseguro/a.bin","fileName":"..\\x.dll"},
            {"id":"ferramentas","name":"Duplicada"}
        ]}"#;
        let mods = super::parse_remote_catalog(raw).expect("json valido");
        let ids: Vec<&str> = mods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["ferramentas", "temas", "extra"]);
        assert_eq!(mods[0].version, "1.2.0");
        assert!(mods[1].coming_soon && !mods[0].coming_soon);
        // so https e nome de arquivo sem caminho
        assert!(mods[2].download_url.is_none() && mods[2].file_name.is_none());
        assert!(super::parse_remote_catalog(b"<html>erro</html>").is_none());
    }

    #[test]
    fn test_modular_payload_selection_and_sha256() {
        assert_eq!(super::select_payload_kind(&[]), super::PayloadKind::Core);
        assert_eq!(
            super::select_payload_kind(&["ferramentas".to_string()]),
            super::PayloadKind::Mods
        );
        assert_eq!(
            super::build_download_artifact_url("https://cdn.example.com/releases/", "golive-lite-core.exe"),
            "https://cdn.example.com/releases/golive-lite-core.exe"
        );
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

/// `GetDlgItem` que tambem procura dentro do painel com rolagem da etapa 2.
fn dlg_item(hwnd: isize, id: usize) -> isize {
    let item = unsafe { GetDlgItem(hwnd, id as i32) };
    if item != 0 {
        item
    } else {
        mods_panel::find(id)
    }
}

/// `spawn_ctrl` para dentro do painel de modificacoes: coordenadas relativas a ele (sem a
/// barra lateral), escaladas pelo DPI.
#[allow(clippy::too_many_arguments)]
unsafe fn spawn_in_panel(
    panel: isize,
    h_inst: isize,
    class_name: &str,
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    font: isize,
) -> isize {
    let cls_w = to_wide_null(class_name);
    let txt_w = to_wide_null(text);
    let hwnd = CreateWindowExW(
        0,
        cls_w.as_ptr(),
        txt_w.as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        look::s(x),
        look::s(y),
        look::s(w),
        look::s(h),
        panel,
        id as isize,
        h_inst,
        null(),
    );
    if hwnd != 0 && font != 0 {
        let _ = SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
    }
    hwnd
}
