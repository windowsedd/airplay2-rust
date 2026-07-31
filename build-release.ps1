# Build release airplay-app.exe with default features (player=auto).
# Output: dist\airplay-app.exe + dist\config.toml
#
# Usage:
#   .\build-release.ps1
#   .\build-release.ps1 -GStreamer   # also enable gstreamer feature

param([switch]$GStreamer)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$chocoBin = "C:\ProgramData\chocolatey\bin"
if (Test-Path $chocoBin) { $env:Path = "$chocoBin;" + $env:Path }

$features = @("h264-dump", "ffmpeg")
if ($GStreamer) {
    $gstRoot = if ($env:GSTREAMER_1_0_ROOT_MSVC_X86_64) {
        $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
    } else {
        "C:\Program Files\gstreamer\1.0\msvc_x86_64"
    }
    $env:Path = "$(Join-Path $gstRoot 'bin');" + $env:Path
    $env:PKG_CONFIG_PATH = Join-Path $gstRoot "lib\pkgconfig"
    $env:GST_PLUGIN_PATH = Join-Path $gstRoot "lib\gstreamer-1.0"
    $features += "gstreamer"
}

$feat = ($features -join ",")
Write-Host "Building release with features: $feat" -ForegroundColor Green
cargo build -p airplay-app --release --features $feat
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$dist = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item "target\release\airplay-app.exe" (Join-Path $dist "airplay-app.exe") -Force

# Tray icon (optional; binary has a fallback icon if missing)
$assetsDist = Join-Path $dist "assets"
New-Item -ItemType Directory -Force -Path $assetsDist | Out-Null
if (Test-Path "assets\tray-icon.png") {
    Copy-Item "assets\tray-icon.png" (Join-Path $assetsDist "tray-icon.png") -Force
}
if (Test-Path "assets\logo.png") {
    Copy-Item "assets\logo.png" (Join-Path $assetsDist "logo.png") -Force
}

$configSrc = Join-Path $PSScriptRoot "config.toml"
$configDst = Join-Path $dist "config.toml"
if (Test-Path $configSrc) {
    Copy-Item $configSrc $configDst -Force
} else {
    @"
[airplay]
server_name = "airplay2-rust"
width = 1280
height = 720
fps = 30
refresh_rate = 60

[player]
implementation = "auto"
output = "dump.h264"
"@ | Set-Content -Encoding utf8 $configDst
}

Write-Host ""
Write-Host "Done. Run:" -ForegroundColor Cyan
Write-Host "  dist\airplay-app.exe"
Write-Host "  (needs ffplay on PATH for the live window)"
Write-Host "  player.implementation = auto in dist\config.toml"
