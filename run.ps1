# airplay2-rust - Windows run helper
#
# Default: GStreamer + dump + ffplay (same as plain cargo run when GST is installed)
#   .\run.ps1
#   cargo run
#
# Without GStreamer (ffplay + dump only):
#   .\run.ps1 -NoGStreamer
#
# Release .exe:
#   .\build-release.ps1
#
# Options:
#   -NoGStreamer  omit gstreamer feature
#   -BuildOnly    cargo build only
#   -Config path  config.toml path

param(
    [switch]$NoGStreamer,
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
$realFfplay = "C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin"
if (Test-Path $realFfplay) {
    $env:Path = "$realFfplay;" + $env:Path
}

$gstRoot = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
if (-not $gstRoot) {
    $gstRoot = "C:\Program Files\gstreamer\1.0\msvc_x86_64"
}

$features = @("h264-dump", "ffmpeg")
$useGst = -not $NoGStreamer

if ($useGst) {
    if (-not (Test-Path $gstRoot)) {
        Write-Host "GStreamer not found at '$gstRoot' - building without gstreamer feature." -ForegroundColor Yellow
        Write-Host "Install MSVC x86_64 from https://gstreamer.freedesktop.org/download/" -ForegroundColor Yellow
        $useGst = $false
    } else {
        $gstBin = Join-Path $gstRoot "bin"
        $gstPc = Join-Path $gstRoot "lib\pkgconfig"
        $env:Path = "$gstBin;" + $env:Path
        $env:PKG_CONFIG_PATH = $gstPc
        $env:GST_PLUGIN_PATH = (Join-Path $gstRoot "lib\gstreamer-1.0")
        $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gstRoot
        $features += "gstreamer"
        Write-Host "GStreamer enabled (default): $gstRoot" -ForegroundColor Cyan
        $pkg = Get-Command pkg-config -ErrorAction SilentlyContinue
        if (-not $pkg) {
            Write-Error "pkg-config.exe not on PATH. Expected at $gstBin\pkg-config.exe"
        }
        Write-Host "  pkg-config: $($pkg.Source)" -ForegroundColor DarkGray
    }
}

$feat = ($features -join ",")
Write-Host "Features: $feat" -ForegroundColor Green

if ($BuildOnly) {
    cargo build -p airplay-app --features $feat
    exit $LASTEXITCODE
}

if (-not (Test-Path $Config)) {
    Write-Host "No $Config - cargo run will create defaults (player=auto)." -ForegroundColor Yellow
    cargo run -p airplay-app --features $feat
} else {
    cargo run -p airplay-app --features $feat -- --config $Config
}
