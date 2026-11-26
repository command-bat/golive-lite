//! Atualizador remoto leve e opcional/obrigatorio para o GoLive Lite.
//! Verifica o manifesto JSON apenas na inicializacao (apos 5s), a cada 6 horas (com ETag/304)
//! ou sob demanda quando o usuario clica em "Verificar atualizacoes".

use std::cmp::Ordering;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::mpsc;

use crate::net::auth::build_http_agent;
use crate::net::protocol::{is_safe_http_url, open_external_url};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Repositorio do GitHub onde ficam as releases (`usuario/repositorio`). Vem do build: o
/// `build-installer.ps1` le do `git remote` e o GitHub Actions usa o proprio repositorio.
pub const GITHUB_REPO: &str = match option_env!("GOLIVE_LITE_GITHUB_REPO") {
    Some(r) => r,
    None => "",
};

/// Retorna a URL da ultima release no GitHub (override: `GOLIVE_LITE_UPDATE_URL`).
pub fn update_manifest_url() -> String {
    if let Ok(raw) = std::env::var("GOLIVE_LITE_UPDATE_URL") {
        let trimmed = raw.trim();
        if is_safe_http_url(trimmed) {
            return trimmed.to_string();
        }
    }
    github_latest_release_url(GITHUB_REPO).unwrap_or_default()
}

pub fn github_latest_release_url(repo: &str) -> Option<String> {
    let repo = repo.trim().trim_matches('/');
    let ok = repo.split('/').count() == 2
        && repo.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'));
    ok.then(|| format!("https://api.github.com/repos/{}/releases/latest", repo))
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default, alias = "min_version")]
    pub min_version: Option<String>,
    #[serde(default)]
    pub mandatory: bool,
    /// Executavel do nucleo (sem modificacoes).
    #[serde(default, alias = "download_url", alias = "url")]
    pub download_url: String,
    /// Executavel com modificacoes; quem foi instalado com elas baixa este (nao vira nucleo).
    #[serde(default, alias = "mods_download_url")]
    pub mods_download_url: Option<String>,
    #[serde(default, alias = "mods_sha256")]
    pub mods_sha256: Option<String>,
    #[serde(default, alias = "release_page_url")]
    pub release_page_url: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default, alias = "published_at")]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub current_version: String,
    pub is_mandatory: bool,
    pub download_url: String,
    pub release_page_url: Option<String>,
    pub sha256: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateDownloadState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    ReadyToRestart,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    CheckFinished {
        available: Option<AvailableUpdate>,
        manual: bool,
    },
    CheckFailed {
        error: String,
        manual: bool,
    },
    DownloadProgress {
        downloaded: u64,
        total: Option<u64>,
    },
    DownloadReadyToApply,
    DownloadFailed(String),
}

/// Compara strings semver (`1.2.3`, `v1.2.3`, `1.2.0-beta`).
pub fn compare_semver(a: &str, b: &str) -> Ordering {
    parse_semver_tuple(a).cmp(&parse_semver_tuple(b))
}

fn parse_semver_tuple(raw: &str) -> (u64, u64, u64) {
    let clean = raw
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("");
    let mut parts = clean.split('.');
    let major = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    (major, minor, patch)
}

pub fn parse_update_manifest_json(raw: &str) -> Result<UpdateManifest, String> {
    let manifest: UpdateManifest =
        serde_json::from_str(raw).map_err(|e| format!("manifesto de atualizacao invalido: {e}"))?;
    if manifest.version.trim().is_empty() {
        return Err("manifesto sem versao".to_string());
    }
    Ok(manifest)
}

/// Avalia se ha atualizacao mais recente e se ela e obrigatoria (`mandatory` ou `current < min_version`).
pub fn evaluate_update(manifest: &UpdateManifest, current_version: &str) -> Option<AvailableUpdate> {
    if compare_semver(&manifest.version, current_version) != Ordering::Greater {
        return None;
    }

    let below_min = manifest
        .min_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some_and(|min| compare_semver(current_version, min) == Ordering::Less);

    let is_mandatory = manifest.mandatory || below_min;
    let (raw_url, raw_sha) = pick_build_artifact(manifest, is_mods_build());
    let download_url = raw_url.trim().to_string();
    let release_page_url = manifest
        .release_page_url
        .as_deref()
        .map(str::trim)
        .filter(|s| is_safe_http_url(s))
        .map(|s| s.to_string());

    if !is_safe_http_url(&download_url) && release_page_url.is_none() {
        return None;
    }

    Some(AvailableUpdate {
        version: manifest.version.trim().trim_start_matches(['v', 'V']).to_string(),
        current_version: current_version.to_string(),
        is_mandatory,
        download_url,
        release_page_url,
        sha256: raw_sha
            .map(str::trim)
            .filter(|s| s.len() == 64)
            .map(|s| s.to_ascii_lowercase()),
        notes: manifest
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    })
}

