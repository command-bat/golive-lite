use std::collections::VecDeque;
use std::ffi::c_void;
use std::mem::{size_of, ManuallyDrop};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use windows::core::{IInspectable, Interface, PWSTR, VARIANT};
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{BOOL, HMODULE, HWND, LUID, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BIND_VIDEO_ENCODER, D3D11_BOX, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CAPS,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE,
    D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_MODE_ROTATION_IDENTITY,
    DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIDevice3, IDXGIFactory1,
    IDXGIOutput, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication, DXGI_ADAPTER_DESC1,
    DXGI_ERROR_ACCESS_LOST,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_POINTER_SHAPE_INFO,
    DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR, DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR,
    DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME, DXGI_OUTPUT_DESC,
};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, HMONITOR, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Base,
    eAVEncH264VProfile_ConstrainedBase, eAVEncH264VProfile_Main, ICodecAPI, IMF2DBuffer,
    IMFActivate, IMFAttributes, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaEventGenerator,
    IMFSample, IMFShutdown, IMFTransform, MEError, MFCreateAttributes, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFNominalRange_16_235, MFStartup,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709,
    MFVideoTransFunc_709, MFVideoTransferMatrix_BT709, CODECAPI_AVEncCommonBufferSize,
    CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonQualityVsSpeed,
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVEncVideoMaxNumRefFrame,
    CODECAPI_AVLowLatencyMode, MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS, METransformHaveOutput,
    METransformNeedInput, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_ADAPTER_LUID,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_FRIENDLY_NAME_Attribute,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_DATA_BUFFER_INCOMPLETE,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MFSTARTUP_FULL, MF_EVENT_FLAG_NO_WAIT, MF_E_NO_EVENTS_AVAILABLE,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use crate::media::codecs::H264ParameterSets;
#[cfg(test)]
use crate::media::codecs::inspect_h264_au;
use crate::media::screen_capture::{
    compute_preview_dimensions, HighResTimerGuard, MonitorTarget, WindowTarget,
};

const NV12_POOL_SIZE: usize = 3;
const PREVIEW_INTERVAL_MS: u64 = 50; // ~20 fps nitido para preview local sem bloquear o encoder
const STATIC_HEARTBEAT_MS: u64 = 1_000; // 1 fps quando a tela esta totalmente estatica
const DUPL_LOST_TO_WGC_MS: u64 = 1_000;
const ACCESS_LOST_TIMEOUT_MS: u64 = 10_000;
const HRESULT_MF_E_SHUTDOWN: i32 = 0xC00D_3E85_u32 as i32;
const HRESULT_MF_E_NOTACCEPTING: i32 = 0xC00D_36B5_u32 as i32;
const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
const D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH: i32 = 4;

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn GetPriorityClass(hProcess: isize) -> u32;
    fn SetPriorityClass(hProcess: isize, dwPriorityClass: u32) -> i32;
    fn LoadLibraryA(lpLibFileName: *const u8) -> isize;
    fn GetProcAddress(hModule: isize, lpProcName: *const u8) -> *const c_void;
    fn FreeLibrary(hLibModule: isize) -> i32;
}

fn load_proc_cached(slot: &OnceLock<usize>, dll: &[u8], proc_name: &[u8]) -> Option<usize> {
    let addr = *slot.get_or_init(|| unsafe {
        let m = LoadLibraryA(dll.as_ptr());
        if m == 0 {
            return 0;
        }
        let p = GetProcAddress(m, proc_name.as_ptr());
        if p.is_null() {
            0
        } else {
            p as usize
        }
    });
    if addr == 0 {
        None
    } else {
        Some(addr)
    }
}

unsafe fn dyn_mf_create_dxgi_surface_buffer(
    riid: &windows::core::GUID,
    tex: &ID3D11Texture2D,
) -> Result<IMFMediaBuffer, String> {
    static FN: OnceLock<usize> = OnceLock::new();
    let addr = load_proc_cached(&FN, b"mfplat.dll\0", b"MFCreateDXGISurfaceBuffer\0")
        .ok_or_else(|| "MFCreateDXGISurfaceBuffer indisponivel neste Windows.".to_string())?;
    type FnTy = unsafe extern "system" fn(
        *const windows::core::GUID,
        *mut c_void,
        u32,
        BOOL,
        *mut *mut c_void,
    ) -> windows::core::HRESULT;
    let func: FnTy = std::mem::transmute(addr);
    let mut raw: *mut c_void = std::ptr::null_mut();
    let hr = func(riid, tex.as_raw(), 0, BOOL(0), &mut raw);
    hr.ok()
        .map_err(|e| format!("MFCreateDXGISurfaceBuffer falhou: {}", e))?;
    if raw.is_null() {
        return Err("IMFMediaBuffer nulo.".to_string());
    }
    Ok(IMFMediaBuffer::from_raw(raw))
}

unsafe fn dyn_mf_create_dxgi_device_manager() -> Result<(u32, IMFDXGIDeviceManager), String> {
    static FN: OnceLock<usize> = OnceLock::new();
    let addr = load_proc_cached(&FN, b"mfplat.dll\0", b"MFCreateDXGIDeviceManager\0")
        .ok_or_else(|| "MFCreateDXGIDeviceManager indisponivel neste Windows.".to_string())?;
    type FnTy = unsafe extern "system" fn(*mut u32, *mut *mut c_void) -> windows::core::HRESULT;
    let func: FnTy = std::mem::transmute(addr);
    let mut token = 0u32;
    let mut raw: *mut c_void = std::ptr::null_mut();
    let hr = func(&mut token, &mut raw);
    hr.ok()
        .map_err(|e| format!("MFCreateDXGIDeviceManager falhou: {}", e))?;
    if raw.is_null() {
        return Err("IMFDXGIDeviceManager nulo.".to_string());
    }
    Ok((token, IMFDXGIDeviceManager::from_raw(raw)))
}

unsafe fn dyn_mft_enum2(
    category: windows::core::GUID,
    flags: u32,
    input_type: &MFT_REGISTER_TYPE_INFO,
    output_type: &MFT_REGISTER_TYPE_INFO,
    attrs: Option<&IMFAttributes>,
    out_ptr: *mut *mut Option<IMFActivate>,
    out_count: *mut u32,
) -> windows::core::Result<()> {
    static FN: OnceLock<usize> = OnceLock::new();
    let Some(addr) = load_proc_cached(&FN, b"mfplat.dll\0", b"MFTEnum2\0") else {
        return Err(windows::core::Error::from_win32());
    };
    type FnTy = unsafe extern "system" fn(
        windows::core::GUID,
        u32,
        *const MFT_REGISTER_TYPE_INFO,
        *const MFT_REGISTER_TYPE_INFO,
        *mut c_void,
        *mut *mut Option<IMFActivate>,
        *mut u32,
    ) -> windows::core::HRESULT;
    let func: FnTy = std::mem::transmute(addr);
    let attr_raw = attrs.map_or(std::ptr::null_mut(), |a| a.as_raw());
    func(
        category,
        flags,
        input_type,
        output_type,
        attr_raw,
        out_ptr,
        out_count,
    )
    .ok()
}

unsafe fn dyn_create_d3d11_device_from_dxgi_device(
    dxgi_device: &IDXGIDevice,
) -> Result<IInspectable, String> {
    static FN: OnceLock<usize> = OnceLock::new();
    let addr = load_proc_cached(
        &FN,
        b"d3d11.dll\0",
        b"CreateDirect3D11DeviceFromDXGIDevice\0",
    )
    .ok_or_else(|| "CreateDirect3D11DeviceFromDXGIDevice indisponivel neste Windows.".to_string())?;
    type FnTy = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> windows::core::HRESULT;
    let func: FnTy = std::mem::transmute(addr);
    let mut raw: *mut c_void = std::ptr::null_mut();
    let hr = func(dxgi_device.as_raw(), &mut raw);
    hr.ok()
        .map_err(|e| format!("CreateDirect3D11DeviceFromDXGIDevice falhou: {}", e))?;
    if raw.is_null() {
        return Err("IDirect3DDevice nulo.".to_string());
    }
    Ok(IInspectable::from_raw(raw))
}

/// Wrapper para ponteiros COM em MTA (`COINIT_MULTITHREADED`) enviados entre threads dedicadas.
struct SendCom<T>(T);
unsafe impl<T> Send for SendCom<T> {}
unsafe impl<T> Sync for SendCom<T> {}
impl<T> SendCom<T> {
    #[inline]
    fn into_inner(self) -> T {
        self.0
    }
}

#[derive(Debug, Clone)]
pub enum GpuCaptureSource {
    Monitor(MonitorTarget),
    Window(WindowTarget),
}

pub struct GpuCaptureConfig {
    pub source: GpuCaptureSource,
    pub out_width: u32,
    pub out_height: u32,
    pub target_fps: u32,
    pub bitrate_bps: u32,
    pub preview_enabled: Arc<AtomicBool>,
}

pub struct GpuEncoderInfo {
    pub encoder_name: String,
    pub width: u32,
    pub height: u32,
    pub target_fps: u32,
}

static MF_INIT_ONCE: OnceLock<Result<(), String>> = OnceLock::new();
static PRIO_ACTIVE_COUNT: AtomicUsize = AtomicUsize::new(0);
static PRIO_SAVED_CLASS: AtomicU32 = AtomicU32::new(0);

fn ensure_mf_started() -> Result<(), String> {
    MF_INIT_ONCE
        .get_or_init(|| {
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
                .map_err(|e| format!("MFStartup falhou: {}", e))
        })
        .clone()
}

struct ComMfGuard {
    com_init: bool,
}

impl ComMfGuard {
    fn init() -> Result<Self, String> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_init = hr.0 == 0 || hr.0 == 1;
        if hr.is_err() && hr.0 != -2147417850_i32 {
            return Err(format!("CoInitializeEx falhou: 0x{:08x}", hr.0 as u32));
        }

        if let Err(e) = ensure_mf_started() {
            if com_init {
                unsafe { CoUninitialize() };
            }
            return Err(e);
        }

        Ok(Self { com_init })
    }
}

impl Drop for ComMfGuard {
    fn drop(&mut self) {
        if self.com_init {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

struct PriorityGuard {
    dxgi_device: Option<IDXGIDevice>,
}

impl PriorityGuard {
    fn enter(device: &ID3D11Device) -> Self {
        let proc_h = unsafe { GetCurrentProcess() };
        if PRIO_ACTIVE_COUNT.fetch_add(1, Ordering::AcqRel) == 0 {
            let prev = unsafe { GetPriorityClass(proc_h) };
            if prev != 0 && prev != ABOVE_NORMAL_PRIORITY_CLASS {
                if unsafe { SetPriorityClass(proc_h, ABOVE_NORMAL_PRIORITY_CLASS) } != 0 {
                    PRIO_SAVED_CLASS.store(prev, Ordering::Release);
                }
            }

            // Eleva prioridade de escalonamento WDDM igual ao videocap.cpp para competir com jogos
            unsafe {
                let gdi = LoadLibraryA(b"gdi32.dll\0".as_ptr());
                if gdi != 0 {
                    let sym =
                        GetProcAddress(gdi, b"D3DKMTSetProcessSchedulingPriorityClass\0".as_ptr());
                    if !sym.is_null() {
                        let func: unsafe extern "system" fn(isize, i32) -> i32 =
                            std::mem::transmute(sym);
                        let _ = func(proc_h, D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH);
                    }
                    let _ = FreeLibrary(gdi);
                }
            }
        }

        let dxgi_device = device.cast::<IDXGIDevice>().ok();
        if let Some(ref dd) = dxgi_device {
            let _ = unsafe { dd.SetGPUThreadPriority(7) };
        }

        Self { dxgi_device }
    }
}

impl Drop for PriorityGuard {
    fn drop(&mut self) {
        if let Some(ref dd) = self.dxgi_device {
            let _ = unsafe { dd.SetGPUThreadPriority(0) };
        }
        if PRIO_ACTIVE_COUNT.fetch_sub(1, Ordering::AcqRel) == 1 {
            let saved = PRIO_SAVED_CLASS.swap(0, Ordering::AcqRel);
            if saved != 0 {
                unsafe {
                    let _ = SetPriorityClass(GetCurrentProcess(), saved);
                }
            }
        }
    }
}

struct MmcssGuard {
    avrt_mod: isize,
    task_handle: isize,
}

impl MmcssGuard {
    fn enter_capture() -> Self {
        let avrt_mod = unsafe { LoadLibraryA(b"avrt.dll\0".as_ptr()) };
        if avrt_mod == 0 {
            return Self {
                avrt_mod: 0,
                task_handle: 0,
            };
        }
        let sym = unsafe { GetProcAddress(avrt_mod, b"AvSetMmThreadCharacteristicsW\0".as_ptr()) };
        if sym.is_null() {
            unsafe {
                let _ = FreeLibrary(avrt_mod);
            }
            return Self {
                avrt_mod: 0,
                task_handle: 0,
            };
        }
        let task_name: [u16; 8] = [
            b'C' as u16,
            b'a' as u16,
            b'p' as u16,
            b't' as u16,
            b'u' as u16,
            b'r' as u16,
            b'e' as u16,
            0,
        ];
        let mut task_idx = 0u32;
        let func: unsafe extern "system" fn(*const u16, *mut u32) -> isize =
            unsafe { std::mem::transmute(sym) };
        let task_handle = unsafe { func(task_name.as_ptr(), &mut task_idx) };
        Self {
            avrt_mod,
            task_handle,
        }
    }
}

impl Drop for MmcssGuard {
    fn drop(&mut self) {
        if self.avrt_mod != 0 {
            if self.task_handle != 0 {
                let sym = unsafe {
                    GetProcAddress(self.avrt_mod, b"AvRevertMmThreadCharacteristics\0".as_ptr())
                };
                if !sym.is_null() {
                    let func: unsafe extern "system" fn(isize) -> i32 =
                        unsafe { std::mem::transmute(sym) };
                    unsafe {
                        let _ = func(self.task_handle);
                    }
                }
            }
            unsafe {
                let _ = FreeLibrary(self.avrt_mod);
            }
        }
    }
}

#[inline]
fn pack_u64(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

/// Calcula o FPS maximo de cabecalho dentro do limite H.264 Level 5.2 (2.073.600 MB/s),
/// garantindo SPS compativel com navegadores e Electron sem limitar o ritmo real dos timestamps.
pub fn compute_mft_header_fps(out_w: u32, out_h: u32, target_fps: u32) -> u32 {
    let mb_w = out_w.div_ceil(16).max(1);
    let mb_h = out_h.div_ceil(16).max(1);
    let mb_per_frame = mb_w.saturating_mul(mb_h).max(1);
    const H264_L52_MAX_MBPS: u32 = 2_073_600;
    let max_fps_l52 = (H264_L52_MAX_MBPS / mb_per_frame).clamp(15, 240);
    target_fps.clamp(5, 240).min(max_fps_l52)
}

fn utf16_z_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn create_duplication(output: &IDXGIOutput, device: &ID3D11Device) -> Result<IDXGIOutputDuplication, String> {
    if let Ok(o5) = output.cast::<IDXGIOutput5>() {
        let fmts = [DXGI_FORMAT_B8G8R8A8_UNORM];
        if let Ok(dupl) = unsafe { o5.DuplicateOutput1(device, 0, &fmts) } {
            return Ok(dupl);
        }
    }
    let o1 = output
        .cast::<IDXGIOutput1>()
        .map_err(|e| format!("IDXGIOutput1 indisponivel: {}", e))?;
    unsafe { o1.DuplicateOutput(device) }.map_err(|e| format!("DuplicateOutput falhou: {}", e))
}

fn find_dxgi_adapter_and_output(
    monitor: &MonitorTarget,
) -> Result<(IDXGIAdapter1, IDXGIOutput, LUID), String> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|e| format!("CreateDXGIFactory1 falhou: {}", e))?;

    let mut fallback: Option<(IDXGIAdapter1, IDXGIOutput, LUID)> = None;
    let mut adapter_idx = 0u32;

    loop {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(adapter_idx) }) else {
            break;
        };
        adapter_idx += 1;

        let mut adesc = DXGI_ADAPTER_DESC1::default();
        if unsafe { adapter.GetDesc1(&mut adesc) }.is_err() {
            continue;
        }

        let mut out_idx = 0u32;
        loop {
            let Ok(output) = (unsafe { adapter.EnumOutputs(out_idx) }) else {
                break;
            };
            out_idx += 1;

            let mut odesc = DXGI_OUTPUT_DESC::default();
            if unsafe { output.GetDesc(&mut odesc) }.is_err() {
                continue;
            }
            if !odesc.AttachedToDesktop.as_bool() {
                continue;
            }

            let out_name = utf16_z_to_string(&odesc.DeviceName);
            let rect_match = odesc.DesktopCoordinates.left == monitor.x
                && odesc.DesktopCoordinates.top == monitor.y;
            if (!monitor.name.is_empty() && out_name.eq_ignore_ascii_case(&monitor.name)) || rect_match {
                return Ok((adapter, output, adesc.AdapterLuid));
            }

            if fallback.is_none() {
                fallback = Some((adapter.clone(), output, adesc.AdapterLuid));
            }
        }
    }

