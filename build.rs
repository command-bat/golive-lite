//! Build script that embeds the multi-size GoLive Lite `.res` icon into Windows executables
//! (passing `.res` on MSVC or converting `.res` -> COFF `.o` via `windres` on MinGW GNU).

#[path = "src/ui/icon_raster.rs"]
mod icon_raster;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src/ui/icon_raster.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));
    let res_path = out_dir.join("golive-lite-icon.res");
    let pkg_version = env!("CARGO_PKG_VERSION");
    let res_bytes = build_win32_icon_res(&[16, 20, 24, 32, 48, 64, 256], pkg_version);
    fs::write(&res_path, res_bytes).expect("failed to write Win32 icon .res file");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let obj_path = out_dir.join("golive-lite-icon.o");
        let windres = find_windres();
        let status = Command::new(&windres)
            .arg("-i")
            .arg(&res_path)
            .arg("-J")
            .arg("res")
            .arg("-o")
            .arg(&obj_path)
            .arg("-O")
            .arg("coff")
            .status()
            .expect("failed to execute windres for GNU Windows resource icon");
        assert!(status.success(), "windres exited with status {}", status);
        println!("cargo:rustc-link-arg-bins={}", obj_path.display());

        // Vincula libstdc++ / libgcc estaticamente para rodar em qualquer Windows limpo (Win8-Win11)
        // sem depender de libstdc++-6.dll / libgcc_s_seh-1.dll externa.
        // Isola apenas `libstdc++.a` em uma pasta dedicada no OUT_DIR para que o `-lstdc++`
        // emitido pelo openh264-sys2 resolva `libstdc++.a` antes do `libstdc++.dll.a` do MinGW.
        println!("cargo:rustc-link-arg-bins=-static-libgcc");
        println!("cargo:rustc-link-arg-bins=-static-libstdc++");
        if let Some(libstdcpp) = find_gcc_static_lib("libstdc++.a") {
            let static_dir = out_dir.join("static-stdcpp");
            let _ = fs::create_dir_all(&static_dir);
            let dst = static_dir.join("libstdc++.a");
            let src_len = fs::metadata(&libstdcpp).map(|m| m.len()).unwrap_or(0);
            let dst_len = fs::metadata(&dst).map(|m| m.len()).unwrap_or(u64::MAX);
            if src_len == 0 || src_len != dst_len {
                fs::copy(&libstdcpp, &dst).expect("failed to copy static libstdc++.a to OUT_DIR");
            }
            println!("cargo:rustc-link-search=native={}", static_dir.display());
        }
        println!("cargo:rustc-link-arg-bins=-Wl,--as-needed");
    } else {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
    }
}

fn find_gcc_static_lib(name: &str) -> Option<PathBuf> {
    let out = Command::new("gcc")
        .arg(format!("-print-file-name={}", name))
        .output()
        .ok()?;
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let p = PathBuf::from(&raw);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

fn find_windres() -> String {
    for candidate in ["windres", "x86_64-w64-mingw32-windres", "llvm-windres"] {
        if Command::new(candidate).arg("--version").output().is_ok() {
            return candidate.to_string();
        }
    }
    if let Ok(out) = Command::new("gcc").arg("-print-prog-name=windres").output() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return path;
        }
    }
    "windres".to_string()
}