/// Este executavel foi compilado com alguma modificacao (o "mods" do instalador)?
pub const fn is_mods_build() -> bool {
    cfg!(any(
        feature = "mod_ferramentas",
        feature = "mod_permissoes_chamada",
        feature = "mod_perfil"
    ))
}

/// Qual arquivo baixar: o build com modificacoes usa `modsDownloadUrl` quando a API publicou
/// um; senao (ou no nucleo) o `downloadUrl` de sempre.
fn pick_build_artifact(manifest: &UpdateManifest, mods_build: bool) -> (&str, Option<&str>) {
    if mods_build {
        if let Some(url) = manifest
            .mods_download_url
            .as_deref()
            .filter(|u| is_safe_http_url(u.trim()))
        {
            return (url, manifest.mods_sha256.as_deref());
        }
    }
    (manifest.download_url.as_str(), manifest.sha256.as_deref())
}

/// Consulta o manifesto remoto usando `If-None-Match` (`ETag`) para evitar trafego desnecessario.
pub fn fetch_update_manifest(
    agent: &ureq::Agent,
    url: &str,
    etag: Option<&str>,
) -> Result<Option<(UpdateManifest, Option<String>)>, String> {
    if !is_safe_http_url(url) {
        return Err("URL do manifesto invalida".to_string());
    }
    // O GitHub exige User-Agent; o Accept fixa o formato da API.
    let mut req = agent
        .get(url)
        .set("User-Agent", "GoLive-Lite-Updater")
        .set("Accept", "application/vnd.github+json");
    if let Some(tag) = etag.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.set("If-None-Match", tag);
    }

    match req.call() {
        Ok(resp) => {
            if resp.status() == 304 {
                return Ok(None);
            }
            let new_etag = resp.header("ETag").map(|s| s.to_string());
            let body = resp
                .into_string()
                .map_err(|e| format!("falha ao ler manifesto: {e}"))?;
            let manifest = if url.contains("api.github.com") {
                manifest_from_github_release(&body, |u| fetch_small_text(agent, u))?
            } else {
                parse_update_manifest_json(&body)?
            };
            Ok(Some((manifest, new_etag)))
        }
        Err(ureq::Error::Status(304, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {}", code)),
        Err(e) => Err(format!("erro de rede ao checar atualizacao: {e}")),
    }
}

/// Inicia download direto do `.exe` em background, valida PE/SHA-256 e prepara auto-substituicao;
/// se `download_url` for pagina web, abre no navegador do usuario.
pub fn start_update_download_and_apply(
    update: AvailableUpdate,
    tx: mpsc::UnboundedSender<UpdateEvent>,
    ctx: egui::Context,
) {
    if !is_direct_exe_url(&update.download_url) {
        let target = update
            .release_page_url
            .as_deref()
            .filter(|u| is_safe_http_url(u))
            .unwrap_or(&update.download_url);
        match open_external_url(target) {
            Ok(()) => {
                if !update.is_mandatory {
                    let _ = tx.send(UpdateEvent::DownloadFailed(
                        "Pagina de download aberta no navegador.".to_string(),
                    ));
                }
            }
            Err(e) => {
                let _ = tx.send(UpdateEvent::DownloadFailed(e));
            }
        }
        ctx.request_repaint();
        return;
    }

    std::thread::Builder::new()
        .name("golive-updater".into())
        .spawn(move || {
            let res = perform_exe_download_and_stage(&update, &tx, &ctx);
            match res {
                Ok(()) => {
                    let _ = tx.send(UpdateEvent::DownloadReadyToApply);
                }
                Err(err) => {
                    let _ = tx.send(UpdateEvent::DownloadFailed(err));
                }
            }
            ctx.request_repaint();
        })
        .ok();
}

fn is_direct_exe_url(url: &str) -> bool {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://")
        && !trimmed.starts_with("http://127.0.0.1")
        && !trimmed.starts_with("http://localhost")
    {
        return false;
    }
    let path_only = trimmed.split(['?', '#']).next().unwrap_or("");
    path_only.to_ascii_lowercase().ends_with(".exe")
}

fn perform_exe_download_and_stage(
    update: &AvailableUpdate,
    tx: &mpsc::UnboundedSender<UpdateEvent>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let cur_exe = std::env::current_exe()
        .map_err(|e| format!("nao foi possivel localizar o executavel atual: {e}"))?;
    let exe_dir = cur_exe
        .parent()
        .ok_or_else(|| "diretorio do executavel invalido".to_string())?;
    let staged_exe = exe_dir.join("golive-lite.update.exe");

    let agent = build_http_agent(Duration::from_secs(20))?;
    let resp = agent
        .get(&update.download_url)
        .call()
        .map_err(|e| format!("falha ao baixar atualizacao: {e}"))?;

    let total = resp
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok());

    let mut file = File::create(&staged_exe).map_err(|e| {
        format!(
            "sem permissao para gravar em {} ({}). Abra a pagina de download.",
            exe_dir.display(),
            e
        )
    })?;

    let mut reader = resp.into_reader().take(200 * 1024 * 1024);
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut hasher = Sha256State::new();
    let mut header_check = [0u8; 2];
    let mut header_len = 0usize;
    let mut last_ui = Instant::now();

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("erro durante download: {e}"))?;
        if n == 0 {
            break;
        }
        if header_len < 2 {
            let take = (2 - header_len).min(n);
            header_check[header_len..header_len + take].copy_from_slice(&buf[..take]);
            header_len += take;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("erro ao gravar arquivo temporario: {e}"))?;
        downloaded += n as u64;

        if last_ui.elapsed() >= Duration::from_millis(120) {
            let _ = tx.send(UpdateEvent::DownloadProgress { downloaded, total });
            ctx.request_repaint();
            last_ui = Instant::now();
        }
    }
    file.flush()
        .map_err(|e| format!("erro ao finalizar arquivo de atualizacao: {e}"))?;
    drop(file);

    if downloaded < 256 * 1024 || header_len < 2 || &header_check != b"MZ" {
        let _ = std::fs::remove_file(&staged_exe);
        return Err("arquivo baixado nao e um executavel Windows valido".to_string());
    }

    if let Some(ref expected) = update.sha256 {
        let actual = hasher.finalize_hex();
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&staged_exe);
            return Err("checksum SHA-256 da atualizacao nao confere".to_string());
        }
    }

    let _ = tx.send(UpdateEvent::DownloadProgress {
        downloaded,
        total: Some(downloaded),
    });

    spawn_windows_replace_script(cur_exe, staged_exe)?;
    Ok(())
}