    fallback.ok_or_else(|| "Nenhum IDXGIOutput ativo encontrado.".to_string())
}

fn find_dxgi_adapter_for_hmonitor(hmonitor: HMONITOR) -> Result<(IDXGIAdapter1, LUID), String> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|e| format!("CreateDXGIFactory1 falhou: {}", e))?;

    let mut fallback: Option<(IDXGIAdapter1, LUID)> = None;
    let mut adapter_idx = 0u32;

    loop {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(adapter_idx) }) else {
            break;
        };
        adapter_idx += 1;

        let mut adesc = DXGI_ADAPTER_DESC1::default();
        if unsafe { adapter.GetDesc1(&mut adesc) }.is_err() {
            continue;
        }
        if fallback.is_none() {
            fallback = Some((adapter.clone(), adesc.AdapterLuid));
        }

        let mut out_idx = 0u32;
        loop {
            let Ok(output) = (unsafe { adapter.EnumOutputs(out_idx) }) else {
                break;
            };
            out_idx += 1;
            let mut odesc = DXGI_OUTPUT_DESC::default();
            if unsafe { output.GetDesc(&mut odesc) }.is_ok()
                && hmonitor.0 != 0
                && odesc.Monitor == hmonitor
            {
                return Ok((adapter, adesc.AdapterLuid));
            }
        }
    }

    fallback.ok_or_else(|| "Nenhum adaptador DXGI disponivel.".to_string())
}

fn create_nv12_texture(device: &ID3D11Device, width: u32, height: u32) -> Result<ID3D11Texture2D, String> {
    let flags_try = [
        D3D11_BIND_RENDER_TARGET.0 as u32,
        D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_VIDEO_ENCODER.0 as u32,
    ];
    for bind_flags in flags_try {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind_flags,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        if unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex)) }.is_ok() {
            if let Some(t) = tex {
                return Ok(t);
            }
        }
    }
    Err("Falha ao criar textura NV12 em VRAM.".to_string())
}

fn create_bgra_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    usage_staging: bool,
) -> Result<ID3D11Texture2D, String> {
    let desc = if usage_staging {
        D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        }
    } else {
        D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        }
    };
    let mut tex: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex)) }
        .map_err(|e| format!("CreateTexture2D BGRA falhou: {}", e))?;
    tex.ok_or_else(|| "CreateTexture2D BGRA retornou vazio.".to_string())
}

struct Nv12Slot {
    tex: ID3D11Texture2D,
    ov: ID3D11VideoProcessorOutputView,
    busy: bool,
}

struct Nv12TexturePool {
    slots: Mutex<Vec<Nv12Slot>>,
}

impl Nv12TexturePool {
    fn new(
        device: &ID3D11Device,
        video_device: &ID3D11VideoDevice,
        vp_enum: &ID3D11VideoProcessorEnumerator,
        out_w: u32,
        out_h: u32,
    ) -> Result<Self, String> {
        let mut slots = Vec::with_capacity(NV12_POOL_SIZE);
        for _ in 0..NV12_POOL_SIZE {
            let tex = create_nv12_texture(device, out_w, out_h)?;
            let ov_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };
            let mut ov_opt: Option<ID3D11VideoProcessorOutputView> = None;
            unsafe {
                video_device.CreateVideoProcessorOutputView(&tex, vp_enum, &ov_desc, Some(&mut ov_opt))
            }
            .map_err(|e| format!("CreateVideoProcessorOutputView(NV12) falhou: {}", e))?;
            let ov = ov_opt.ok_or_else(|| "NV12 VideoProcessorOutputView nulo.".to_string())?;
            slots.push(Nv12Slot {
                tex,
                ov,
                busy: false,
            });
        }
        Ok(Self {
            slots: Mutex::new(slots),
        })
    }

    fn try_acquire(&self) -> Option<(usize, ID3D11VideoProcessorOutputView)> {
        let mut guard = self.slots.lock().ok()?;
        let (idx, slot) = guard.iter_mut().enumerate().find(|(_, s)| !s.busy)?;
        slot.busy = true;
        Some((idx, slot.ov.clone()))
    }

    #[cfg(test)]
    fn acquire_free(&self, timeout: Duration) -> Option<(usize, ID3D11VideoProcessorOutputView)> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(res) = self.try_acquire() {
                return Some(res);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn create_sample_for_slot(
        &self,
        slot_idx: usize,
        pts_100ns: i64,
        dur_100ns: i64,
    ) -> Result<IMFSample, String> {
        let tex = {
            let guard = self.slots.lock().map_err(|_| "Pool mutex poisoned".to_string())?;
            guard
                .get(slot_idx)
                .ok_or_else(|| "Slot NV12 invalido".to_string())?
                .tex
                .clone()
        };
        let sample = unsafe { MFCreateSample() }
            .map_err(|e| format!("MFCreateSample(NV12) falhou: {}", e))?;
        let buf: IMFMediaBuffer =
            unsafe { dyn_mf_create_dxgi_surface_buffer(&ID3D11Texture2D::IID, &tex) }?;
        if let Ok(buf2d) = buf.cast::<IMF2DBuffer>() {
            if let Ok(contig_len) = unsafe { buf2d.GetContiguousLength() } {
                let _ = unsafe { buf.SetCurrentLength(contig_len) };
            }
        }
        unsafe { sample.AddBuffer(&buf) }
            .map_err(|e| format!("AddBuffer(NV12) falhou: {}", e))?;
        unsafe {
            let _ = sample.SetSampleTime(pts_100ns);
            let _ = sample.SetSampleDuration(dur_100ns);
        }
        Ok(sample)
    }

    fn release(&self, slot_idx: usize) {
        if let Ok(mut guard) = self.slots.lock() {
            if let Some(slot) = guard.get_mut(slot_idx) {
                slot.busy = false;
            }
        }
    }

    #[cfg(test)]
    fn busy_count(&self) -> usize {
        self.slots
            .lock()
            .map(|g| g.iter().filter(|s| s.busy).count())
            .unwrap_or(0)
    }

    fn rebuild_output_views(
        &self,
        video_device: &ID3D11VideoDevice,
        vp_enum: &ID3D11VideoProcessorEnumerator,
    ) -> Result<(), String> {
        let mut guard = self
            .slots
            .lock()
            .map_err(|_| "Pool mutex poisoned".to_string())?;
        let ov_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        for slot in guard.iter_mut() {
            let mut ov_opt: Option<ID3D11VideoProcessorOutputView> = None;
            unsafe {
                video_device.CreateVideoProcessorOutputView(
                    &slot.tex,
                    vp_enum,
                    &ov_desc,
                    Some(&mut ov_opt),
                )
            }
            .map_err(|e| format!("RebuildVideoProcessorOutputView(NV12) falhou: {}", e))?;
            slot.ov = ov_opt
                .ok_or_else(|| "NV12 VideoProcessorOutputView nulo apos resize.".to_string())?;
        }
        Ok(())
    }
}

unsafe impl Send for Nv12TexturePool {}
unsafe impl Sync for Nv12TexturePool {}

struct HardwareMftEncoder {
    mft: IMFTransform,
    in_id: u32,
    out_id: u32,
    is_async: bool,
    event_gen: Option<IMFMediaEventGenerator>,
    codec_api: Option<ICodecAPI>,
    mft_provides_samples: bool,
    scratch_out_sample: Option<IMFSample>,
    encoder_name: String,
    configured_fps_header: u32,
    profile_used: u32,
    out_w: u32,
    out_h: u32,
    bitrate_bps: u32,
}

fn read_activate_friendly_name(act: &IMFActivate) -> String {
    let mut raw = PWSTR::null();
    let mut cch = 0u32;
    if unsafe { act.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut raw, &mut cch) }.is_ok()
        && !raw.is_null()
    {
        let s = unsafe {
            let slice = std::slice::from_raw_parts(raw.0, cch as usize);
            String::from_utf16_lossy(slice)
        };
        unsafe { CoTaskMemFree(Some(raw.0 as *const _)) };
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "Hardware H.264 MFT".to_string()
}

fn enum_hardware_h264_activates(adapter_luid: Option<LUID>) -> Vec<IMFActivate> {
    let in_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let out_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;

    let mut attrs: Option<IMFAttributes> = None;
    if let Some(luid) = adapter_luid {
        if let Ok(()) = unsafe { MFCreateAttributes(&mut attrs, 1) } {
            if let Some(ref a) = attrs {
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        &luid as *const LUID as *const u8,
                        size_of::<LUID>(),
                    )
                };
                let _ = unsafe { a.SetBlob(&MFT_ENUM_ADAPTER_LUID, bytes) };
            }
        }
    }

    let mut ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    let res = unsafe {
        dyn_mft_enum2(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags.0 as u32,
            &in_type,
            &out_type,
            attrs.as_ref(),
            &mut ptr,
            &mut count,
        )
    };
    if res.is_err() || ptr.is_null() || count == 0 {
        if !ptr.is_null() {
            unsafe { CoTaskMemFree(Some(ptr as *const _)) };
        }
        return Vec::new();
    }

    let mut list = Vec::with_capacity(count as usize);
    unsafe {
        for i in 0..(count as usize) {
            if let Some(act) = std::ptr::read(ptr.add(i)) {
                list.push(act);
            }
        }
        CoTaskMemFree(Some(ptr as *const _));
    }
    list
}

fn build_h264_output_type(
    out_w: u32,
    out_h: u32,
    fps_header: u32,
    bitrate_bps: u32,
    profile: u32,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, String> {
    let mt = unsafe { MFCreateMediaType() }
        .map_err(|e| format!("MFCreateMediaType output falhou: {}", e))?;
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|_| mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))
            .and_then(|_| mt.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps))
            .and_then(|_| mt.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(out_w, out_h)))
            .and_then(|_| mt.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(fps_header, 1)))
            .and_then(|_| mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1)))
            .and_then(|_| {
                mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            })
            .and_then(|_| mt.SetUINT32(&MF_MT_MPEG2_PROFILE, profile))
            .map_err(|e| format!("Set attributes output type falhou: {}", e))?;
    }
    Ok(mt)
}

fn build_nv12_input_type(
    out_w: u32,
    out_h: u32,
    fps_header: u32,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, String> {
    let mt = unsafe { MFCreateMediaType() }
        .map_err(|e| format!("MFCreateMediaType input falhou: {}", e))?;
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|_| mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
            .and_then(|_| mt.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(out_w, out_h)))
            .and_then(|_| mt.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(fps_header, 1)))
            .and_then(|_| mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1)))
            .and_then(|_| {
                mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            })
            .map_err(|e| format!("Set attributes input type falhou: {}", e))?;
        let _ = mt.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32);
        let _ = mt.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32);
        let _ = mt.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32);
        let _ = mt.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32);
    }
    Ok(mt)
}

fn try_set_input_type_with_fallback(
    mft: &IMFTransform,
    in_id: u32,
    out_w: u32,
    out_h: u32,
    fps_header: u32,
) -> bool {
    if let Ok(in_mt) = build_nv12_input_type(out_w, out_h, fps_hdr_safe(fps_header)) {
        if unsafe { mft.SetInputType(in_id, &in_mt, 0) }.is_ok() {
            return true;
        }
    }
    for idx in 0..16u32 {
        let Ok(cand) = (unsafe { mft.GetInputAvailableType(in_id, idx) }) else {
            break;
        };
        let is_nv12 = unsafe { cand.GetGUID(&MF_MT_SUBTYPE) }
            .map(|g| g == MFVideoFormat_NV12)
            .unwrap_or(false);
        if !is_nv12 {
            continue;
        }
        unsafe {
            let _ = cand.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(out_w, out_h));
            let _ = cand.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(fps_hdr_safe(fps_header), 1));
            let _ = cand.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1));
            let _ = cand.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32);
            if mft.SetInputType(in_id, &cand, 0).is_ok() {
                return true;
            }
        }
    }
    false
}

#[inline]
fn fps_hdr_safe(fps: u32) -> u32 {
    fps.clamp(5, 240)
}