fn build_win32_icon_res(sizes: &[u32], pkg_version: &str) -> Vec<u8> {
    // 32-byte Win32 `.res` sentinel header
    let mut res = vec![
        0x00, 0x00, 0x00, 0x00, // DataSize = 0
        0x20, 0x00, 0x00, 0x00, // HeaderSize = 32
        0xFF, 0xFF, 0x00, 0x00, // TYPE = 0
        0xFF, 0xFF, 0x00, 0x00, // NAME = 0
        0x00, 0x00, 0x00, 0x00, // DataVersion
        0x00, 0x00, // MemoryFlags
        0x00, 0x00, // LanguageId
        0x00, 0x00, 0x00, 0x00, // Version
        0x00, 0x00, 0x00, 0x00, // Characteristics
    ];

    let mut group = Vec::with_capacity(6 + sizes.len() * 14);
    group.extend_from_slice(&0u16.to_le_bytes()); // idReserved
    group.extend_from_slice(&1u16.to_le_bytes()); // idType = 1 (RT_ICON)
    group.extend_from_slice(&(sizes.len() as u16).to_le_bytes());

    for (idx, &size) in sizes.iter().enumerate() {
        let icon_dib = encode_icon_dib(size);
        let res_id = (idx + 1) as u16;
        push_res_entry(&mut res, 3, res_id, 0x1010, &icon_dib);

        let wh_byte = if size >= 256 { 0u8 } else { size as u8 };
        group.push(wh_byte); // bWidth
        group.push(wh_byte); // bHeight
        group.push(0); // bColorCount
        group.push(0); // bReserved
        group.extend_from_slice(&1u16.to_le_bytes()); // wPlanes
        group.extend_from_slice(&32u16.to_le_bytes()); // wBitCount
        group.extend_from_slice(&(icon_dib.len() as u32).to_le_bytes()); // dwBytesInRes
        group.extend_from_slice(&res_id.to_le_bytes()); // nID
    }

    // RT_GROUP_ICON = 14, ID = 1
    push_res_entry(&mut res, 14, 1, 0x1030, &group);

    // RT_VERSION = 16, ID = 1
    let version_blob = build_vs_version_info_res(
        pkg_version,
        &[
            ("CompanyName", "Go Live"),
            ("FileDescription", "Go Live - Lite"),
            ("FileVersion", pkg_version),
            ("InternalName", "Go Live Lite - Desktop"),
            ("OriginalFilename", "golive-lite.exe"),
            ("ProductName", "Go Live - Lite"),
            ("ProductVersion", pkg_version),
        ],
    );
    push_res_entry(&mut res, 16, 1, 0x0030, &version_blob);

    // RT_MANIFEST = 24, ID = 1 (suporte oficial Win11/Win10/Win8.1/Win8/Win7 + PerMonitorV2 + CCv6)
    let manifest_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="amd64" name="GoLive.Lite.Desktop" type="win32"/>
  <description>Go Live - Lite</description>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>
"#;
    push_res_entry(&mut res, 24, 1, 0x1030, manifest_xml);
    res
}

fn align4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

fn push_utf16_z(buf: &mut Vec<u8>, s: &str) {
    for u in s.encode_utf16().chain(std::iter::once(0u16)) {
        buf.extend_from_slice(&u.to_le_bytes());
    }
}

