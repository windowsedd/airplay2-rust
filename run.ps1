# airplay2-rust - Windows run helper
#
# Default: GStreamer + dump + ffplay fallback
#   .\run.ps1
#   cargo run   (also uses scripts\gst-runner.cmd so DLLs are found)
#
# Without GStreamer:
#   .\run.ps1 -NoGStreamer
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

function Add-PathFront([string]$dir) {
    if ($dir -and (Test-Path $dir)) {
        $env:Path = "$dir;" + $env:Path
    }
}

# FFmpeg (real tools first, then Chocolatey bin)
Add-PathFront "C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin"
Add-PathFront "C:\ProgramData\chocolatey\bin"

$gstRoot = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
if (-not $gstRoot) {
    $gstRoot = "C:\Program Files\gstreamer\1.0\msvc_x86_64"
}

$features = @("h264-dump", "ffmpeg")
$useGst = -not $NoGStreamer

if ($useGst) {
    if (-not (Test-Path $gstRoot)) {
        Write-Host "GStreamer not found at '$gstRoot' - building without gstreamer." -ForegroundColor Yellow
        $useGst = $false
    } else {
        $gstBin = Join-Path $gstRoot "bin"
        Add-PathFront $gstBin
        $env:PKG_CONFIG_PATH = Join-Path $gstRoot "lib\pkgconfig"
        $env:GST_PLUGIN_PATH = Join-Path $gstRoot "lib\gstreamer-1.0"
        $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gstRoot
        $features += "gstreamer"
        Write-Host "GStreamer (runtime + build): $gstRoot" -ForegroundColor Cyan
        Write-Host "  PATH includes: $gstBin" -ForegroundColor DarkGray
        if (-not (Test-Path (Join-Path $gstBin "pkg-config.exe"))) {
            Write-Warning "pkg-config.exe missing under GStreamer bin - compile may fail."
        }
        # Spot-check a core DLL so we fail early with a clear message
        $dll = Get-ChildItem $gstBin -Filter "gstreamer-1.0*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $dll) {
            Write-Warning "No gstreamer-1.0*.dll in $gstBin - install GStreamer MSVC runtime."
        } else {
            Write-Host "  Found: $($dll.Name)" -ForegroundColor DarkGray
        }
    }
}

$feat = ($features -join ",")
Write-Host "Features: $feat" -ForegroundColor Green

if ($BuildOnly) {
    cargo build -p airplay-app --features $feat
    exit $LASTEXITCODE
}

# cargo run uses gst-runner.cmd for DLL PATH; we already set PATH for this shell too.
if (-not (Test-Path $Config)) {
    Write-Host "No $Config - using cargo defaults (player=auto)." -ForegroundColor Yellow
    cargo run -p airplay-app --features $feat
} else {
    cargo run -p airplay-app --features $feat -- --config $Config
}
exit $LASTEXITCODE
