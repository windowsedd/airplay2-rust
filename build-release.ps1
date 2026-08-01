# Build release airplay-app.exe with default features (GStreamer + dump + ffplay).
# Output: dist\airplay-app.exe + dist\config.toml
#
# Usage:
#   .\build-release.ps1
#   .\build-release.ps1 -NoGStreamer

param([switch]$NoGStreamer)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$chocoBin = "C:\ProgramData\chocolatey\bin"
if (Test-Path $chocoBin) { $env:Path = "$chocoBin;" + $env:Path }
$realFfplay = "C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin"
if (Test-Path $realFfplay) { $env:Path = "$realFfplay;" + $env:Path }

$features = @("h264-dump", "ffmpeg")
if (-not $NoGStreamer) {
    $gstRoot = if ($env:GSTREAMER_1_0_ROOT_MSVC_X86_64) {
        $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
    } else {
        "C:\Program Files\gstreamer\1.0\msvc_x86_64"
    }
    if (Test-Path $gstRoot) {
        $env:Path = "$(Join-Path $gstRoot 'bin');" + $env:Path
        $env:PKG_CONFIG_PATH = Join-Path $gstRoot "lib\pkgconfig"
        $env:GST_PLUGIN_PATH = Join-Path $gstRoot "lib\gstreamer-1.0"
        $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gstRoot
        $features += "gstreamer"
        Write-Host "GStreamer: $gstRoot" -ForegroundColor Cyan
    } else {
        Write-Host "GStreamer not found; building without gstreamer feature." -ForegroundColor Yellow
    }
}

$feat = ($features -join ",")
Write-Host "Building release with features: $feat" -ForegroundColor Green
cargo build -p airplay-app --release --features $feat
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$dist = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item "target\release\airplay-app.exe" (Join-Path $dist "airplay-app.exe") -Force

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
width = 1920
height = 1080
fps = 60
refresh_rate = 60

[player]
implementation = "auto"
output = "dump.h264"
"@ | Set-Content -Encoding utf8 $configDst
}

Write-Host ""
Write-Host "Done. Run:" -ForegroundColor Cyan
Write-Host "  dist\airplay-app.exe"
Write-Host "  (GStreamer runtime bin should be on PATH for live A/V)"
Write-Host "  player.implementation = auto (GStreamer primary)"