fn try_activate_hw_encoder(
    act: &IMFActivate,
    dxgi_mgr: &IMFDXGIDeviceManager,
    out_w: u32,
    out_h: u32,
    target_fps: u32,
    bitrate_bps: u32,
) -> Result<HardwareMftEncoder, String> {
    let name = read_activate_friendly_name(act);
    let mft: IMFTransform = unsafe { act.ActivateObject() }
        .map_err(|e| format!("ActivateObject({}) falhou: {}", name, e))?;

    let attrs = unsafe { mft.GetAttributes() }
        .map_err(|e| format!("GetAttributes({}) falhou: {}", name, e))?;

    let d3d11_aware = unsafe { attrs.GetUINT32(&MF_SA_D3D11_AWARE) }.unwrap_or(0);
    if d3d11_aware == 0 {
        return Err(format!("{} nao suporta D3D11", name));
    }

    let is_async = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if is_async {
        unsafe { attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
            .map_err(|e| format!("MF_TRANSFORM_ASYNC_UNLOCK({}) falhou: {}", name, e))?;
    }
    let _ = unsafe { attrs.SetUINT32(&MF_LOW_LATENCY, 1) };

    unsafe { mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, dxgi_mgr.as_raw() as usize) }
        .map_err(|e| format!("SET_D3D_MANAGER({}) falhou: {}", name, e))?;

    let mut in_id = 0u32;
    let mut out_id = 0u32;
    let mut in_ids = [0u32; 4];
    let mut out_ids = [0u32; 4];
    let mut c_in = 0u32;
    let mut c_out = 0u32;
    if unsafe { mft.GetStreamCount(&mut c_in, &mut c_out) }.is_ok()
        && c_in > 0
        && c_out > 0
        && c_in <= 4
        && c_out <= 4
    {
        if unsafe { mft.GetStreamIDs(&mut in_ids[..c_in as usize], &mut out_ids[..c_out as usize]) }
            .is_ok()
        {
            in_id = in_ids[0];
            out_id = out_ids[0];
        }
    }

    // 1. Mantem cabeçalho dentro de H.264 Level 5.2 para compativel 100% com navegadores/Electron
    let fps_hdr_primary = compute_mft_header_fps(out_w, out_h, target_fps);
    let mut fps_candidates = vec![fps_hdr_primary];
    if fps_hdr_primary > 60 {
        fps_candidates.push(60);
    }
    if fps_hdr_primary > 30 {
        fps_candidates.push(30);
    }

    // 2. Perfis estritamente compativeis com WebRTC e decodificacao HW (ConstrainedBase -> Main -> Base)
    let profiles = [
        eAVEncH264VProfile_ConstrainedBase.0 as u32,
        eAVEncH264VProfile_Main.0 as u32,
        eAVEncH264VProfile_Base.0 as u32,
    ];

    let mut negotiated: Option<(u32, u32)> = None;
    'outer: for &fps_hdr in &fps_candidates {
        for &prof in &profiles {
            let Ok(out_mt) = build_h264_output_type(out_w, out_h, fps_hdr, bitrate_bps, prof)
            else {
                continue;
            };
            if unsafe { mft.SetOutputType(out_id, &out_mt, 0) }.is_err() {
                continue;
            }
            if try_set_input_type_with_fallback(&mft, in_id, out_w, out_h, fps_hdr) {
                negotiated = Some((fps_hdr, prof));
                break 'outer;
            }
        }
    }

    let (configured_fps_header, profile_used) =
        negotiated.ok_or_else(|| format!("{}: nenhum formato H264/NV12 aceito", name))?;

    // 3. Configura ICodecAPI APOS SetOutputType/SetInputType (drivers NVENC/AMF/QSV resetam props se setado antes)
    let codec_api = mft.cast::<ICodecAPI>().ok();
    if let Some(ref capi) = codec_api {
        let gop = target_fps.saturating_mul(4).clamp(60, 480);
        let vbv_bytes = (bitrate_bps / 4).clamp(65_536, 2_000_000);
        unsafe {
            let _ = capi.SetValue(
                &CODECAPI_AVEncCommonRateControlMode,
                &VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
            );
            let _ = capi.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate_bps));
            let _ = capi.SetValue(&CODECAPI_AVEncCommonBufferSize, &VARIANT::from(vbv_bytes));
            let _ = capi.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true));
            let _ = capi.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &VARIANT::from(0u32));
            let _ = capi.SetValue(&CODECAPI_AVEncVideoMaxNumRefFrame, &VARIANT::from(1u32));
            let _ = capi.SetValue(&CODECAPI_AVEncMPVGOPSize, &VARIANT::from(gop));
            let _ = capi.SetValue(&CODECAPI_AVEncCommonQualityVsSpeed, &VARIANT::from(0u32));
        }
    }

    let out_info = unsafe { mft.GetOutputStreamInfo(out_id) }
        .map_err(|e| format!("GetOutputStreamInfo({}) falhou: {}", name, e))?;
    let mft_provides_samples = (out_info.dwFlags
        & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
            | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32))
        != 0;

    let scratch_out_sample = if !mft_provides_samples {
        let cap = out_info.cbSize.max(out_w * out_h).clamp(262_144, 4 * 1024 * 1024);
        let sample = unsafe { MFCreateSample() }
            .map_err(|e| format!("MFCreateSample output falhou: {}", e))?;
        let buf = unsafe { MFCreateMemoryBuffer(cap) }
            .map_err(|e| format!("MFCreateMemoryBuffer({}) falhou: {}", name, e))?;
        unsafe { sample.AddBuffer(&buf) }
            .map_err(|e| format!("AddBuffer output falhou: {}", e))?;
        Some(sample)
    } else {
        None
    };

    unsafe { mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }
        .map_err(|e| format!("BEGIN_STREAMING({}) falhou: {}", name, e))?;
    unsafe { mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) }
        .map_err(|e| format!("START_OF_STREAM({}) falhou: {}", name, e))?;

    let event_gen = if is_async {
        Some(
            mft.cast::<IMFMediaEventGenerator>()
                .map_err(|e| format!("IMFMediaEventGenerator({}) falhou: {}", name, e))?,
        )
    } else {
        None
    };

    Ok(HardwareMftEncoder {
        mft,
        in_id,
        out_id,
        is_async,
        event_gen,
        codec_api,
        mft_provides_samples,
        scratch_out_sample,
        encoder_name: name,
        configured_fps_header,
        profile_used,
        out_w,
        out_h,
        bitrate_bps,
    })
}

fn create_hardware_encoder(
    adapter_luid: LUID,
    dxgi_mgr: &IMFDXGIDeviceManager,
    out_w: u32,
    out_h: u32,
    target_fps: u32,
    bitrate_bps: u32,
) -> Result<HardwareMftEncoder, String> {
    let mut activates = enum_hardware_h264_activates(Some(adapter_luid));
    if activates.is_empty() {
        activates = enum_hardware_h264_activates(None);
    }
    if activates.is_empty() {
        return Err("Nenhum codificador H.264 de hardware Media Foundation disponivel.".to_string());
    }

    let mut last_err = String::new();
    for act in &activates {
        match try_activate_hw_encoder(act, dxgi_mgr, out_w, out_h, target_fps, bitrate_bps) {
            Ok(enc) => return Ok(enc),
            Err(e) => last_err = e,
        }
    }
    Err(format!("Nenhum encoder H.264 HW iniciou: {}", last_err))
}

fn pull_one_output(
    enc: &mut HardwareMftEncoder,
    raw_out_buf: &mut Vec<u8>,
) -> Result<Option<(bool, bool, Option<Duration>)>, String> {
    // Retorna Some((got_frame, incomplete_more, sample_pts_opt))
    let mut odb = MFT_OUTPUT_DATA_BUFFER {
        dwStreamID: enc.out_id,
        pSample: ManuallyDrop::new(if enc.mft_provides_samples {
            None
        } else {
            enc.scratch_out_sample.clone()
        }),
        dwStatus: 0,
        pEvents: ManuallyDrop::new(None),
    };
    let mut status = 0u32;
    let res = unsafe {
        enc.mft
            .ProcessOutput(0, std::slice::from_mut(&mut odb), &mut status)
    };
    let sample_out = ManuallyDrop::into_inner(odb.pSample);
    let _events_out = ManuallyDrop::into_inner(odb.pEvents);

    match res {
        Ok(()) => {
            if let Some(sample) = sample_out {
                let sample_pts = unsafe { sample.GetSampleTime() }
                    .ok()
                    .filter(|&t| t >= 0)
                    .map(|t| Duration::from_nanos((t as u64).saturating_mul(100)));
                let buf = unsafe { sample.ConvertToContiguousBuffer() }
                    .map_err(|e| format!("ConvertToContiguousBuffer falhou: {}", e))?;
                let mut ptr: *mut u8 = std::ptr::null_mut();
                let mut cur_len: u32 = 0;
                unsafe { buf.Lock(&mut ptr, None, Some(&mut cur_len)) }
                    .map_err(|e| format!("Buffer Lock falhou: {}", e))?;
                raw_out_buf.clear();
                if !ptr.is_null() && cur_len > 0 {
                    let slice = unsafe { std::slice::from_raw_parts(ptr, cur_len as usize) };
                    raw_out_buf.extend_from_slice(slice);
                }
                let _ = unsafe { buf.Unlock() };
                let incomplete =
                    (odb.dwStatus & (MFT_OUTPUT_DATA_BUFFER_INCOMPLETE.0 as u32)) != 0;
                return Ok(Some((!raw_out_buf.is_empty(), incomplete, sample_pts)));
            }
            Ok(None)
        }
        Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
        Err(e) if e.code().0 == HRESULT_MF_E_SHUTDOWN => Ok(None),
        Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
            if let Ok(avail) = unsafe { enc.mft.GetOutputAvailableType(enc.out_id, 0) } {
                let _ = unsafe { enc.mft.SetOutputType(enc.out_id, &avail, 0) };
            } else if let Ok(fallback_mt) = build_h264_output_type(
                enc.out_w,
                enc.out_h,
                enc.configured_fps_header,
                enc.bitrate_bps,
                enc.profile_used,
            ) {
                let _ = unsafe { enc.mft.SetOutputType(enc.out_id, &fallback_mt, 0) };
            }
            Ok(Some((false, true, None)))
        }
        Err(e) => Err(format!("ProcessOutput falhou: {}", e)),
    }
}

fn convert_pointer_shape_to_bgra(
    shape_type: u32,
    width: u32,
    height: u32,
    pitch: u32,
    raw: &[u8],
) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 {
        return None;
    }
    if shape_type == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR.0 as u32 {
        let w = width as usize;
        let h = height as usize;
        let p = pitch as usize;
        if raw.len() < p * h {
            return None;
        }
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let src = &raw[y * p..y * p + w * 4];
            let dst = &mut out[y * w * 4..(y + 1) * w * 4];
            dst.copy_from_slice(src);
        }
        return Some((width, height, out));
    }
    if shape_type == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR.0 as u32 {
        let w = width as usize;
        let h = height as usize;
        let p = pitch as usize;
        if raw.len() < p * h {
            return None;
        }
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let s = y * p + x * 4;
                let d = (y * w + x) * 4;
                out[d] = raw[s];
                out[d + 1] = raw[s + 1];
                out[d + 2] = raw[s + 2];
                out[d + 3] = if raw[s + 3] == 0 { 255 } else { 0 };
            }
        }
        return Some((width, height, out));
    }
    if shape_type == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME.0 as u32 && height >= 2 {
        let w = width as usize;
        let h = (height / 2) as usize;
        let p = pitch as usize;
        if raw.len() < p * (h * 2) {
            return None;
        }
        let xor_offset = h * p;
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let mask = 0x80u8 >> (x & 7);
                let byte_off = y * p + (x >> 3);
                let and_bit = (raw[byte_off] & mask) != 0;
                let xor_bit = (raw[xor_offset + byte_off] & mask) != 0;
                let d = (y * w + x) * 4;
                match (and_bit, xor_bit) {
                    (false, false) => {
                        out[d] = 0;
                        out[d + 1] = 0;
                        out[d + 2] = 0;
                        out[d + 3] = 255;
                    }
                    (false, true) => {
                        out[d] = 255;
                        out[d + 1] = 255;
                        out[d + 2] = 255;
                        out[d + 3] = 255;
                    }
                    (true, false) => {
                        out[d + 3] = 0;
                    }
                    (true, true) => {
                        out[d] = 240;
                        out[d + 1] = 240;
                        out[d + 2] = 240;
                        out[d + 3] = 200;
                    }
                }
            }
        }
        return Some((width, h as u32, out));
    }
    None
}

struct CaptureVpResources {
    src_w: u32,
    src_h: u32,
    vp_enum: ID3D11VideoProcessorEnumerator,
    vp: ID3D11VideoProcessor,
    vp_caps: D3D11_VIDEO_PROCESSOR_CAPS,
    canvas_tex: ID3D11Texture2D,
    canvas_iv: ID3D11VideoProcessorInputView,
    prev_w: u32,
    prev_h: u32,
    prev_vp: Option<ID3D11VideoProcessor>,
    prev_ov: Option<ID3D11VideoProcessorOutputView>,
    prev_tex: Option<ID3D11Texture2D>,
    prev_staging: [Option<ID3D11Texture2D>; 2],
    prev_pending: [bool; 2],
    prev_idx: usize,
}