fn spawn_windows_replace_script(cur_exe: PathBuf, staged_exe: PathBuf) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let script_path = std::env::temp_dir().join("golive-lite-apply.cmd");
        let pid = std::process::id();
        let src_s = staged_exe.display().to_string();
        let dst_s = cur_exe.display().to_string();
        let cmd_body = format!(
            "@echo off\r\nsetlocal\r\n:waitloop\r\ntasklist /FI \"PID eq {pid}\" 2>NUL | find /I \"{pid}\" >NUL\r\nif not errorlevel 1 (\r\n  timeout /t 1 /nobreak >NUL\r\n  goto waitloop\r\n)\r\nmove /Y \"{src_s}\" \"{dst_s}\" >NUL\r\nif not errorlevel 1 (\r\n  start \"\" \"{dst_s}\"\r\n)\r\ndel \"%~f0\" >NUL 2>&1\r\n"
        );
        std::fs::write(&script_path, cmd_body)
            .map_err(|e| format!("falha ao gerar script de substituicao: {e}"))?;

        std::process::Command::new("cmd.exe")
            .args(["/C", &script_path.display().to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("falha ao executar script de atualizacao: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (cur_exe, staged_exe);
        Err("auto-substituicao suportada apenas no Windows".to_string())
    }
}

/// Implementacao minima e pura de SHA-256 (FIPS 180-4) sem dependencias externas.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256State::new();
    h.update(data);
    h.finalize_hex()
}

struct Sha256State {
    state: [u32; 8],
    block: [u8; 64],
    block_len: usize,
    total_bits: u64,
}

