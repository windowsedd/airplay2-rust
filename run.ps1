# airplay2-rust — Windows run helper
#
# Default (no GStreamer build tools):
#   .\run.ps1
#   → cargo run with ffplay window + dump.h264
#
# With GStreamer live window (needs GStreamer MSVC + its pkg-config):
#   .\run.ps1 -GStreamer
#
# Options:
#   -GStreamer   build/run with --features gstreamer
#   -BuildOnly   cargo build only
#   -Config path  config.toml path

param(
    [switch]$GStreamer,
    [switch]$BuildOnly,
    [string]$Config = "config.toml"
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# FFmpeg / ffplay (Chocolatey or other)
$chocoBin = "C:\ProgramData\chocolatey\bin"
if (Test-Path $chocoBin) {
    $env:Path = "$chocoBin;" + $env:Path
}

$gstRoot = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
if (-not $gstRoot) {
    $gstRoot = "C:\Program Files\gstreamer\1.0\msvc_x86_64"
}

$features = @("h264-dump", "ffmpeg")
if ($GStreamer) {
    if (-not (Test-Path $gstRoot)) {
        Write-Error "GStreamer not found at '$gstRoot'. Install MSVC x86_64 runtime+dev from https://gstreamer.freedesktop.org/download/ or set GSTREAMER_1_0_ROOT_MSVC_X86_64."
    }
    $gstBin = Join-Path $gstRoot "bin"
    $gstPc = Join-Path $gstRoot "lib\pkgconfig"
    # pkg-config.exe ships inside GStreamer bin on Windows
    $env:Path = "$gstBin;" + $env:Path
    $env:PKG_CONFIG_PATH = $gstPc
    $env:GST_PLUGIN_PATH = Join-Path $gstRoot "lib\gstreamer-1.0"
    $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gstRoot
    $features += "gstreamer"
    Write-Host "GStreamer enabled: $gstRoot" -ForegroundColor Cyan
    Write-Host "  PATH += $gstBin (includes pkg-config.exe)" -ForegroundColor DarkGray
    Write-Host "  PKG_CONFIG_PATH = $gstPc" -ForegroundColor DarkGray

    $pkg = Get-Command pkg-config -ErrorAction SilentlyContinue
    if (-not $pkg) {
        Write-Error "pkg-config.exe still not on PATH after adding GStreamer bin. Check $gstBin\pkg-config.exe exists."
    }
    Write-Host "  pkg-config: $($pkg.Source)" -ForegroundColor DarkGray
}

$feat = ($features -join ",")
Write-Host "Features: $feat" -ForegroundColor Green

if ($BuildOnly) {
    cargo build -p airplay-app --features $feat
    exit $LASTEXITCODE
}

if (-not (Test-Path $Config)) {
    Write-Host "No $Config — cargo run will create defaults (player=auto)." -ForegroundColor Yellow
    cargo run -p airplay-app --features $feat
} else {
    cargo run -p airplay-app --features $feat -- --config $Config
}
