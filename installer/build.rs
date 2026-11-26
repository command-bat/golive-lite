//! Empacota os payloads opcionais comprimidos em zlib (`golive-lite-core.zlib`,
//! `golive-lite-mods.zlib`, `golive-lite-unins.zlib`) e embute o icone multi-resolucao,
//! metadados de versao e manifesto Win32 do instalador nativo.

#[path = "../src/ui/icon_raster.rs"]
mod icon_raster;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_zlib_payload(out_path: &Path, input_path: Option<&Path>) {
    let raw = if let Some(p) = input_path.filter(|p| p.is_file()) {
        println!("cargo:rerun-if-changed={}", p.display());
        fs::read(p).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&raw).expect("failed to compress payload");
    let compressed = enc.finish().expect("failed to finish zlib compression");
    fs::write(out_path, compressed).expect("failed to write compressed zlib payload");
}

fn env_nonempty_path(name: &str) -> Option<PathBuf> {
    env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../src/ui/icon_raster.rs");
    println!("cargo:rerun-if-env-changed=GOLIVE_LITE_UNINS_ONLY");
    println!("cargo:rerun-if-env-changed=GOLIVE_LITE_CORE_EXE");
    println!("cargo:rerun-if-env-changed=GOLIVE_LITE_MODS_EXE");
    println!("cargo:rerun-if-env-changed=GOLIVE_LITE_UNINS_EXE");
    println!("cargo:rerun-if-env-changed=GOLIVE_LITE_PAYLOAD_EXE");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));

    let unins_only = env::var("GOLIVE_LITE_UNINS_ONLY").as_deref() == Ok("1");
    if unins_only {
        write_zlib_payload(&out_dir.join("golive-lite-core.zlib"), None);
        write_zlib_payload(&out_dir.join("golive-lite-mods.zlib"), None);
        write_zlib_payload(&out_dir.join("golive-lite-unins.zlib"), None);
    } else {
        let legacy_exe = env_nonempty_path("GOLIVE_LITE_PAYLOAD_EXE")
            .unwrap_or_else(|| manifest_dir.join("../target/release/golive-lite.exe"));

        let core_candidate = env_nonempty_path("GOLIVE_LITE_CORE_EXE")
            .unwrap_or_else(|| manifest_dir.join("../target/release/golive-lite-core.exe"));
        let core_path = if core_candidate.is_file() {
            Some(core_candidate)
        } else if legacy_exe.is_file() {
            Some(legacy_exe.clone())
        } else {
            None
        };

        let mods_candidate = env_nonempty_path("GOLIVE_LITE_MODS_EXE")
            .unwrap_or_else(|| manifest_dir.join("../target/release/golive-lite-mods.exe"));
        let mods_path = if mods_candidate.is_file() {
            Some(mods_candidate)
        } else if legacy_exe.is_file() {
            Some(legacy_exe)
        } else {
            None
        };

        let unins_candidate = env_nonempty_path("GOLIVE_LITE_UNINS_EXE")
            .unwrap_or_else(|| manifest_dir.join("../target/release/golive-lite-unins.exe"));
        let unins_path = if unins_candidate.is_file() {
            Some(unins_candidate)
        } else {
            None
        };

        write_zlib_payload(&out_dir.join("golive-lite-core.zlib"), core_path.as_deref());
        write_zlib_payload(&out_dir.join("golive-lite-mods.zlib"), mods_path.as_deref());
        write_zlib_payload(
            &out_dir.join("golive-lite-unins.zlib"),
            unins_path.as_deref(),
        );
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let res_path = out_dir.join("golive-lite-setup.res");
    let pkg_version = env!("CARGO_PKG_VERSION");
    let res_bytes = build_win32_setup_res(&[16, 20, 24, 32, 48, 64, 256], pkg_version);
    fs::write(&res_path, res_bytes).expect("failed to write setup .res file");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let obj_path = out_dir.join("golive-lite-setup.o");
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
            .expect("failed to execute windres for installer resource");
        assert!(status.success(), "windres exited with status {}", status);
        println!("cargo:rustc-link-arg-bins={}", obj_path.display());
        println!("cargo:rustc-link-arg-bins=-static-libgcc");
        println!("cargo:rustc-link-arg-bins=-Wl,--as-needed");
    } else {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
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

fn build_win32_setup_res(sizes: &[u32], pkg_version: &str) -> Vec<u8> {
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
        group.push(wh_byte);
        group.push(wh_byte);
        group.push(0);
        group.push(0);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(icon_dib.len() as u32).to_le_bytes());
        group.extend_from_slice(&res_id.to_le_bytes());
    }

    push_res_entry(&mut res, 14, 1, 0x1030, &group);

    let version_blob = build_vs_version_info_res(
        pkg_version,
        &[
            ("CompanyName", "Go Live"),
            ("FileDescription", "Instalador do Go Live - Lite"),
            ("FileVersion", pkg_version),
            ("InternalName", "Go Live Lite - Desktop Setup"),
            ("OriginalFilename", "Go-Live-Lite-Setup.exe"),
            ("ProductName", "Go Live - Lite"),
            ("ProductVersion", pkg_version),
        ],
    );
    push_res_entry(&mut res, 16, 1, 0x0030, &version_blob);

    let manifest_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="amd64" name="GoLive.Lite.Setup" type="win32"/>
  <description>Go Live - Lite Setup</description>
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
    out.extend_from_slice(&0u16.to_le_bytes());
    let w_val_len = (val.encode_utf16().count() + 1) as u16;
    out.extend_from_slice(&w_val_len.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
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

    let mut sfi = Vec::new();
    sfi.extend_from_slice(&0u16.to_le_bytes());
    sfi.extend_from_slice(&0u16.to_le_bytes());
    sfi.extend_from_slice(&1u16.to_le_bytes());
    push_utf16_z(&mut sfi, "StringFileInfo");
    align4(&mut sfi);
    sfi.extend_from_slice(&str_table);
    let sfi_len = sfi.len() as u16;
    sfi[0..2].copy_from_slice(&sfi_len.to_le_bytes());

    let mut var = Vec::new();
    var.extend_from_slice(&0u16.to_le_bytes());
    var.extend_from_slice(&4u16.to_le_bytes());
    var.extend_from_slice(&0u16.to_le_bytes());
    push_utf16_z(&mut var, "Translation");
    align4(&mut var);
    var.extend_from_slice(&0x0409u16.to_le_bytes());
    var.extend_from_slice(&0x04B0u16.to_le_bytes());
    let var_len = var.len() as u16;
    var[0..2].copy_from_slice(&var_len.to_le_bytes());

    let mut vfi = Vec::new();
    vfi.extend_from_slice(&0u16.to_le_bytes());
    vfi.extend_from_slice(&0u16.to_le_bytes());
    vfi.extend_from_slice(&1u16.to_le_bytes());
    push_utf16_z(&mut vfi, "VarFileInfo");
    align4(&mut vfi);
    vfi.extend_from_slice(&var);
    let vfi_len = vfi.len() as u16;
    vfi[0..2].copy_from_slice(&vfi_len.to_le_bytes());

    let mut root = Vec::new();
    root.extend_from_slice(&0u16.to_le_bytes());
    root.extend_from_slice(&52u16.to_le_bytes());
    root.extend_from_slice(&0u16.to_le_bytes());
    push_utf16_z(&mut root, "VS_VERSION_INFO");
    align4(&mut root);
    for dword in [
        0xFEEF_04BDu32,
        0x0001_0000u32,
        ver_ms,
        ver_ls,
        ver_ms,
        ver_ls,
        0x0000_003Fu32,
        0x0000_0000u32,
        0x0004_0004u32,
        0x0000_0001u32,
        0x0000_0000u32,
        0x0000_0000u32,
        0x0000_0000u32,
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
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&res_type.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&res_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&mem_flags.to_le_bytes());
    out.extend_from_slice(&0x0409u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
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
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(size as i32).to_le_bytes());
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());

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