impl Sha256State {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                0x1f83d9ab, 0x5be0cd19,
            ],
            block: [0u8; 64],
            block_len: 0,
            total_bits: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);
        let mut offset = 0;
        while offset < data.len() {
            let space = 64 - self.block_len;
            let take = space.min(data.len() - offset);
            self.block[self.block_len..self.block_len + take]
                .copy_from_slice(&data[offset..offset + take]);
            self.block_len += take;
            offset += take;
            if self.block_len == 64 {
                let blk = self.block;
                self.compress(&blk);
                self.block_len = 0;
            }
        }
    }

    fn finalize_hex(mut self) -> String {
        self.block[self.block_len] = 0x80;
        self.block_len += 1;
        if self.block_len > 56 {
            for b in &mut self.block[self.block_len..64] {
                *b = 0;
            }
            let blk = self.block;
            self.compress(&blk);
            self.block_len = 0;
        }
        for b in &mut self.block[self.block_len..56] {
            *b = 0;
        }
        self.block[56..64].copy_from_slice(&self.total_bits.to_be_bytes());
        let blk = self.block;
        self.compress(&blk);

        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{:08x}", word));
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        let vals = [a, b, c, d, e, f, g, h];
        for (dst, v) in self.state.iter_mut().zip(vals) {
            *dst = dst.wrapping_add(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_semver_and_mandatory_evaluation() {
        assert_eq!(compare_semver("0.2.0", "0.1.0"), Ordering::Greater);
        assert_eq!(compare_semver("v1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_semver("0.1.5", "0.2.0"), Ordering::Less);

        let opt_manifest = UpdateManifest {
            version: "0.2.0".into(),
            min_version: Some("0.1.0".into()),
            mandatory: false,
            download_url: "https://golive.nemtudo.me/downloads/golive-lite.exe".into(),
            release_page_url: None,
            sha256: None,
            mods_download_url: None,
            mods_sha256: None,
            notes: Some("Melhorias de desempenho".into()),
            published_at: None,
        };
        let opt_upd = evaluate_update(&opt_manifest, "0.1.0").expect("should offer optional update");
        assert!(!opt_upd.is_mandatory);

        let min_forced = UpdateManifest {
            version: "0.3.0".into(),
            min_version: Some("0.2.0".into()),
            mandatory: false,
            download_url: "https://golive.nemtudo.me/downloads/golive-lite.exe".into(),
            release_page_url: None,
            sha256: None,
            mods_download_url: None,
            mods_sha256: None,
            notes: None,
            published_at: None,
        };
        let forced_upd =
            evaluate_update(&min_forced, "0.1.5").expect("min_version > current forces update");
        assert!(forced_upd.is_mandatory);

        assert!(evaluate_update(&min_forced, "0.3.0").is_none());
    }

    #[test]
    fn test_pick_build_artifact_keeps_core_and_mods_apart() {
        let raw = r#"{"version":"1.0.0","downloadUrl":"https://cdn/core.exe","sha256":"aa",
            "modsDownloadUrl":"https://cdn/mods.exe","modsSha256":"bb"}"#;
        let m = parse_update_manifest_json(raw).expect("manifesto");
        assert_eq!(pick_build_artifact(&m, false), ("https://cdn/core.exe", Some("aa")));
        assert_eq!(pick_build_artifact(&m, true), ("https://cdn/mods.exe", Some("bb")));
        // Sem build de mods publicado, o build com mods cai no arquivo normal.
        let only_core = parse_update_manifest_json(r#"{"version":"1.0.0","downloadUrl":"https://cdn/core.exe"}"#)
            .expect("manifesto");
        assert_eq!(pick_build_artifact(&only_core, true).0, "https://cdn/core.exe");
    }

    #[test]
    fn test_parse_update_manifest_json_valid_and_optional_fields() {
        let raw = r#"{
            "version": "1.2.0",
            "minVersion": "1.0.0",
            "mandatory": true,
            "downloadUrl": "https://golive.nemtudo.me/lite.exe",
            "notes": "Correcao critica de rede"
        }"#;
        let m = parse_update_manifest_json(raw).expect("manifest json valid");
        assert_eq!(m.version, "1.2.0");
        assert_eq!(m.min_version.as_deref(), Some("1.0.0"));
        assert!(m.mandatory);
        assert_eq!(m.notes.as_deref(), Some("Correcao critica de rede"));
    }

    #[test]
    fn test_sha256_hex_matches_standard_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

// --- Releases do GitHub ----------------------------------------------------------------------
//
// Cada versao e uma release com tag `vX.Y.Z` (criada pelo GitHub Actions, `.github/workflows`).
// Arquivos da release que o app usa:
//   - `golive-lite-core.exe`  (sem modificacoes)
//   - `golive-lite-mods.exe`  (com as modificacoes compiladas)
//   - `SHA256SUMS.txt`        (hash de cada arquivo; o download so e aceito se bater)
// No texto da release:
//   - uma linha `obrigatoria: sim` torna a atualizacao obrigatoria para todos;
//   - uma linha `versao-minima: X.Y.Z` obriga so quem esta abaixo dela;
//   - o resto vira as novidades mostradas no app.

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

pub const ASSET_CORE: &str = "golive-lite-core.exe";
pub const ASSET_MODS: &str = "golive-lite-mods.exe";
pub const ASSET_SUMS: &str = "SHA256SUMS.txt";

fn fetch_small_text(agent: &ureq::Agent, url: &str) -> Option<String> {
    if !is_safe_http_url(url) {
        return None;
    }
    let resp = agent.get(url).set("User-Agent", "GoLive-Lite-Updater").call().ok()?;
    let mut buf = String::new();
    resp.into_reader().take(64 * 1024).read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// `hash  nome` (formato do sha256sum) -> hash do arquivo pedido.
pub fn sha_from_sums(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name.eq_ignore_ascii_case(file) && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// Transforma a release do GitHub no manifesto que o resto do atualizador ja entende.
pub fn manifest_from_github_release(
    raw: &str,
    fetch_text: impl Fn(&str) -> Option<String>,
) -> Result<UpdateManifest, String> {
    let rel: GhRelease = serde_json::from_str(raw).map_err(|e| format!("release do GitHub invalida: {e}"))?;
    if rel.draft || rel.prerelease {
        return Err("a ultima release ainda nao foi publicada".to_string());
    }
    let version = rel.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    if version.is_empty() {
        return Err("release sem versao".to_string());
    }
    let url_of = |name: &str| rel.assets.iter().find(|a| a.name.eq_ignore_ascii_case(name)).map(|a| a.browser_download_url.clone());
    let core = url_of(ASSET_CORE).ok_or("release sem golive-lite-core.exe")?;
    let sums = url_of(ASSET_SUMS).and_then(|u| fetch_text(&u)).unwrap_or_default();

    let body = rel.body.unwrap_or_default();
    let mut mandatory = false;
    let mut min_version = None;
    let mut notes = Vec::new();
    for line in body.lines() {
        let l = line.trim().to_lowercase();
        let value = |prefixes: &[&str]| prefixes.iter().find_map(|p| l.strip_prefix(p).map(|v| v.trim().to_string()));
        if let Some(v) = value(&["obrigatoria:", "obrigatória:"]) {
            mandatory = matches!(v.as_str(), "sim" | "s" | "true" | "yes");
        } else if let Some(v) = value(&["versao-minima:", "versão-mínima:", "versao minima:"]) {
            if !v.is_empty() {
                min_version = Some(v.trim_start_matches('v').to_string());
            }
        } else {
            notes.push(line);
        }
    }
    let notes = notes.join("\n").trim().to_string();
    Ok(UpdateManifest {
        version,
        min_version,
        mandatory,
        sha256: sha_from_sums(&sums, ASSET_CORE),
        mods_download_url: url_of(ASSET_MODS),
        mods_sha256: sha_from_sums(&sums, ASSET_MODS),
        download_url: core,
        release_page_url: rel.html_url,
        notes: (!notes.is_empty()).then_some(notes),
        published_at: rel.published_at,
    })
}

#[cfg(test)]
mod github_tests {
    use super::*;

    #[test]
    fn reads_a_github_release() {
        let raw = r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false,"html_url":"https://github.com/a/b/releases/tag/v0.2.0",
            "body":"Novidades legais\nobrigatoria: sim\nversao-minima: 0.1.5",
            "assets":[
              {"name":"golive-lite-core.exe","browser_download_url":"https://github.com/a/b/releases/download/v0.2.0/golive-lite-core.exe"},
              {"name":"golive-lite-mods.exe","browser_download_url":"https://github.com/a/b/releases/download/v0.2.0/golive-lite-mods.exe"},
              {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/a/b/releases/download/v0.2.0/SHA256SUMS.txt"}]}"#;
        let sums = format!("{}  golive-lite-core.exe\n{}  golive-lite-mods.exe\n", "a".repeat(64), "b".repeat(64));
        let m = manifest_from_github_release(raw, |_| Some(sums.clone())).unwrap();
        assert_eq!(m.version, "0.2.0");
        assert!(m.mandatory);
        assert_eq!(m.min_version.as_deref(), Some("0.1.5"));
        assert_eq!(m.sha256.as_deref(), Some("a".repeat(64).as_str()));
        assert_eq!(m.mods_sha256.as_deref(), Some("b".repeat(64).as_str()));
        assert!(m.mods_download_url.unwrap().ends_with("golive-lite-mods.exe"));
        assert_eq!(m.notes.as_deref(), Some("Novidades legais"));
        assert_eq!(github_latest_release_url("a/b").as_deref(), Some("https://api.github.com/repos/a/b/releases/latest"));
        assert!(github_latest_release_url("a/b/c").is_none());
    }
}