fn parse_version_quad(v: &str) -> (u16, u16, u16, u16) {
    let clean = v.trim().split(['-', '+']).next().unwrap_or("0.1.0");
    let mut it = clean.split('.').map(|s| s.parse::<u16>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(1),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

fn build_version_string_entry(key: &str, val: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    let w_val_len = (val.encode_utf16().count() + 1) as u16;
    out.extend_from_slice(&w_val_len.to_le_bytes()); // wValueLength (WCHARs incl NUL)
    out.extend_from_slice(&1u16.to_le_bytes()); // wType = 1 (text)
    push_utf16_z(&mut out, key);
    align4(&mut out);
    push_utf16_z(&mut out, val);
    let len = out.len() as u16;
    out[0..2].copy_from_slice(&len.to_le_bytes());
    out
}

fn build_vs_version_info_res(version: &str, entries: &[(&str, &str)]) -> Vec<u8> {
    let (maj, min, pat, bld) = parse_version_quad(version);
    let ver_ms = ((maj as u32) << 16) | (min as u32);
    let ver_ls = ((pat as u32) << 16) | (bld as u32);

    // StringTable "040904B0"
    let mut str_table = Vec::new();
    str_table.extend_from_slice(&0u16.to_le_bytes());
    str_table.extend_from_slice(&0u16.to_le_bytes());
    str_table.extend_from_slice(&1u16.to_le_bytes());
    push_utf16_z(&mut str_table, "040904B0");
    for &(k, v) in entries {
        align4(&mut str_table);
        let entry = build_version_string_entry(k, v);
        str_table.extend_from_slice(&entry);
    }
    let st_len = str_table.len() as u16;
    str_table[0..2].copy_from_slice(&st_len.to_le_bytes());

    // StringFileInfo
    let mut sfi = Vec::new();
    sfi.extend_from_slice(&0u16.to_le_bytes());
    sfi.extend_from_slice(&0u16.to_le_bytes());
    sfi.extend_from_slice(&1u16.to_le_bytes());
    push_utf16_z(&mut sfi, "StringFileInfo");
    align4(&mut sfi);
    sfi.extend_from_slice(&str_table);
    let sfi_len = sfi.len() as u16;
    sfi[0..2].copy_from_slice(&sfi_len.to_le_bytes());

    // Var "Translation"
    let mut var = Vec::new();
    var.extend_from_slice(&0u16.to_le_bytes());
    var.extend_from_slice(&4u16.to_le_bytes()); // 4 bytes
    var.extend_from_slice(&0u16.to_le_bytes()); // binary
    push_utf16_z(&mut var, "Translation");
    align4(&mut var);
    var.extend_from_slice(&0x0409u16.to_le_bytes());
    var.extend_from_slice(&0x04B0u16.to_le_bytes());
    let var_len = var.len() as u16;
    var[0..2].copy_from_slice(&var_len.to_le_bytes());

    // VarFileInfo
    let mut vfi = Vec::new();
    vfi.extend_from_slice(&0u16.to_le_bytes());
    vfi.extend_from_slice(&0u16.to_le_bytes());
    vfi.extend_from_slice(&1u16.to_le_bytes());
    push_utf16_z(&mut vfi, "VarFileInfo");
    align4(&mut vfi);
    vfi.extend_from_slice(&var);
    let vfi_len = vfi.len() as u16;
    vfi[0..2].copy_from_slice(&vfi_len.to_le_bytes());

    // Root VS_VERSIONINFO
    let mut root = Vec::new();
    root.extend_from_slice(&0u16.to_le_bytes());
    root.extend_from_slice(&52u16.to_le_bytes()); // sizeof(VS_FIXEDFILEINFO)
    root.extend_from_slice(&0u16.to_le_bytes());
    push_utf16_z(&mut root, "VS_VERSION_INFO");
    align4(&mut root);
    for dword in [
        0xFEEF_04BDu32, // dwSignature
        0x0001_0000u32, // dwStrucVersion
        ver_ms,         // dwFileVersionMS
        ver_ls,         // dwFileVersionLS
        ver_ms,         // dwProductVersionMS
        ver_ls,         // dwProductVersionLS
        0x0000_003Fu32, // dwFileFlagsMask
        0x0000_0000u32, // dwFileFlags
        0x0004_0004u32, // dwFileOS = VOS_NT_WINDOWS32
        0x0000_0001u32, // dwFileType = VFT_APP
        0x0000_0000u32, // dwFileSubtype
        0x0000_0000u32, // dwFileDateMS
        0x0000_0000u32, // dwFileDateLS
    ] {
        root.extend_from_slice(&dword.to_le_bytes());
    }
    align4(&mut root);
    root.extend_from_slice(&sfi);
    align4(&mut root);
    root.extend_from_slice(&vfi);
    let root_len = root.len() as u16;
    root[0..2].copy_from_slice(&root_len.to_le_bytes());
    root
}

fn push_res_entry(out: &mut Vec<u8>, res_type: u16, res_id: u16, mem_flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // DataSize
    out.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&res_type.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&res_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&mem_flags.to_le_bytes()); // MemoryFlags
    out.extend_from_slice(&0x0409u16.to_le_bytes()); // LanguageId (en-US)
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn encode_icon_dib(size: u32) -> Vec<u8> {
    let rgba = icon_raster::render_golive_lite_icon_rgba(size);
    let pixel_bytes = (size * size * 4) as usize;
    let mask_stride = (((size + 31) / 32) * 4) as usize;
    let mask_bytes = mask_stride * size as usize;

    let mut dib = Vec::with_capacity(40 + pixel_bytes + mask_bytes);
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight (XOR + AND)
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&(pixel_bytes as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // Bottom-up BGRA scanlines
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            dib.push(rgba[idx + 2]);
            dib.push(rgba[idx + 1]);
            dib.push(rgba[idx]);
            dib.push(rgba[idx + 3]);
        }
    }
    dib.resize(dib.len() + mask_bytes, 0);
    dib
}