fn build_capture_vp_resources(
    device: &ID3D11Device,
    video_device: &ID3D11VideoDevice,
    video_context: &ID3D11VideoContext,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    target_fps: u32,
) -> Result<CaptureVpResources, String> {
    let vp_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: DXGI_RATIONAL {
            Numerator: target_fps,
            Denominator: 1,
        },
        InputWidth: src_w,
        InputHeight: src_h,
        OutputFrameRate: DXGI_RATIONAL {
            Numerator: target_fps,
            Denominator: 1,
        },
        OutputWidth: out_w,
        OutputHeight: out_h,
        Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
    };
    let vp_enum: ID3D11VideoProcessorEnumerator =
        unsafe { video_device.CreateVideoProcessorEnumerator(&vp_desc) }
            .map_err(|e| format!("CreateVideoProcessorEnumerator falhou: {}", e))?;
    let vp: ID3D11VideoProcessor = unsafe { video_device.CreateVideoProcessor(&vp_enum, 0) }
        .map_err(|e| format!("CreateVideoProcessor falhou: {}", e))?;
    let mut vp_caps = D3D11_VIDEO_PROCESSOR_CAPS::default();
    let _ = unsafe { vp_enum.GetVideoProcessorCaps(&mut vp_caps) };

    let src_rect = RECT {
        left: 0,
        top: 0,
        right: src_w as i32,
        bottom: src_h as i32,
    };
    let dst_rect = RECT {
        left: 0,
        top: 0,
        right: out_w as i32,
        bottom: out_h as i32,
    };
    unsafe {
        video_context.VideoProcessorSetStreamFrameFormat(&vp, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
        video_context.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, false);
        video_context.VideoProcessorSetStreamSourceRect(&vp, 0, true, Some(&src_rect));
        video_context.VideoProcessorSetStreamDestRect(&vp, 0, true, Some(&dst_rect));
        video_context.VideoProcessorSetOutputTargetRect(&vp, true, Some(&dst_rect));
        if let Ok(vc1) = video_context.cast::<ID3D11VideoContext1>() {
            vc1.VideoProcessorSetStreamColorSpace1(&vp, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
            vc1.VideoProcessorSetOutputColorSpace1(
                &vp,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
            );
        }
    }

    let canvas_tex = create_bgra_texture(device, src_w, src_h, false)?;
    let iv_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
        FourCC: 0,
        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_VPIV {
                MipSlice: 0,
                ArraySlice: 0,
            },
        },
    };
    let mut canvas_iv_opt: Option<ID3D11VideoProcessorInputView> = None;
    unsafe {
        video_device.CreateVideoProcessorInputView(&canvas_tex, &vp_enum, &iv_desc, Some(&mut canvas_iv_opt))
    }
    .map_err(|e| format!("CreateVideoProcessorInputView(canvas) falhou: {}", e))?;
    let canvas_iv = canvas_iv_opt.ok_or_else(|| "canvas VideoProcessorInputView nulo.".to_string())?;

    // Preview nitido em double-buffer sem bloqueio (DO_NOT_WAIT)
    let (prev_w, prev_h) = compute_preview_dimensions(src_w, src_h, 1280, 720);
    let prev_vp_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: DXGI_RATIONAL {
            Numerator: 20,
            Denominator: 1,
        },
        InputWidth: src_w,
        InputHeight: src_h,
        OutputFrameRate: DXGI_RATIONAL {
            Numerator: 20,
            Denominator: 1,
        },
        OutputWidth: prev_w,
        OutputHeight: prev_h,
        Usage: D3D11_VIDEO_USAGE(2),
    };
    let prev_enum = unsafe { video_device.CreateVideoProcessorEnumerator(&prev_vp_desc) }.ok();
    let prev_vp = prev_enum
        .as_ref()
        .and_then(|pe| unsafe { video_device.CreateVideoProcessor(pe, 0) }.ok());
    let prev_tex = create_bgra_texture(device, prev_w, prev_h, false).ok();
    let prev_staging = [
        create_bgra_texture(device, prev_w, prev_h, true).ok(),
        create_bgra_texture(device, prev_w, prev_h, true).ok(),
    ];
    let prev_ov = if let (Some(pe), Some(pt), Some(pv)) = (&prev_enum, &prev_tex, &prev_vp) {
        let prev_rect = RECT {
            left: 0,
            top: 0,
            right: prev_w as i32,
            bottom: prev_h as i32,
        };
        unsafe {
            video_context.VideoProcessorSetStreamFrameFormat(pv, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            video_context.VideoProcessorSetStreamAutoProcessingMode(pv, 0, false);
            video_context.VideoProcessorSetStreamSourceRect(pv, 0, true, Some(&src_rect));
            video_context.VideoProcessorSetStreamDestRect(pv, 0, true, Some(&prev_rect));
            video_context.VideoProcessorSetOutputTargetRect(pv, true, Some(&prev_rect));
            if let Ok(vc1) = video_context.cast::<ID3D11VideoContext1>() {
                vc1.VideoProcessorSetStreamColorSpace1(pv, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
                vc1.VideoProcessorSetOutputColorSpace1(pv, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
            }
        }
        let ov_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut ov_opt: Option<ID3D11VideoProcessorOutputView> = None;
        if unsafe { video_device.CreateVideoProcessorOutputView(pt, pe, &ov_desc, Some(&mut ov_opt)) }
            .is_ok()
        {
            ov_opt
        } else {
            None
        }
    } else {
        None
    };

    Ok(CaptureVpResources {
        src_w,
        src_h,
        vp_enum,
        vp,
        vp_caps,
        canvas_tex,
        canvas_iv,
        prev_w,
        prev_h,
        prev_vp,
        prev_ov,
        prev_tex,
        prev_staging,
        prev_pending: [false, false],
        prev_idx: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SubmittedSlot {
    slot_idx: usize,
    pts: Duration,
    force_kf: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum MailboxSubmitAction {
    ProcessNow(SubmittedSlot),
    QueuedLatest { replaced_slot: Option<usize> },
    RejectedFinished(usize),
}

struct AsyncMailboxState {
    need_input: usize,
    pending: Option<SubmittedSlot>,
    in_flight: VecDeque<(usize, Duration)>,
    retire_delay: VecDeque<usize>,
    finished: bool,
    error: Option<String>,
}

impl AsyncMailboxState {
    fn new() -> Self {
        Self {
            need_input: 0,
            pending: None,
            in_flight: VecDeque::with_capacity(NV12_POOL_SIZE),
            retire_delay: VecDeque::with_capacity(2),
            finished: false,
            error: None,
        }
    }

    fn submit(&mut self, mut slot: SubmittedSlot) -> MailboxSubmitAction {
        if self.finished {
            return MailboxSubmitAction::RejectedFinished(slot.slot_idx);
        }
        if self.need_input > 0 && self.pending.is_none() {
            self.need_input -= 1;
            self.in_flight.push_back((slot.slot_idx, slot.pts));
            return MailboxSubmitAction::ProcessNow(slot);
        }
        let replaced_slot = self.pending.take().map(|old| {
            slot.force_kf |= old.force_kf;
            old.slot_idx
        });
        self.pending = Some(slot);
        MailboxSubmitAction::QueuedLatest { replaced_slot }
    }

    fn on_need_input(&mut self) -> Option<SubmittedSlot> {
        if self.finished {
            return None;
        }
        if let Some(slot) = self.pending.take() {
            self.in_flight.push_back((slot.slot_idx, slot.pts));
            Some(slot)
        } else {
            self.need_input = self.need_input.saturating_add(1);
            None
        }
    }

    fn push_in_flight(&mut self, slot: SubmittedSlot) {
        self.in_flight.push_back((slot.slot_idx, slot.pts));
    }

    fn revert_in_flight(&mut self, slot_idx: usize) {
        if let Some(pos) = self.in_flight.iter().rposition(|(idx, _)| *idx == slot_idx) {
            self.in_flight.remove(pos);
        }
    }

    fn on_output_complete(&mut self) -> (Option<Duration>, Option<usize>) {
        let Some((slot_idx, pts)) = self.in_flight.pop_front() else {
            return (None, None);
        };
        self.retire_delay.push_back(slot_idx);
        let release_slot = if self.retire_delay.len() > 1 {
            self.retire_delay.pop_front()
        } else {
            None
        };
        (Some(pts), release_slot)
    }

    fn mark_finished_take_pending(&mut self) -> Option<usize> {
        self.finished = true;
        self.pending.take().map(|s| s.slot_idx)
    }

    fn finish_drain(&mut self) -> Vec<usize> {
        self.finished = true;
        let mut to_release =
            Vec::with_capacity(1 + self.in_flight.len() + self.retire_delay.len());
        if let Some(pending) = self.pending.take() {
            to_release.push(pending.slot_idx);
        }
        while let Some((slot_idx, _)) = self.in_flight.pop_front() {
            to_release.push(slot_idx);
        }
        while let Some(slot_idx) = self.retire_delay.pop_front() {
            to_release.push(slot_idx);
        }
        to_release
    }

    fn record_error(&mut self, err: String) {
        if self.error.is_none() {
            self.error = Some(err);
        }
        self.finished = true;
    }
}

struct SyncEncoderSink {
    enc: SendCom<HardwareMftEncoder>,
    ps: H264ParameterSets,
    raw_buf: Vec<u8>,
    on_h264: Box<dyn FnMut(Bytes, bool, Duration) + Send>,
}

struct AsyncEncoderCore {
    mft: SendCom<IMFTransform>,
    in_id: u32,
    is_async: bool,
    codec_api: Option<SendCom<ICodecAPI>>,
    pool: Arc<Nv12TexturePool>,
    frame_dur_100ns: i64,
    input_serialize: Mutex<()>,
    state: Mutex<AsyncMailboxState>,
    sync_sink: Mutex<Option<SyncEncoderSink>>,
}

fn emit_normalized_h264(
    raw_buf: &[u8],
    ps: &mut H264ParameterSets,
    pts: Duration,
    on_h264: &mut dyn FnMut(Bytes, bool, Duration),
) {
    if let Some((au, is_kf)) = ps.normalize_access_unit_no_b_slices(raw_buf) {
        on_h264(au, is_kf, pts);
    }
}

impl AsyncEncoderCore {
    fn new(
        mft: IMFTransform,
        in_id: u32,
        is_async: bool,
        codec_api: Option<ICodecAPI>,
        pool: Arc<Nv12TexturePool>,
        frame_dur_100ns: i64,
        sync_sink: Option<SyncEncoderSink>,
    ) -> Self {
        Self {
            mft: SendCom(mft),
            in_id,
            is_async,
            codec_api: codec_api.map(SendCom),
            pool,
            frame_dur_100ns,
            input_serialize: Mutex::new(()),
            state: Mutex::new(AsyncMailboxState::new()),
            sync_sink: Mutex::new(sync_sink),
        }
    }

    #[inline]
    fn is_finished(&self) -> bool {
        self.state.lock().map(|s| s.finished).unwrap_or(true)
    }

    fn record_error(&self, err: String) {
        if let Ok(mut s) = self.state.lock() {
            s.record_error(err);
        }
    }

    fn take_error(&self) -> Option<String> {
        self.state.lock().ok().and_then(|mut s| s.error.take())
    }

    fn send_slot_to_mft_locked(&self, slot: SubmittedSlot) -> Result<(), String> {
        if slot.force_kf {
            if let Some(ref capi) = self.codec_api {
                unsafe {
                    let _ = capi.0.SetValue(
                        &CODECAPI_AVEncVideoForceKeyFrame,
                        &VARIANT::from(1u32),
                    );
                }
            }
        }
        let pts_100ns = (slot.pts.as_nanos() / 100) as i64;
        let sample = match self
            .pool
            .create_sample_for_slot(slot.slot_idx, pts_100ns, self.frame_dur_100ns)
        {
            Ok(s) => s,
            Err(e) => {
                if let Ok(mut st) = self.state.lock() {
                    st.revert_in_flight(slot.slot_idx);
                    st.record_error(e.clone());
                }
                self.pool.release(slot.slot_idx);
                return Err(e);
            }
        };

        if let Err(e) = unsafe { self.mft.0.ProcessInput(self.in_id, &sample, 0) } {
            let code = e.code().0;
            if let Ok(mut st) = self.state.lock() {
                st.revert_in_flight(slot.slot_idx);
                if code == HRESULT_MF_E_SHUTDOWN {
                    st.finished = true;
                } else if code != HRESULT_MF_E_NOTACCEPTING {
                    st.record_error(format!("ProcessInput falhou: {}", e));
                }
            }
            self.pool.release(slot.slot_idx);
            if code == HRESULT_MF_E_SHUTDOWN || code == HRESULT_MF_E_NOTACCEPTING {
                return Ok(());
            }
            return Err(format!("ProcessInput falhou: {}", e));
        }
        Ok(())
    }

    fn submit(&self, slot: SubmittedSlot) {
        let Ok(_serial) = self.input_serialize.lock() else {
            self.pool.release(slot.slot_idx);
            return;
        };

        if self.is_async {
            let action = {
                let Ok(mut st) = self.state.lock() else {
                    self.pool.release(slot.slot_idx);
                    return;
                };
                st.submit(slot)
            };
            match action {
                MailboxSubmitAction::ProcessNow(s) => {
                    let _ = self.send_slot_to_mft_locked(s);
                }
                MailboxSubmitAction::QueuedLatest { replaced_slot } => {
                    if let Some(old_idx) = replaced_slot {
                        self.pool.release(old_idx);
                    }
                }
                MailboxSubmitAction::RejectedFinished(idx) => {
                    self.pool.release(idx);
                }
            }
            return;
        }

        // MFT sincrono raro
        {
            let Ok(mut st) = self.state.lock() else {
                self.pool.release(slot.slot_idx);
                return;
            };
            if st.finished {
                self.pool.release(slot.slot_idx);
                return;
            }
            st.push_in_flight(slot);
        }
        if self.send_slot_to_mft_locked(slot).is_err() {
            return;
        }
        if let Ok(mut sink_opt) = self.sync_sink.lock() {
            if let Some(ref mut sink) = *sink_opt {
                loop {
                    match pull_one_output(&mut sink.enc.0, &mut sink.raw_buf) {
                        Ok(Some((got, incomplete, sample_pts_opt))) => {
                            if got {
                                let (slot_pts_opt, release_idx) = self.on_output_complete();
                                if let Some(idx) = release_idx {
                                    self.pool.release(idx);
                                }
                                let pts = sample_pts_opt.or(slot_pts_opt).unwrap_or(slot.pts);
                                emit_normalized_h264(
                                    &sink.raw_buf,
                                    &mut sink.ps,
                                    pts,
                                    &mut *sink.on_h264,
                                );
                            }
                            if !incomplete {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(err) => {
                            self.record_error(err);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn on_need_input(&self) {
        let Ok(_serial) = self.input_serialize.lock() else {
            return;
        };
        let slot_opt = {
            let Ok(mut st) = self.state.lock() else {
                return;
            };
            st.on_need_input()
        };
        if let Some(s) = slot_opt {
            let _ = self.send_slot_to_mft_locked(s);
        }
    }

    fn on_output_complete(&self) -> (Option<Duration>, Option<usize>) {
        if let Ok(mut st) = self.state.lock() {
            st.on_output_complete()
        } else {
            (None, None)
        }
    }

    fn mark_finished(&self) {
        if let Ok(mut st) = self.state.lock() {
            if let Some(idx) = st.mark_finished_take_pending() {
                self.pool.release(idx);
            }
        }
    }

    fn send_eos_drain_sync(&self) {
        if let Ok(_serial) = self.input_serialize.lock() {
            unsafe {
                let _ = self.mft.0.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
                let _ = self.mft.0.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
            }
        }
    }

    fn send_flush_and_eos(&self) {
        if let Ok(_serial) = self.input_serialize.lock() {
            unsafe {
                let _ = self.mft.0.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                let _ = self.mft.0.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            }
        }
    }

    fn finish_drain(&self) {
        let slots = if let Ok(mut st) = self.state.lock() {
            st.finish_drain()
        } else {
            Vec::new()
        };
        for idx in slots {
            self.pool.release(idx);
        }
    }
}

struct CursorOverlayState {
    supported: bool,
    iv: Option<ID3D11VideoProcessorInputView>,
    _tex: Option<ID3D11Texture2D>,
    dims: (u32, u32),
    hotspot: (i32, i32),
    pos: (i32, i32),
    visible: bool,
    raw_buf: Vec<u8>,
}

impl CursorOverlayState {
    fn new(supported: bool) -> Self {
        Self {
            supported,
            iv: None,
            _tex: None,
            dims: (0, 0),
            hotspot: (0, 0),
            pos: (0, 0),
            visible: false,
            raw_buf: Vec::with_capacity(16 * 1024),
        }
    }

    fn reset(&mut self, supported: bool) {
        self.supported = supported;
        self.iv = None;
        self._tex = None;
        self.dims = (0, 0);
    }

    fn update_from_dupl(
        &mut self,
        dupl: &IDXGIOutputDuplication,
        fi: &DXGI_OUTDUPL_FRAME_INFO,
        device: &ID3D11Device,
        video_device: &ID3D11VideoDevice,
        vp_enum: &ID3D11VideoProcessorEnumerator,
    ) {
        self.visible = fi.PointerPosition.Visible.as_bool();
        self.pos = (fi.PointerPosition.Position.x, fi.PointerPosition.Position.y);
        if !self.supported || fi.PointerShapeBufferSize == 0 {
            return;
        }
        self.raw_buf.resize(fi.PointerShapeBufferSize as usize, 0);
        let mut req = 0u32;
        let mut sinfo = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
        if unsafe {
            dupl.GetFramePointerShape(
                fi.PointerShapeBufferSize,
                self.raw_buf.as_mut_ptr() as *mut _,
                &mut req,
                &mut sinfo,
            )
        }
        .is_err()
        {
            return;
        }
        self.hotspot = (sinfo.HotSpot.x, sinfo.HotSpot.y);
        let used = (req as usize).min(self.raw_buf.len());
        let Some((cw, ch, bgra)) = convert_pointer_shape_to_bgra(
            sinfo.Type,
            sinfo.Width,
            sinfo.Height,
            sinfo.Pitch,
            &self.raw_buf[..used],
        ) else {
            return;
        };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: cw,
            Height: ch,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let init_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: bgra.as_ptr() as *const _,
            SysMemPitch: cw * 4,
            SysMemSlicePitch: 0,
        };
        let mut new_ctex: Option<ID3D11Texture2D> = None;
        if unsafe { device.CreateTexture2D(&desc, Some(&init_data), Some(&mut new_ctex)) }.is_err() {
            return;
        }
        let Some(ct) = new_ctex else {
            return;
        };
        let iv_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut new_civ: Option<ID3D11VideoProcessorInputView> = None;
        if unsafe {
            video_device.CreateVideoProcessorInputView(&ct, vp_enum, &iv_desc, Some(&mut new_civ))
        }
        .is_ok()
        {
            self._tex = Some(ct);
            self.iv = new_civ;
            self.dims = (cw, ch);
        }
    }
}

struct GpuRenderState {
    vp_res: CaptureVpResources,
    cursor: CursorOverlayState,
    wgc_active: bool,
    have_frame: bool,
    pending_submit: bool,
    last_submit_at: Instant,
    frame_idx: u64,
    last_preview_at: Instant,
    prev_rgba_scratch: Vec<u8>,
}

struct CaptureShared {
    gpu_state: Mutex<GpuRenderState>,
    device: SendCom<ID3D11Device>,
    context: SendCom<ID3D11DeviceContext>,
    video_device: SendCom<ID3D11VideoDevice>,
    video_context: SendCom<ID3D11VideoContext>,
    core: Arc<AsyncEncoderCore>,
    pool: Arc<Nv12TexturePool>,
    kf_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    preview_enabled: Arc<AtomicBool>,
    out_w: u32,
    out_h: u32,
    target_fps: u32,
    frame_interval: Duration,
    start_clock: Instant,
}

unsafe impl Send for CaptureShared {}
unsafe impl Sync for CaptureShared {}

#[inline]
fn is_due_with_slack(elapsed: Duration, frame_interval: Duration, force_kf: bool) -> bool {
    if force_kf {
        return true;
    }
    let slack_interval = (frame_interval.saturating_mul(3)) / 4;
    elapsed >= slack_interval
}

fn render_canvas_to_nv12_slot(
    st: &mut GpuRenderState,
    video_context: &ID3D11VideoContext,
    slot_ov: &ID3D11VideoProcessorOutputView,
    out_w: u32,
    out_h: u32,
) -> Result<(), String> {
    let mut streams = [
        D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(st.vp_res.canvas_iv.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        },
        D3D11_VIDEO_PROCESSOR_STREAM::default(),
    ];

    let mut used_cursor_stream = false;
    if !st.wgc_active && st.cursor.supported && st.cursor.visible {
        if let Some(ref civ) = st.cursor.iv {
            let (cw, ch) = st.cursor.dims;
            let cx_src = st.cursor.pos.0 - st.cursor.hotspot.0;
            let cy_src = st.cursor.pos.1 - st.cursor.hotspot.1;
            let dx0 = ((cx_src as f64) * (out_w as f64) / (st.vp_res.src_w as f64)).round() as i32;
            let dy0 = ((cy_src as f64) * (out_h as f64) / (st.vp_res.src_h as f64)).round() as i32;
            let dx1 =
                (((cx_src + cw as i32) as f64) * (out_w as f64) / (st.vp_res.src_w as f64)).round() as i32;
            let dy1 =
                (((cy_src + ch as i32) as f64) * (out_h as f64) / (st.vp_res.src_h as f64)).round() as i32;

            let cdst = RECT {
                left: dx0.clamp(0, out_w as i32),
                top: dy0.clamp(0, out_h as i32),
                right: dx1.clamp(0, out_w as i32),
                bottom: dy1.clamp(0, out_h as i32),
            };
            if cdst.right > cdst.left && cdst.bottom > cdst.top {
                let csrc = RECT {
                    left: 0,
                    top: 0,
                    right: cw as i32,
                    bottom: ch as i32,
                };
                unsafe {
                    video_context.VideoProcessorSetStreamFrameFormat(
                        &st.vp_res.vp,
                        1,
                        D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                    );
                    video_context.VideoProcessorSetStreamSourceRect(&st.vp_res.vp, 1, true, Some(&csrc));
                    video_context.VideoProcessorSetStreamDestRect(&st.vp_res.vp, 1, true, Some(&cdst));
                }
                streams[1] = D3D11_VIDEO_PROCESSOR_STREAM {
                    Enable: true.into(),
                    OutputIndex: 0,
                    InputFrameOrField: 0,
                    PastFrames: 0,
                    FutureFrames: 0,
                    ppPastSurfaces: std::ptr::null_mut(),
                    pInputSurface: ManuallyDrop::new(Some(civ.clone())),
                    ppFutureSurfaces: std::ptr::null_mut(),
                    ppPastSurfacesRight: std::ptr::null_mut(),
                    pInputSurfaceRight: ManuallyDrop::new(None),
                    ppFutureSurfacesRight: std::ptr::null_mut(),
                };
                used_cursor_stream = true;
            }
        }
    }

    let blt_slice = if used_cursor_stream {
        &streams[..2]
    } else {
        &streams[..1]
    };
    let blt_res = unsafe { video_context.VideoProcessorBlt(&st.vp_res.vp, slot_ov, 0, blt_slice) };
    unsafe {
        ManuallyDrop::drop(&mut streams[0].pInputSurface);
        if used_cursor_stream {
            ManuallyDrop::drop(&mut streams[1].pInputSurface);
        }
    }
    if blt_res.is_err() && used_cursor_stream {
        st.cursor.supported = false;
        let mut fallback_s = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(st.vp_res.canvas_iv.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        let res0 = unsafe {
            video_context.VideoProcessorBlt(
                &st.vp_res.vp,
                slot_ov,
                0,
                std::slice::from_ref(&fallback_s),
            )
        };
        unsafe { ManuallyDrop::drop(&mut fallback_s.pInputSurface) };
        res0.map_err(|e| format!("VideoProcessorBlt falhou: {}", e))?;
    } else if let Err(e) = blt_res {
        return Err(format!("VideoProcessorBlt falhou: {}", e));
    }
    Ok(())
}

fn pump_preview_locked(
    st: &mut GpuRenderState,
    context: &ID3D11DeviceContext,
    video_context: &ID3D11VideoContext,
    on_preview: &mut dyn FnMut(usize, usize, Vec<u8>),
) {
    let read_idx = st.vp_res.prev_idx ^ 1;
    if st.vp_res.prev_pending[read_idx] {
        if let Some(ref stage_read) = st.vp_res.prev_staging[read_idx] {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if unsafe {
                context.Map(
                    stage_read,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
            }
            .is_ok()
            {
                let pw = st.vp_res.prev_w as usize;
                let ph = st.vp_res.prev_h as usize;
                let needed = pw * ph * 4;
                if st.prev_rgba_scratch.len() != needed {
                    st.prev_rgba_scratch.resize(needed, 255);
                }
                let src_bytes = mapped.pData as *const u8;
                let row_pitch = mapped.RowPitch as usize;
                if !src_bytes.is_null() {
                    for y in 0..ph {
                        let srow = unsafe {
                            std::slice::from_raw_parts(src_bytes.add(y * row_pitch), pw * 4)
                        };
                        let drow = &mut st.prev_rgba_scratch[y * pw * 4..(y + 1) * pw * 4];
                        for x in 0..pw {
                            let i = x * 4;
                            drow[i] = srow[i + 2];
                            drow[i + 1] = srow[i + 1];
                            drow[i + 2] = srow[i];
                            drow[i + 3] = 255;
                        }
                    }
                    on_preview(pw, ph, st.prev_rgba_scratch.clone());
                }
                unsafe { context.Unmap(stage_read, 0) };
                st.vp_res.prev_pending[read_idx] = false;
            }
        } else {
            st.vp_res.prev_pending[read_idx] = false;
        }
    }

    let write_idx = st.vp_res.prev_idx;
    if !st.vp_res.prev_pending[write_idx]
        && st.last_preview_at.elapsed() >= Duration::from_millis(PREVIEW_INTERVAL_MS)
    {
        if let (Some(pvp), Some(pov), Some(ptex), Some(stage_write)) = (
            &st.vp_res.prev_vp,
            &st.vp_res.prev_ov,
            &st.vp_res.prev_tex,
            &st.vp_res.prev_staging[write_idx],
        ) {
            let mut ps_stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                OutputIndex: 0,
                InputFrameOrField: 0,
                PastFrames: 0,
                FutureFrames: 0,
                ppPastSurfaces: std::ptr::null_mut(),
                pInputSurface: ManuallyDrop::new(Some(st.vp_res.canvas_iv.clone())),
                ppFutureSurfaces: std::ptr::null_mut(),
                ppPastSurfacesRight: std::ptr::null_mut(),
                pInputSurfaceRight: ManuallyDrop::new(None),
                ppFutureSurfacesRight: std::ptr::null_mut(),
            };
            let blt_ok = unsafe {
                video_context.VideoProcessorBlt(pvp, pov, 0, std::slice::from_ref(&ps_stream))
            }
            .is_ok();
            unsafe { ManuallyDrop::drop(&mut ps_stream.pInputSurface) };
            if blt_ok {
                unsafe { context.CopyResource(stage_write, ptex) };
                st.vp_res.prev_pending[write_idx] = true;
                st.vp_res.prev_idx ^= 1;
                st.last_preview_at = Instant::now();
            }
        }
    }
}

impl CaptureShared {
    fn try_prepare_submit_locked(
        &self,
        st: &mut GpuRenderState,
        on_preview: Option<&mut dyn FnMut(usize, usize, Vec<u8>)>,
        allow_heartbeat: bool,
    ) -> Result<Option<SubmittedSlot>, String> {
        if !st.have_frame {
            return Ok(None);
        }
        let now = Instant::now();
        let force_kf_flag = self.kf_flag.load(Ordering::Acquire);
        let idle = now.saturating_duration_since(st.last_submit_at);
        let heartbeat_due = allow_heartbeat && idle >= Duration::from_millis(STATIC_HEARTBEAT_MS);
        let due_new = st.pending_submit && is_due_with_slack(idle, self.frame_interval, force_kf_flag);
        if !due_new && !heartbeat_due && !(force_kf_flag && st.frame_idx > 0) {
            return Ok(None);
        }

        let Some((slot_idx, slot_ov)) = self.pool.try_acquire() else {
            return Ok(None);
        };

        if let Err(err) =
            render_canvas_to_nv12_slot(st, &self.video_context.0, &slot_ov, self.out_w, self.out_h)
        {
            self.pool.release(slot_idx);
            return Err(err);
        }
        unsafe { self.context.0.Flush() };

        let demanded_kf = self.kf_flag.swap(false, Ordering::AcqRel) || st.frame_idx == 0;
        let pts = now.duration_since(self.start_clock);

        st.pending_submit = false;
        st.last_submit_at = now;
        st.frame_idx = st.frame_idx.wrapping_add(1);

        if let Some(preview_cb) = on_preview {
            if self.preview_enabled.load(Ordering::Relaxed) {
                pump_preview_locked(st, &self.context.0, &self.video_context.0, preview_cb);
            } else if st.prev_rgba_scratch.capacity() > 0 {
                st.prev_rgba_scratch = Vec::new();
            }
        } else if st.prev_rgba_scratch.capacity() > 0 {
            st.prev_rgba_scratch = Vec::new();
        }
        Ok(Some(SubmittedSlot {
            slot_idx,
            pts,
            force_kf: demanded_kf,
        }))
    }

    fn rebuild_capture_dimensions(
        &self,
        st: &mut GpuRenderState,
        new_w: u32,
        new_h: u32,
    ) -> Result<(), String> {
        if new_w == st.vp_res.src_w && new_h == st.vp_res.src_h {
            return Ok(());
        }
        let rebuilt = build_capture_vp_resources(
            &self.device.0,
            &self.video_device.0,
            &self.video_context.0,
            new_w,
            new_h,
            self.out_w,
            self.out_h,
            self.target_fps,
        )?;
        self.pool
            .rebuild_output_views(&self.video_device.0, &rebuilt.vp_enum)?;
        let cursor_sup = rebuilt.vp_caps.MaxInputStreams >= 2;
        st.vp_res = rebuilt;
        st.cursor.reset(cursor_sup);
        st.have_frame = false;
        st.pending_submit = false;
        Ok(())
    }

    fn acquire_timeout_ms(&self, max_wait_ms: u32) -> u32 {
        let Ok(st) = self.gpu_state.lock() else {
            return max_wait_ms;
        };
        if !st.have_frame {
            return 16.min(max_wait_ms);
        }
        if st.pending_submit {
            let rem = self
                .frame_interval
                .saturating_sub(st.last_submit_at.elapsed())
                .as_millis() as u32;
            return rem.clamp(1, max_wait_ms);
        }
        max_wait_ms
    }
}

enum DuplExitReason {
    Stop,
    SwitchToWgc(String),
}

fn run_duplication_capture(
    mut dupl: IDXGIOutputDuplication,
    output: &IDXGIOutput,
    shared: &Arc<CaptureShared>,
    on_preview: &mut dyn FnMut(usize, usize, Vec<u8>),
) -> Result<DuplExitReason, String> {
    let max_wait_ms = (1_000 / shared.target_fps.max(5)).clamp(1, 33);
    let mut access_lost_started: Option<Instant> = None;

    while !shared.stop_flag.load(Ordering::Acquire) && !shared.core.is_finished() {
        let mut wait_ms = shared.acquire_timeout_ms(max_wait_ms);

        // No maximo 2 aquisicoes (1 bloqueante + 1 nao-bloqueante apenas se havia frames acumulados)
        // para obter o quadro mais novo sem copiar varias superficies VRAM seguidas.
        for drain_iter in 0..2 {
            let mut fi = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res_opt = None;
            let acq = unsafe { dupl.AcquireNextFrame(wait_ms, &mut fi, &mut res_opt) };
            match acq {
                Ok(()) => {
                    access_lost_started = None;
                    let has_more_accumulated = fi.AccumulatedFrames > 1;
                    if let Ok(mut st) = shared.gpu_state.lock() {
                        if fi.LastMouseUpdateTime != 0 {
                            let vp_enum = st.vp_res.vp_enum.clone();
                            st.cursor.update_from_dupl(
                                &dupl,
                                &fi,
                                &shared.device.0,
                                &shared.video_device.0,
                                &vp_enum,
                            );
                            st.pending_submit = true;
                        }
                        if fi.LastPresentTime != 0 || fi.AccumulatedFrames > 0 || !st.have_frame {
                            if let Some(res) = res_opt {
                                if let Ok(acquired_tex) = res.cast::<ID3D11Texture2D>() {
                                    unsafe {
                                        shared
                                            .context
                                            .0
                                            .CopyResource(&st.vp_res.canvas_tex, &acquired_tex);
                                    }
                                    st.have_frame = true;
                                    st.pending_submit = true;
                                }
                            }
                        }
                    }
                    let _ = unsafe { dupl.ReleaseFrame() };
                    wait_ms = 0;
                    if drain_iter >= 1 || !has_more_accumulated {
                        break;
                    }
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => break,
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    let started = *access_lost_started.get_or_insert_with(Instant::now);
                    if started.elapsed() >= Duration::from_millis(DUPL_LOST_TO_WGC_MS) {
                        return Ok(DuplExitReason::SwitchToWgc(
                            "DXGI_ERROR_ACCESS_LOST persistente; ativando Windows Graphics Capture"
                                .to_string(),
                        ));
                    }
                    if started.elapsed() >= Duration::from_millis(ACCESS_LOST_TIMEOUT_MS)
                        || shared.stop_flag.load(Ordering::Acquire)
                    {
                        return Err("DXGI_ERROR_ACCESS_LOST persistente".to_string());
                    }
                    std::thread::sleep(Duration::from_millis(30));
                    if let Ok(new_dupl) = create_duplication(output, &shared.device.0) {
                        dupl = new_dupl;
                        let mut new_desc = Default::default();
                        unsafe { dupl.GetDesc(&mut new_desc) };
                        let new_w = new_desc.ModeDesc.Width.max(16);
                        let new_h = new_desc.ModeDesc.Height.max(16);
                        if let Ok(mut st) = shared.gpu_state.lock() {
                            shared.rebuild_capture_dimensions(&mut st, new_w, new_h)?;
                        }
                    }
                    break;
                }
                Err(e) => {
                    return Ok(DuplExitReason::SwitchToWgc(format!(
                        "AcquireNextFrame falhou ({}); ativando Windows Graphics Capture",
                        e
                    )));
                }
            }
        }

        let submit_opt = if let Ok(mut st) = shared.gpu_state.lock() {
            shared.try_prepare_submit_locked(&mut st, Some(on_preview), true)?
        } else {
            None
        };
        if let Some(slot) = submit_opt {
            shared.core.submit(slot);
        }
    }

    Ok(DuplExitReason::Stop)
}

#[derive(Clone, Copy)]
enum WgcCaptureTarget {
    Monitor(HMONITOR),
    Window(HWND),
}

#[inline]
fn wgc_should_recreate_pool(pool_size: SizeInt32, content_size: SizeInt32) -> Option<SizeInt32> {
    if content_size.Width < 16 || content_size.Height < 16 {
        return None;
    }
    if content_size.Width != pool_size.Width || content_size.Height != pool_size.Height {
        Some(content_size)
    } else {
        None
    }
}

struct WgcCaptureSession {
    winrt_device: IDirect3DDevice,
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: EventRegistrationToken,
    arrived: Arc<(Mutex<bool>, Condvar)>,
    pool_size: SizeInt32,
}

impl WgcCaptureSession {
    fn open(
        device: &ID3D11Device,
        target: WgcCaptureTarget,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let supported = GraphicsCaptureSession::IsSupported().unwrap_or(false);
        if !supported {
            return Err("Windows Graphics Capture indisponivel neste sistema.".to_string());
        }
        let dxgi_device: IDXGIDevice = device
            .cast()
            .map_err(|e| format!("Cast IDXGIDevice falhou para WGC: {}", e))?;
        let winrt_insp = unsafe { dyn_create_d3d11_device_from_dxgi_device(&dxgi_device) }?;
        let winrt_device: IDirect3DDevice = winrt_insp
            .cast()
            .map_err(|e| format!("Cast IDirect3DDevice falhou para WGC: {}", e))?;

        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(|e| format!("IGraphicsCaptureItemInterop falhou: {}", e))?;
        let item: GraphicsCaptureItem = match target {
            WgcCaptureTarget::Monitor(hmonitor) => unsafe {
                interop.CreateForMonitor::<HMONITOR, GraphicsCaptureItem>(hmonitor)
            }
            .map_err(|e| format!("WGC CreateForMonitor falhou: {}", e))?,
            WgcCaptureTarget::Window(hwnd) => unsafe {
                interop.CreateForWindow::<HWND, GraphicsCaptureItem>(hwnd)
            }
            .map_err(|e| format!("WGC CreateForWindow falhou: {}", e))?,
        };

        let item_size = item.Size().ok().filter(|s| s.Width >= 16 && s.Height >= 16);
        let init_size = item_size.unwrap_or(SizeInt32 {
            Width: width.max(16) as i32,
            Height: height.max(16) as i32,
        });
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            init_size,
        )
        .map_err(|e| format!("WGC Direct3D11CaptureFramePool falhou: {}", e))?;

        let arrived = Arc::new((Mutex::new(false), Condvar::new()));
        let arrived_cb = Arc::clone(&arrived);
        let token = frame_pool
            .FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                move |_, _| {
                    let (lock, cv) = &*arrived_cb;
                    if let Ok(mut ready) = lock.lock() {
                        *ready = true;
                        cv.notify_one();
                    }
                    Ok(())
                },
            ))
            .map_err(|e| format!("WGC FrameArrived falhou: {}", e))?;

        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(|e| format!("WGC CreateCaptureSession falhou: {}", e))?;
        let _ = session.SetIsCursorCaptureEnabled(true);
        let _ = session.SetIsBorderRequired(false);
        session
            .StartCapture()
            .map_err(|e| format!("WGC StartCapture falhou: {}", e))?;

        Ok(Self {
            winrt_device,
            frame_pool,
            session,
            token,
            arrived,
            pool_size: init_size,
        })
    }
}

impl Drop for WgcCaptureSession {
    fn drop(&mut self) {
        let _ = self.frame_pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.frame_pool.Close();
    }
}

fn drain_latest_wgc_frame(
    wgc: &mut WgcCaptureSession,
    shared: &Arc<CaptureShared>,
) -> Result<(), String> {
    let mut latest: Option<Direct3D11CaptureFrame> = None;
    while let Ok(frame) = wgc.frame_pool.TryGetNextFrame() {
        if let Some(prev) = latest.replace(frame) {
            let _ = prev.Close();
        }
    }
    let Some(frame) = latest else {
        return Ok(());
    };

    let mut recreate_size: Option<SizeInt32> = None;
    let pool_size = wgc.pool_size;
    let res = (|| -> Result<(), String> {
        let csize = frame
            .ContentSize()
            .map_err(|e| format!("WGC ContentSize falhou: {}", e))?;
        let cw = csize.Width.max(0) as u32;
        let ch = csize.Height.max(0) as u32;
        if cw < 16 || ch < 16 {
            return Ok(());
        }
        recreate_size = wgc_should_recreate_pool(pool_size, csize);

        let surface = frame
            .Surface()
            .map_err(|e| format!("WGC Surface falhou: {}", e))?;
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| format!("WGC IDirect3DDxgiInterfaceAccess falhou: {}", e))?;
        let src_tex: ID3D11Texture2D = unsafe { access.GetInterface() }
            .map_err(|e| format!("WGC GetInterface<ID3D11Texture2D> falhou: {}", e))?;

        let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { src_tex.GetDesc(&mut tex_desc) };
        let copy_w = cw.min(tex_desc.Width);
        let copy_h = ch.min(tex_desc.Height);
        if copy_w < 16 || copy_h < 16 {
            return Ok(());
        }

        if let Ok(mut st) = shared.gpu_state.lock() {
            if copy_w != st.vp_res.src_w || copy_h != st.vp_res.src_h {
                shared.rebuild_capture_dimensions(&mut st, copy_w, copy_h)?;
            }
            let safe_copy_w = copy_w.min(st.vp_res.src_w);
            let safe_copy_h = copy_h.min(st.vp_res.src_h);
            if safe_copy_w < 16 || safe_copy_h < 16 {
                return Ok(());
            }
            let copy_box = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: safe_copy_w,
                bottom: safe_copy_h,
                back: 1,
            };
            unsafe {
                shared.context.0.CopySubresourceRegion(
                    &st.vp_res.canvas_tex,
                    0,
                    0,
                    0,
                    0,
                    &src_tex,
                    0,
                    Some(&copy_box),
                );
            }
            st.have_frame = true;
            st.pending_submit = true;
        }
        Ok(())
    })();

    // Fecha frame/surface D3D11 ANTES de chamar frame_pool.Recreate para evitar deadlock no WGC.
    let _ = frame.Close();
    if res.is_ok() {
        if let Some(new_sz) = recreate_size {
            if wgc
                .frame_pool
                .Recreate(
                    &wgc.winrt_device,
                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    3,
                    new_sz,
                )
                .is_ok()
            {
                wgc.pool_size = new_sz;
            }
        }
    }
    res
}

fn run_wgc_capture(
    target: WgcCaptureTarget,
    shared: &Arc<CaptureShared>,
    on_preview: &mut dyn FnMut(usize, usize, Vec<u8>),
) -> Result<(), String> {
    let (init_w, init_h) = shared
        .gpu_state
        .lock()
        .map(|mut st| {
            st.wgc_active = true;
            (st.vp_res.src_w, st.vp_res.src_h)
        })
        .unwrap_or((1920, 1080));

    let mut wgc = WgcCaptureSession::open(&shared.device.0, target, init_w, init_h)?;
    let max_wait = Duration::from_millis((1_000 / shared.target_fps.max(5)).clamp(1, 33) as u64);
    let arrived = Arc::clone(&wgc.arrived);
    let (lock, cv) = &*arrived;

    while !shared.stop_flag.load(Ordering::Acquire) && !shared.core.is_finished() {
        if let WgcCaptureTarget::Window(hwnd) = target {
            if unsafe { !IsWindow(hwnd).as_bool() } {
                crate::diag_info!("gpu-capture", "janela WGC fechada externamente (hwnd={:?})", hwnd);
                break;
            }
        }
        if let Ok(mut ready) = lock.lock() {
            *ready = false;
        }
        drain_latest_wgc_frame(&mut wgc, shared)?;

        let submit_opt = if let Ok(mut st) = shared.gpu_state.lock() {
            shared.try_prepare_submit_locked(&mut st, Some(on_preview), true)?
        } else {
            None
        };
        let submitted = submit_opt.is_some();
        if let Some(slot) = submit_opt {
            shared.core.submit(slot);
        }

        let wait_dur = if submitted {
            max_wait
        } else {
            Duration::from_millis(shared.acquire_timeout_ms(max_wait.as_millis() as u32) as u64)
        };
        if let Ok(ready) = lock.lock() {
            if !*ready {
                let (_guard, _) = cv
                    .wait_timeout(ready, wait_dur)
                    .map_err(|_| "WGC condvar poisoned".to_string())?;
            }
        }
    }
    Ok(())
}

/// Executa o pipeline zero-copy GPU DXGI/WGC -> D3D11 VideoProcessor -> Media Foundation H.264 HW
/// sem perda de wakeup (mailbox 1-slot serializado) e com prioridade de GPU para jogos.
/// Retorna `Err(...)` imediatamente caso nao haja suporte de hardware (para cair no fallback de software).
pub fn try_run_gpu_screen_pipeline(
    cfg: GpuCaptureConfig,
    stop_flag: Arc<AtomicBool>,
    kf_flag: Arc<AtomicBool>,
    on_ready: impl FnOnce(GpuEncoderInfo),
    on_h264: impl FnMut(Bytes, bool, Duration) + Send + 'static,
    mut on_preview: impl FnMut(usize, usize, Vec<u8>),
) -> Result<(), String> {
    let _com_mf_guard = ComMfGuard::init()?;

    let target_fps = cfg.target_fps.clamp(5, 240);
    let out_w = (cfg.out_width & !1).clamp(16, 3840);
    let out_h = (cfg.out_height & !1).clamp(16, 2160);
    let bitrate_bps = cfg.bitrate_bps.clamp(150_000, 32_000_000);

    let (adapter1, adapter_luid, monitor_output_opt, wgc_target) = match &cfg.source {
        GpuCaptureSource::Monitor(mon) => {
            let (adapter1, output, adapter_luid) = find_dxgi_adapter_and_output(mon)?;
            let mut odesc = DXGI_OUTPUT_DESC::default();
            unsafe { output.GetDesc(&mut odesc) }
                .map_err(|e| format!("IDXGIOutput::GetDesc falhou: {}", e))?;
            let coord_w = (odesc.DesktopCoordinates.right - odesc.DesktopCoordinates.left).max(16) as u32;
            let coord_h = (odesc.DesktopCoordinates.bottom - odesc.DesktopCoordinates.top).max(16) as u32;
            let rotation_supported = odesc.Rotation == DXGI_MODE_ROTATION_IDENTITY
                || odesc.Rotation == DXGI_MODE_ROTATION_UNSPECIFIED;
            let hmonitor = odesc.Monitor;
            (
                adapter1,
                adapter_luid,
                Some((output, coord_w, coord_h, rotation_supported)),
                WgcCaptureTarget::Monitor(hmonitor),
            )
        }
        GpuCaptureSource::Window(win) => {
            if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
                return Err("Windows Graphics Capture indisponivel para captura de janela.".to_string());
            }
            let hwnd = HWND(win.hwnd);
            if unsafe { !IsWindow(hwnd).as_bool() } {
                return Err("Janela selecionada nao esta mais aberta.".to_string());
            }
            let hmonitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) };
            let (adapter1, adapter_luid) = find_dxgi_adapter_for_hmonitor(hmonitor)?;
            (adapter1, adapter_luid, None, WgcCaptureTarget::Window(hwnd))
        }
    };

    let dxgi_adapter: IDXGIAdapter = adapter1
        .cast()
        .map_err(|e| format!("Cast IDXGIAdapter falhou: {}", e))?;

    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut device_opt: Option<ID3D11Device> = None;
    let mut context_opt: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            &dxgi_adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device_opt),
            None,
            Some(&mut context_opt),
        )
    }
    .map_err(|e| format!("D3D11CreateDevice falhou: {}", e))?;

    let device = device_opt.ok_or_else(|| "D3D11Device nulo.".to_string())?;
    let context = context_opt.ok_or_else(|| "D3D11DeviceContext nulo.".to_string())?;
    if let Ok(mt) = device.cast::<ID3D11Multithread>() {
        unsafe {
            let _ = mt.SetMultithreadProtected(true);
        }
    }

    let _prio_guard = PriorityGuard::enter(&device);
    let _cap_mmcss = MmcssGuard::enter_capture();

    let (dupl_opt, output_opt, src_w, src_h, start_wgc_directly) = match monitor_output_opt {
        Some((output, coord_w, coord_h, rotation_supported)) => {
            if rotation_supported {
                match create_duplication(&output, &device) {
                    Ok(dupl) => {
                        let mut ddesc = Default::default();
                        unsafe { dupl.GetDesc(&mut ddesc) };
                        (
                            Some(dupl),
                            Some(output),
                            ddesc.ModeDesc.Width.max(16),
                            ddesc.ModeDesc.Height.max(16),
                            false,
                        )
                    }
                    Err(dupl_err) => {
                        if let WgcCaptureTarget::Monitor(hmon) = wgc_target {
                            if hmon.0 != 0 && GraphicsCaptureSession::IsSupported().unwrap_or(false) {
                                (None, Some(output), coord_w, coord_h, true)
                            } else {
                                return Err(dupl_err);
                            }
                        } else {
                            return Err(dupl_err);
                        }
                    }
                }
            } else if let WgcCaptureTarget::Monitor(hmon) = wgc_target {
                if hmon.0 != 0 && GraphicsCaptureSession::IsSupported().unwrap_or(false) {
                    (None, Some(output), coord_w, coord_h, true)
                } else {
                    return Err("Rotacao de monitor nao suportada e WGC indisponivel.".to_string());
                }
            } else {
                return Err("Rotacao de monitor nao suportada.".to_string());
            }
        }
        None => {
            let (win_w, win_h) = match &cfg.source {
                GpuCaptureSource::Window(w) => (w.width.max(16) as u32, w.height.max(16) as u32),
                GpuCaptureSource::Monitor(m) => (m.width.max(16) as u32, m.height.max(16) as u32),
            };
            (None, None, win_w, win_h, true)
        }
    };

    let d3d_device_trim = device.clone();
    let d3d_context_trim = context.clone();

    let result = (|| -> Result<(), String> {
        let video_device: ID3D11VideoDevice = device
            .cast()
            .map_err(|e| format!("ID3D11VideoDevice indisponivel: {}", e))?;
        let video_context: ID3D11VideoContext = context
            .cast()
            .map_err(|e| format!("ID3D11VideoContext indisponivel: {}", e))?;

        let vp_res = build_capture_vp_resources(
            &device,
            &video_device,
            &video_context,
            src_w,
            src_h,
            out_w,
            out_h,
            target_fps,
        )?;

        let (reset_token, dxgi_mgr) = unsafe { dyn_mf_create_dxgi_device_manager() }?;
        unsafe { dxgi_mgr.ResetDevice(&device, reset_token) }
            .map_err(|e| format!("ResetDevice falhou: {}", e))?;

        let hw_enc = create_hardware_encoder(
            adapter_luid,
            &dxgi_mgr,
            out_w,
            out_h,
            target_fps,
            bitrate_bps,
        )?;

        let pool = Arc::new(Nv12TexturePool::new(
            &device,
            &video_device,
            &vp_res.vp_enum,
            out_w,
            out_h,
        )?);

        let encoder_name = hw_enc.encoder_name.clone();
        let _timer_guard = HighResTimerGuard::new();
        crate::diag_info!(
            "gpu-capture",
            "pipeline GPU iniciado (encoder={}, {}x{} @ {}fps, {} kbps, wgc_direct={})",
            encoder_name,
            out_w,
            out_h,
            target_fps,
            bitrate_bps / 1000,
            start_wgc_directly
        );
        on_ready(GpuEncoderInfo {
            encoder_name,
            width: out_w,
            height: out_h,
            target_fps,
        });

        let frame_interval = Duration::from_nanos((1_000_000_000u64 / target_fps as u64).max(4_000_000));
        let frame_dur_100ns = (10_000_000i64 / target_fps as i64).max(40_000);
        let start_clock = Instant::now();

        let is_async = hw_enc.is_async;
        let shutdown_iface = hw_enc.mft.cast::<IMFShutdown>().ok().map(SendCom);
        let has_shutdown_iface = shutdown_iface.is_some();
        let mft_for_core = hw_enc.mft.clone();
        let capi_for_core = hw_enc.codec_api.clone();
        let in_id = hw_enc.in_id;

        let (core, output_thread): (Arc<AsyncEncoderCore>, Option<JoinHandle<()>>) = if is_async {
            let core = Arc::new(AsyncEncoderCore::new(
                mft_for_core,
                in_id,
                true,
                capi_for_core,
                Arc::clone(&pool),
                frame_dur_100ns,
                None,
            ));
            let core_out = Arc::clone(&core);
            let pool_out = Arc::clone(&pool);
            let stop_out = Arc::clone(&stop_flag);
            let enc_box = SendCom(hw_enc);
            let mut on_h264_out = on_h264;
            let handle = std::thread::Builder::new()
                .name("golive-gpu-mft-out".to_string())
                .spawn(move || {
                    let _cm = ComMfGuard::init();
                    let _mm = MmcssGuard::enter_capture();
                    let mut enc = enc_box.into_inner();
                    let Some(eg) = enc.event_gen.clone() else {
                        core_out.finish_drain();
                        return;
                    };
                    let mut ps = H264ParameterSets::new();
                    let mut raw_buf = Vec::<u8>::with_capacity(256 * 1024);
                    let get_ev_flags = if has_shutdown_iface {
                        MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0)
                    } else {
                        MF_EVENT_FLAG_NO_WAIT
                    };

                    while !stop_out.load(Ordering::Acquire) && !core_out.is_finished() {
                        match unsafe { eg.GetEvent(get_ev_flags) } {
                            Ok(ev) => {
                                let ev_type = unsafe { ev.GetType() }.unwrap_or(0);
                                if ev_type == METransformNeedInput.0 as u32 {
                                    core_out.on_need_input();
                                } else if ev_type == METransformHaveOutput.0 as u32 {
                                    loop {
                                        match pull_one_output(&mut enc, &mut raw_buf) {
                                            Ok(Some((got, incomplete, sample_pts_opt))) => {
                                                if got {
                                                    let (slot_pts_opt, release_idx) =
                                                        core_out.on_output_complete();
                                                    if let Some(idx) = release_idx {
                                                        pool_out.release(idx);
                                                    }
                                                    if !stop_out.load(Ordering::Acquire) {
                                                        let pts = sample_pts_opt
                                                            .or(slot_pts_opt)
                                                            .unwrap_or_else(|| start_clock.elapsed());
                                                        emit_normalized_h264(
                                                            &raw_buf,
                                                            &mut ps,
                                                            pts,
                                                            &mut on_h264_out,
                                                        );
                                                    }
                                                }
                                                if !incomplete {
                                                    break;
                                                }
                                            }
                                            Ok(None) => break,
                                            Err(err) => {
                                                core_out.record_error(err);
                                                break;
                                            }
                                        }
                                    }
                                } else if ev_type == MEError.0 as u32 {
                                    let status = unsafe { ev.GetStatus() }
                                        .err()
                                        .map(|e| format!(": {}", e))
                                        .unwrap_or_default();
                                    core_out.record_error(format!("MFT MEError{}", status));
                                    break;
                                }
                            }
                            Err(e) if e.code().0 == HRESULT_MF_E_SHUTDOWN => break,
                            Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => {
                                if !has_shutdown_iface {
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                            }
                            Err(e) => {
                                if !stop_out.load(Ordering::Acquire) {
                                    core_out.record_error(format!("GetEvent falhou: {}", e));
                                }
                                break;
                            }
                        }
                    }

                    core_out.finish_drain();
                })
                .map_err(|e| format!("Falha ao iniciar thread de saida GPU: {}", e))?;
            (core, Some(handle))
        } else {
            let sync_sink = SyncEncoderSink {
                enc: SendCom(hw_enc),
                ps: H264ParameterSets::new(),
                raw_buf: Vec::with_capacity(256 * 1024),
                on_h264: Box::new(on_h264),
            };
            let core = Arc::new(AsyncEncoderCore::new(
                mft_for_core,
                in_id,
                false,
                capi_for_core,
                Arc::clone(&pool),
                frame_dur_100ns,
                Some(sync_sink),
            ));
            (core, None)
        };

        let prev_cap = (vp_res.prev_w * vp_res.prev_h * 4) as usize;
        let cursor_sup = vp_res.vp_caps.MaxInputStreams >= 2;
        let shared = Arc::new(CaptureShared {
            gpu_state: Mutex::new(GpuRenderState {
                vp_res,
                cursor: CursorOverlayState::new(cursor_sup),
                wgc_active: start_wgc_directly,
                have_frame: false,
                pending_submit: false,
                last_submit_at: start_clock - Duration::from_secs(1),
                frame_idx: 0,
                last_preview_at: start_clock - Duration::from_secs(1),
                prev_rgba_scratch: Vec::with_capacity(prev_cap),
            }),
            device: SendCom(device),
            context: SendCom(context),
            video_device: SendCom(video_device),
            video_context: SendCom(video_context),
            core: Arc::clone(&core),
            pool: Arc::clone(&pool),
            kf_flag: Arc::clone(&kf_flag),
            stop_flag: Arc::clone(&stop_flag),
            preview_enabled: Arc::clone(&cfg.preview_enabled),
            out_w,
            out_h,
            target_fps,
            frame_interval,
            start_clock,
        });

        let repeat_shared = Arc::clone(&shared);
        let repeat_stop = Arc::new(AtomicBool::new(false));
        let repeat_stop_th = Arc::clone(&repeat_stop);
        let repeat_thread = std::thread::Builder::new()
            .name("golive-gpu-repeat".to_string())
            .spawn(move || {
                let _cm = ComMfGuard::init();
                let _mm = MmcssGuard::enter_capture();
                while !repeat_stop_th.load(Ordering::Acquire)
                    && !repeat_shared.stop_flag.load(Ordering::Acquire)
                    && !repeat_shared.core.is_finished()
                {
                    std::thread::sleep(Duration::from_millis(50));
                    if repeat_stop_th.load(Ordering::Acquire)
                        || repeat_shared.stop_flag.load(Ordering::Acquire)
                        || repeat_shared.core.is_finished()
                    {
                        break;
                    }
                    let submit_opt = if let Ok(mut st) = repeat_shared.gpu_state.lock() {
                        if !st.have_frame {
                            None
                        } else {
                            let idle = st.last_submit_at.elapsed();
                            let want_kf = repeat_shared.kf_flag.load(Ordering::Acquire);
                            if (want_kf && idle >= Duration::from_millis(100))
                                || (st.pending_submit && idle >= repeat_shared.frame_interval)
                                || idle >= Duration::from_millis(STATIC_HEARTBEAT_MS)
                            {
                                match repeat_shared.try_prepare_submit_locked(&mut st, None, true) {
                                    Ok(opt) => opt,
                                    Err(err) => {
                                        repeat_shared.core.record_error(err);
                                        break;
                                    }
                                }
                            } else {
                                None
                            }
                        }
                    } else {
                        None
                    };
                    if let Some(slot) = submit_opt {
                        repeat_shared.core.submit(slot);
                    }
                }
            })
            .ok();

        let loop_result = if let (Some(dupl), Some(output)) = (dupl_opt, output_opt) {
            match run_duplication_capture(dupl, &output, &shared, &mut on_preview) {
                Ok(DuplExitReason::Stop) => Ok(()),
                Ok(DuplExitReason::SwitchToWgc(reason)) => {
                    crate::diag_warn!("gpu-capture", "alternando DXGI -> WGC: {}", reason);
                    if let WgcCaptureTarget::Monitor(hmon) = wgc_target {
                        if hmon.0 != 0 && GraphicsCaptureSession::IsSupported().unwrap_or(false) {
                            run_wgc_capture(wgc_target, &shared, &mut on_preview)
                        } else {
                            Err(reason)
                        }
                    } else {
                        Err(reason)
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            run_wgc_capture(wgc_target, &shared, &mut on_preview)
        };

        repeat_stop.store(true, Ordering::Release);
        core.mark_finished();
        if let Some(t) = repeat_thread {
            let _ = t.join();
        }

        if is_async {
            core.send_flush_and_eos();
        } else {
            core.send_eos_drain_sync();
        }
        if let Some(sh) = shutdown_iface {
            unsafe {
                let _ = sh.0.Shutdown();
            }
        }
        if let Some(t) = output_thread {
            let _ = t.join();
        }
        core.finish_drain();

        if let Some(err) = core.take_error() {
            return Err(err);
        }
        loop_result
    })();

    // Libera caches de staging e command lists retidos pelo driver DXGI/D3D11 na RAM privada do processo.
    unsafe {
        d3d_context_trim.ClearState();
        d3d_context_trim.Flush();
        if let Ok(dxgi3) = d3d_device_trim.cast::<IDXGIDevice3>() {
            dxgi3.Trim();
        }
    }
    drop(d3d_context_trim);
    drop(d3d_device_trim);

    result
}

#[cfg(test)]
pub struct SyntheticHwEncodeResult {
    pub frames: Vec<(Bytes, bool)>,
    pub encoder_name: String,
    pub profile_used: u32,
    pub elapsed: Duration,
}

/// Helper sintetico de codificacao H.264 HW em GPU para testes de integracao por qualidade/bitrate.
#[cfg(test)]
pub fn encode_synthetic_nv12_frames_hw(
    out_w: u32,
    out_h: u32,
    target_fps: u32,
    bitrate_bps: u32,
    num_frames: u32,
) -> Result<SyntheticHwEncodeResult, String> {
    let _com_mf = ComMfGuard::init()?;

    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|e| format!("CreateDXGIFactory1 falhou: {}", e))?;
    let adapter1 = unsafe { factory.EnumAdapters1(0) }
        .map_err(|e| format!("Nenhum adaptador DXGI: {}", e))?;
    let mut adesc = DXGI_ADAPTER_DESC1::default();
    unsafe { adapter1.GetDesc1(&mut adesc) }
        .map_err(|e| format!("GetDesc1 falhou: {}", e))?;
    let dxgi_adapter: IDXGIAdapter = adapter1
        .cast()
        .map_err(|e| format!("Cast IDXGIAdapter falhou: {}", e))?;

    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut device_opt: Option<ID3D11Device> = None;
    let mut context_opt: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            &dxgi_adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device_opt),
            None,
            Some(&mut context_opt),
        )
    }
    .map_err(|e| format!("D3D11CreateDevice falhou: {}", e))?;
    let device = device_opt.ok_or_else(|| "D3D11Device nulo.".to_string())?;
    let context = context_opt.ok_or_else(|| "D3D11DeviceContext nulo.".to_string())?;
    let video_device: ID3D11VideoDevice = device
        .cast()
        .map_err(|e| format!("ID3D11VideoDevice indisponivel: {}", e))?;
    let video_context: ID3D11VideoContext = context
        .cast()
        .map_err(|e| format!("ID3D11VideoContext indisponivel: {}", e))?;

    let vp_res = build_capture_vp_resources(
        &device,
        &video_device,
        &video_context,
        out_w,
        out_h,
        out_w,
        out_h,
        target_fps,
    )?;

    let (reset_token, dxgi_mgr) = unsafe { dyn_mf_create_dxgi_device_manager() }?;
    unsafe { dxgi_mgr.ResetDevice(&device, reset_token) }
        .map_err(|e| format!("ResetDevice falhou: {}", e))?;

    let mut enc = create_hardware_encoder(
        adesc.AdapterLuid,
        &dxgi_mgr,
        out_w,
        out_h,
        target_fps,
        bitrate_bps,
    )?;
    let encoder_name = enc.encoder_name.clone();
    let profile_used = enc.profile_used;

    let pool = Nv12TexturePool::new(&device, &video_device, &vp_res.vp_enum, out_w, out_h)?;
    let frame_dur_100ns = (10_000_000i64 / target_fps.max(5) as i64).max(40_000);
    let mut bgra = vec![32u8; (out_w * out_h * 4) as usize];
    let mut param_sets = H264ParameterSets::new();
    let mut raw_buf = Vec::<u8>::with_capacity(256 * 1024);
    let mut outputs: Vec<(Bytes, bool)> = Vec::with_capacity(num_frames as usize);
    let mut in_flight: VecDeque<usize> = VecDeque::with_capacity(NV12_POOL_SIZE);

    let drain_async = |enc_ref: &mut HardwareMftEncoder,
                           ps: &mut H264ParameterSets,
                           out_vec: &mut Vec<(Bytes, bool)>,
                           inf: &mut VecDeque<usize>,
                           buf_arg: &mut Vec<u8>,
                           wait_no_events: bool|
     -> Result<u32, String> {
        let Some(eg) = enc_ref.event_gen.clone() else {
            return Ok(1);
        };
        let mut credits = 0u32;
        let flags = if wait_no_events {
            MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0)
        } else {
            MF_EVENT_FLAG_NO_WAIT
        };
        loop {
            match unsafe { eg.GetEvent(flags) } {
                Ok(ev) => {
                    let ev_type = unsafe { ev.GetType() }.unwrap_or(0);
                    if ev_type == METransformNeedInput.0 as u32 {
                        credits += 1;
                        if wait_no_events {
                            break;
                        }
                    } else if ev_type == METransformHaveOutput.0 as u32 {
                        loop {
                            match pull_one_output(enc_ref, buf_arg)? {
                                Some((got, incomplete, _)) => {
                                    if got {
                                        if let Some(sidx) = inf.pop_front() {
                                            pool.release(sidx);
                                        }
                                        if let Some((au, is_kf)) = ps.normalize_access_unit(buf_arg)
                                        {
                                            out_vec.push((au, is_kf));
                                        }
                                    }
                                    if !incomplete {
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                        if wait_no_events {
                            break;
                        }
                    }
                }
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                Err(e) if e.code().0 == HRESULT_MF_E_SHUTDOWN => break,
                Err(e) => return Err(format!("GetEvent teste falhou: {}", e)),
            }
        }
        Ok(credits)
    };

    unsafe {
        context.UpdateSubresource(
            &vp_res.canvas_tex,
            0,
            None,
            bgra.as_ptr() as *const _,
            out_w * 4,
            0,
        );
    }

    let strip_h = 64u32.min(out_h);
    let strip_box = D3D11_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: out_w,
        bottom: strip_h,
        back: 1,
    };

    let bench_start = Instant::now();
    let mut credits = if enc.is_async { 0 } else { num_frames };
    for i in 0..num_frames {
        let band_x = (i as usize * 32) % (out_w as usize - 16);
        for y in 0..strip_h as usize {
            for x in band_x..(band_x + 16).min(out_w as usize) {
                let p = (y * out_w as usize + x) * 4;
                bgra[p] = ((i * 37) % 250) as u8;
                bgra[p + 1] = 200;
                bgra[p + 2] = 60;
                bgra[p + 3] = 255;
            }
        }
        unsafe {
            context.UpdateSubresource(
                &vp_res.canvas_tex,
                0,
                Some(&strip_box),
                bgra.as_ptr() as *const _,
                out_w * 4,
                0,
            );
        }

        while enc.is_async && credits == 0 {
            credits += drain_async(
                &mut enc,
                &mut param_sets,
                &mut outputs,
                &mut in_flight,
                &mut raw_buf,
                true,
            )?;
        }

        let (slot_idx, slot_ov) = pool
            .acquire_free(Duration::from_millis(500))
            .ok_or_else(|| "Timeout aguardando slot NV12 em teste".to_string())?;
        let mut s = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(vp_res.canvas_iv.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        let blt_res = unsafe {
            video_context.VideoProcessorBlt(&vp_res.vp, &slot_ov, 0, std::slice::from_ref(&s))
        };
        unsafe { ManuallyDrop::drop(&mut s.pInputSurface) };
        blt_res.map_err(|e| format!("VideoProcessorBlt teste falhou: {}", e))?;
        unsafe { context.Flush() };

        if i == 0 {
            if let Some(ref capi) = enc.codec_api {
                unsafe {
                    let _ = capi.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32));
                }
            }
        }

        let sample =
            pool.create_sample_for_slot(slot_idx, i as i64 * frame_dur_100ns, frame_dur_100ns)?;
        in_flight.push_back(slot_idx);
        unsafe { enc.mft.ProcessInput(enc.in_id, &sample, 0) }
            .map_err(|e| format!("ProcessInput teste falhou: {}", e))?;

        if enc.is_async {
            credits = credits.saturating_sub(1);
            credits += drain_async(
                &mut enc,
                &mut param_sets,
                &mut outputs,
                &mut in_flight,
                &mut raw_buf,
                false,
            )?;
        } else {
            loop {
                match pull_one_output(&mut enc, &mut raw_buf)? {
                    Some((got, incomplete, _)) => {
                        if got {
                            if let Some(sidx) = in_flight.pop_front() {
                                pool.release(sidx);
                            }
                            if let Some((au, is_kf)) = param_sets.normalize_access_unit(&raw_buf) {
                                outputs.push((au, is_kf));
                            }
                        }
                        if !incomplete {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    unsafe {
        let _ = enc.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
        let _ = enc.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
    }
    if enc.is_async {
        let deadline = Instant::now() + Duration::from_secs(2);
        while outputs.len() < num_frames as usize && Instant::now() < deadline {
            let _ = drain_async(
                &mut enc,
                &mut param_sets,
                &mut outputs,
                &mut in_flight,
                &mut raw_buf,
                false,
            )?;
            if outputs.len() < num_frames as usize {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    let elapsed = bench_start.elapsed();

    if let Ok(sh) = enc.mft.cast::<IMFShutdown>() {
        unsafe {
            let _ = sh.Shutdown();
        }
    }
    Ok(SyntheticHwEncodeResult {
        frames: outputs,
        encoder_name,
        profile_used,
        elapsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::codecs::H264VideoDecoder;

    #[test]
    fn test_compute_mft_header_fps_respects_level_5_2() {
        assert_eq!(compute_mft_header_fps(1024, 576, 240), 240);
        assert_eq!(compute_mft_header_fps(1920, 1080, 60), 60);
        assert_eq!(compute_mft_header_fps(1920, 1080, 240), 240);
        assert_eq!(compute_mft_header_fps(2560, 1440, 120), 120);
        assert_eq!(compute_mft_header_fps(2560, 1440, 240), 144);
        assert_eq!(compute_mft_header_fps(3840, 2160, 15), 15);
        assert_eq!(compute_mft_header_fps(3840, 2160, 60), 60);
        assert_eq!(compute_mft_header_fps(3840, 2160, 240), 64);
    }

    #[test]
    fn test_is_due_with_slack_avoids_halving_game_fps() {
        // Jogo a 100-144 Hz ou monitor a 60 Hz tem pequeno jitter de timer (ex: 13 ms em alvo 16.6 ms):
        // tolerancia de 75% evita descartar 1 a cada 2 frames e cortar o FPS pela metade.
        let interval_60 = Duration::from_micros(16_667);
        assert!(is_due_with_slack(Duration::from_millis(13), interval_60, false));
        assert!(!is_due_with_slack(Duration::from_millis(10), interval_60, false));

        let interval_240 = Duration::from_micros(4_167);
        assert!(is_due_with_slack(Duration::from_micros(3_200), interval_240, false));
        assert!(is_due_with_slack(Duration::from_millis(0), interval_240, true));
    }

    #[test]
    fn test_async_encoder_mailbox_no_lost_wakeup() {
        let mut st = AsyncMailboxState::new();

        // 1) METransformNeedInput chega ANTES do submit: credito retido
        assert!(st.on_need_input().is_none());
        assert_eq!(st.need_input, 1);

        let a1 = st.submit(SubmittedSlot {
            slot_idx: 0,
            pts: Duration::from_millis(16),
            force_kf: false,
        });
        assert!(matches!(a1, MailboxSubmitAction::ProcessNow(s) if s.slot_idx == 0));
        assert_eq!(st.need_input, 0);

        // 2) Submit sem credito retido entra na caixa 1-slot; novo submit sobrescreve e preserva force_kf
        let a2 = st.submit(SubmittedSlot {
            slot_idx: 1,
            pts: Duration::from_millis(33),
            force_kf: true,
        });
        assert_eq!(
            a2,
            MailboxSubmitAction::QueuedLatest {
                replaced_slot: None
            }
        );

        let a3 = st.submit(SubmittedSlot {
            slot_idx: 2,
            pts: Duration::from_millis(50),
            force_kf: false,
        });
        assert_eq!(
            a3,
            MailboxSubmitAction::QueuedLatest {
                replaced_slot: Some(1)
            }
        );

        // 3) METransformNeedInput drena o pending preservando force_kf=true
        let drained = st.on_need_input().expect("pending deve ser drenado");
        assert_eq!(drained.slot_idx, 2);
        assert!(drained.force_kf);

        // 4) Output complete retem 1 slot no retire_delay e libera no proximo
        let (p0, r0) = st.on_output_complete();
        assert_eq!(p0, Some(Duration::from_millis(16)));
        assert_eq!(r0, None);
        let (p1, r1) = st.on_output_complete();
        assert_eq!(p1, Some(Duration::from_millis(50)));
        assert_eq!(r1, Some(0));

        // 5) finish_drain libera tudo restante sem leak
        let remaining = st.finish_drain();
        assert_eq!(remaining, vec![2]);
    }

    #[test]
    fn test_hw_mft_all_qualities_at_700kbps_decodable_by_openh264() {
        let qualities = [
            (1024u32, 576u32, 240u32),
            (1920, 1080, 60),
            (1920, 1080, 240),
            (2560, 1440, 120),
            (3840, 2160, 15),
            (3840, 2160, 60),
            (3840, 2160, 240),
        ];

        for (w, h, fps) in qualities {
            let res = match encode_synthetic_nv12_frames_hw(w, h, fps, 700_000, 15) {
                Ok(r) => r,
                Err(e) => {
                    if e.contains("Nenhum codificador H.264") || e.contains("Nenhum adaptador") {
                        eprintln!("Aviso: GPU HW MFT indisponivel neste ambiente: {}", e);
                        return;
                    }
                    panic!("Falha inesperada em {}x{}@{}fps: {}", w, h, fps, e);
                }
            };
            let frames = res.frames;

            assert!(
                frames.len() >= 10,
                "Encoder HW gerou apenas {} quadros em {}x{}@{}fps",
                frames.len(),
                w,
                h,
                fps
            );
            assert!(
                frames[0].1,
                "Primeiro quadro deve ser IDR em {}x{}@{}fps",
                w,
                h,
                fps
            );

            let first_info = inspect_h264_au(&frames[0].0);
            eprintln!(
                "[HW-MFT {}x{}@{}fps] encoder={} profile_used={} sps=(profile={:?}, flags={:?}, level={:?})",
                w,
                h,
                fps,
                res.encoder_name,
                res.profile_used,
                first_info.profile_idc,
                first_info.constraint_flags,
                first_info.level_idc
            );
            assert!(first_info.has_sps && first_info.has_pps && first_info.has_idr);
            assert!(
                first_info.level_idc.unwrap_or(99) <= 52,
                "Level {} > 5.2 em {}x{}@{}fps",
                first_info.level_idc.unwrap_or(99),
                w,
                h,
                fps
            );
            let hw_friendly_profile = first_info.profile_idc == Some(0x4D)
                || (first_info.profile_idc == Some(0x42)
                    && (first_info.constraint_flags.unwrap_or(0) & 0x40) != 0);
            assert!(
                hw_friendly_profile,
                "SPS nao-compativel com decodificacao HW (profile={:?}, flags={:?}) em {}x{}@{}fps",
                first_info.profile_idc,
                first_info.constraint_flags,
                w,
                h,
                fps
            );

            let mut dec = H264VideoDecoder::new().expect("OpenH264 decoder");
            let mut rgba = Vec::new();
            let mut decoded_count = 0usize;
            for (idx, (au, _is_kf)) in frames.iter().enumerate() {
                let info = inspect_h264_au(au);
                assert!(
                    !info.has_b_slice,
                    "B-slice detectada no frame {} ({}x{}@{}fps)",
                    idx,
                    w,
                    h,
                    fps
                );
                assert!(
                    !info.has_sei && !info.has_aud,
                    "SEI/AUD presente no frame {}",
                    idx
                );
                let dec_res = dec.decode_annex_b(au, &mut rgba).unwrap_or_else(|e| {
                    panic!(
                        "Erro OpenH264 no frame {} ({}x{}@{}fps): {}",
                        idx, w, h, fps, e
                    )
                });
                if let Some((dw, dh)) = dec_res {
                    assert_eq!((dw as u32, dh as u32), (w, h));
                    decoded_count += 1;
                }
            }
            assert!(
                decoded_count >= frames.len() - 1,
                "OpenH264 decodificou apenas {}/{} quadros em {}x{}@{}fps",
                decoded_count,
                frames.len(),
                w,
                h,
                fps
            );
        }
    }

    #[test]
    fn test_hw_mft_4k_high_bitrate_throughput() {
        let res = match encode_synthetic_nv12_frames_hw(3840, 2160, 60, 16_000_000, 30) {
            Ok(r) => r,
            Err(e) => {
                if e.contains("Nenhum codificador H.264") || e.contains("Nenhum adaptador") {
                    eprintln!("Aviso: GPU HW MFT indisponivel neste ambiente: {}", e);
                    return;
                }
                panic!("Falha 4K@60fps 16Mbps: {}", e);
            }
        };
        let fps_measured = res.frames.len() as f64 / res.elapsed.as_secs_f64().max(0.001);
        let first_info = inspect_h264_au(&res.frames[0].0);
        eprintln!(
            "[4K 16Mbps] encoder={} frames={}/30 elapsed={:?} fps={:.1} sps=(profile={:?}, flags={:?}, level={:?})",
            res.encoder_name,
            res.frames.len(),
            res.elapsed,
            fps_measured,
            first_info.profile_idc,
            first_info.constraint_flags,
            first_info.level_idc
        );
        assert!(res.frames.len() >= 25, "Gerou apenas {} frames 4K", res.frames.len());
        assert!(
            fps_measured >= 50.0,
            "Throughput 4K HW MFT insuficiente: {:.1} fps (esperado >= 50 fps)",
            fps_measured
        );
        for (au, _) in &res.frames {
            let info = inspect_h264_au(au);
            assert!(!info.has_b_slice, "4K HW MFT emitiu B-slice");
        }
        assert!(first_info.level_idc.unwrap_or(99) <= 52);
    }

    #[test]
    fn test_wgc_should_recreate_pool_handles_minimize_and_resize() {
        let pool_1080 = SizeInt32 {
            Width: 1920,
            Height: 1080,
        };
        assert_eq!(wgc_should_recreate_pool(pool_1080, pool_1080), None);
        // Janela minimizada (ContentSize 0x0 ou <16) nunca deve recriar pool
        assert_eq!(
            wgc_should_recreate_pool(pool_1080, SizeInt32 { Width: 0, Height: 0 }),
            None
        );
        assert_eq!(
            wgc_should_recreate_pool(pool_1080, SizeInt32 { Width: 12, Height: 12 }),
            None
        );
        // Crescimento ou reducao valida >= 16 exige Recreate com o novo tamanho
        let grown = SizeInt32 {
            Width: 2560,
            Height: 1440,
        };
        assert_eq!(wgc_should_recreate_pool(pool_1080, grown), Some(grown));
        let shrunk = SizeInt32 {
            Width: 1280,
            Height: 720,
        };
        assert_eq!(wgc_should_recreate_pool(pool_1080, shrunk), Some(shrunk));
    }
}
