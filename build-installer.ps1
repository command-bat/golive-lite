$ErrorActionPreference = "Stop"

$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# Repositorio do GitHub das releases (o app e o setup buscam atualizacoes nele). No GitHub
# Actions ja vem definido; localmente sai do `git remote origin`.
if (-not $env:GOLIVE_LITE_GITHUB_REPO) {
    $origin = ""
    try { $origin = (& git remote get-url origin 2>$null) } catch { $origin = "" }
    if ($origin -match "github\.com[:/]([^/]+/[^/]+?)(\.git)?$") { $env:GOLIVE_LITE_GITHUB_REPO = $Matches[1] }
}
if ($env:GOLIVE_LITE_GITHUB_REPO) {
    Write-Host "==> Repositorio de atualizacoes: $env:GOLIVE_LITE_GITHUB_REPO"
} else {
    Write-Host "==> Sem repositorio do GitHub: este build nao vai buscar atualizacoes."
}
# --offline so na maquina local (o cache ja tem tudo); no Actions baixa as dependencias.
$offline = @(); if (-not $env:CI) { $offline = @("--offline") }

Write-Host "==> Encerrando instancias em execucao..."
Stop-Process -Name golive-lite,golive-lite-setup,unins000 -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

Write-Host "==> 1/4 Compilando golive-lite CORE Ultra-Leve (--no-default-features)..."
cargo build --release @offline --bin golive-lite --no-default-features
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
$coreExe = Join-Path (Get-Location) "target\release\golive-lite-core.exe"
Copy-Item -Path (Resolve-Path "target\release\golive-lite.exe").Path -Destination $coreExe -Force

Write-Host "==> 2/4 Compilando golive-lite COM MODIFICACOES (--all-features)..."
cargo build --release @offline --bin golive-lite --all-features
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
$modsExe = Join-Path (Get-Location) "target\release\golive-lite-mods.exe"
Copy-Item -Path (Resolve-Path "target\release\golive-lite.exe").Path -Destination $modsExe -Force

Write-Host "==> 3/4 Compilando desinstalador compacto (GOLIVE_LITE_UNINS_ONLY=1)..."
$env:GOLIVE_LITE_UNINS_ONLY = "1"
Remove-Item Env:GOLIVE_LITE_CORE_EXE -ErrorAction SilentlyContinue
Remove-Item Env:GOLIVE_LITE_MODS_EXE -ErrorAction SilentlyContinue
Remove-Item Env:GOLIVE_LITE_UNINS_EXE -ErrorAction SilentlyContinue
Remove-Item Env:GOLIVE_LITE_PAYLOAD_EXE -ErrorAction SilentlyContinue
cargo build --release @offline -p golive-lite-setup
if ($LASTEXITCODE -ne 0) {
    Remove-Item Env:GOLIVE_LITE_UNINS_ONLY -ErrorAction SilentlyContinue
    exit $LASTEXITCODE
}
Remove-Item Env:GOLIVE_LITE_UNINS_ONLY -ErrorAction SilentlyContinue
$uninsExe = Join-Path (Get-Location) "target\release\golive-lite-unins.exe"
Copy-Item -Path (Resolve-Path "target\release\golive-lite-setup.exe").Path -Destination $uninsExe -Force

Write-Host "==> 4/4 Compilando instalador nativo modular golive-lite-setup..."
$env:GOLIVE_LITE_CORE_EXE = $coreExe
$env:GOLIVE_LITE_MODS_EXE = $modsExe
$env:GOLIVE_LITE_UNINS_EXE = $uninsExe
cargo build --release @offline -p golive-lite-setup
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$appVersion = [regex]::Match((Get-Content "Cargo.toml" -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if ([string]::IsNullOrWhiteSpace($appVersion)) {
    throw "Nao foi possivel extrair a versao de Cargo.toml"
}

$distDir = Join-Path (Get-Location) "dist"
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir | Out-Null
}

$legacySetup = Join-Path $distDir "GoLiveLite-Setup.exe"
if (Test-Path $legacySetup) {
    Remove-Item -Path $legacySetup -Force -ErrorAction SilentlyContinue
}

$setupSrc = Join-Path (Get-Location) "target\release\golive-lite-setup.exe"
$setupDst = Join-Path $distDir "Go-Live-Lite-Setup-$appVersion.exe"
Copy-Item -Path $setupSrc -Destination $setupDst -Force

$coreMb = [math]::Round((Get-Item $coreExe).Length / 1MB, 2)
$modsMb = [math]::Round((Get-Item $modsExe).Length / 1MB, 2)
$uninsKb = [math]::Round((Get-Item $uninsExe).Length / 1KB, 1)
$setupMb = [math]::Round((Get-Item $setupDst).Length / 1MB, 2)
Write-Host ""
Write-Host "Build modular concluido:"
Write-Host "  Core (Ultra-Leve): $coreExe ($coreMb MB)"
Write-Host "  Mods (Completo):   $modsExe ($modsMb MB)"
Write-Host "  Desinstalador:     $uninsExe ($uninsKb KB)"
Write-Host "  Instalador:        $setupDst ($setupMb MB)"

# Pasta pronta para a release do GitHub: os arquivos que o app e o setup procuram por nome.
$releaseDir = Join-Path $distDir "release"
if (Test-Path $releaseDir) { Remove-Item -Recurse -Force $releaseDir }
New-Item -ItemType Directory -Path $releaseDir | Out-Null
Copy-Item $coreExe (Join-Path $releaseDir "golive-lite-core.exe")
Copy-Item $modsExe (Join-Path $releaseDir "golive-lite-mods.exe")
Copy-Item $setupDst (Join-Path $releaseDir "Go-Live-Lite-Setup.exe")
Copy-Item (Join-Path (Get-Location) "modifications.json") (Join-Path $releaseDir "modifications.json")
$sums = Get-ChildItem $releaseDir -File | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower(), $_.Name
}
[System.IO.File]::WriteAllLines((Join-Path $releaseDir "SHA256SUMS.txt"), $sums)
Write-Host "  Release:           $releaseDir (core, mods, setup, modifications.json, SHA256SUMS.txt)"
