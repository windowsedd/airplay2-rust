# airplay2-rust

**English** | [**繁體中文**](#播放器--players-繁體中文)

<p align="center">
  <img src="assets/logo.svg" alt="airplay2-rust logo" width="160" height="160" />
</p>

<p align="center">
  <img src="assets/logo-banner.svg" alt="airplay2-rust" width="640" />
</p>

Rust port of [java-airplay-2open](https://github.com) — an **AirPlay receiver** (screen mirror / media) with pairing, FairPlay setup, RTSP control, media decrypt, mDNS advertisement, multiple player backends, and a small sender client.

## Disclaimer

1. **Educational / research only.** This project exists to study the AirPlay protocol and cryptographic plumbing. Do **not** use it for commercial products or any illegal purpose.
2. **You** are responsible for compliance with local law and third-party terms of service.
3. Provided **"as is"** without warranty of any kind.
4. **AirPlay** is a trademark of **Apple Inc.** This project is **not** affiliated with, endorsed by, or sponsored by Apple.

## Architecture (crate map)

| Crate | Role |
|-------|------|
| [`airplay-lib`](crates/airplay-lib) | Core: pairing (Ed25519 / X25519 / AES-CTR), FairPlay `fp-setup`, OmgHax / HandGarble / SapHash, video & audio decrypt, RTSP setup helpers, Bonjour TXT helpers. **No TCP servers.** |
| [`airplay-server`](crates/airplay-server) | RTSP/HTTP control plane, sessions, video/audio media sockets, `AirPlayConsumer` trait, packet parse → decrypt → consumer. |
| [`airplay-player`](crates/airplay-player) | Player backends implementing the consumer: `h264-dump`, `gstreamer`, `ffmpeg`, `vlc` (Cargo features). |
| [`airplay-client`](crates/airplay-client) | Sender path: mDNS browse, RTSP control client (info / pair-setup / pair-verify), FairPlay video encryptor. |
| [`airplay-app`](crates/airplay-app) | Runnable receiver binary: TOML config → player → `AirPlayServer`. |

```
iOS/macOS device  ──mDNS──►  airplay-app (advertise)
                  ──RTSP──►  airplay-server (control + media)
                  ──RTP───►  decrypt (airplay-lib) → AirPlayConsumer → player
```

Design and task plan:

- [Design spec](docs/superpowers/specs/2026-08-01-airplay2-rust-design.md)
- [Implementation plan](docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md)
- [Acceptance checklist](docs/superpowers/plans/acceptance-checklist.md)

AI / coding agents:

- [AGENTS.md](./AGENTS.md) — shared agent instructions
- [CLAUDE.md](./CLAUDE.md) — Claude entry (links to AGENTS.md)

## Players

Select the backend with `player.implementation` in `config.toml` (must match a **Cargo feature** built into the binary). Default app features are **`h264-dump` + `ffmpeg` + `gstreamer`**. Use `implementation = "auto"` to tee **GStreamer (primary)** + dump + ffplay.

### GStreamer

- Supports **video and audio** streams (**ALAC** + **AAC-ELD**).
- Requires [GStreamer](https://gstreamer.freedesktop.org/download/) 1.x installed (plugins for `h264parse`, `avdec_h264`, `avdec_alac` / `avdec_aac`, `autovideosink`, etc.).
- On Windows, the Rust crates need **`pkg-config`** (ships in GStreamer’s `bin`) and `PKG_CONFIG_PATH` — easiest: `.\run.ps1 -GStreamer`.
- Config: `implementation = "gstreamer"` with `--features gstreamer`.

### FFmpeg

- **Video only** via **`ffplay`** (H.264 annex-B on stdin).
- **AAC-ELD** audio is not handled by this backend; full AAC-ELD in FFmpeg generally needs a build with e.g. `--enable-libfdk-aac` (and a custom player path — not the default `ffplay` mirror path).
- Requires **FFmpeg** installed with **`ffplay` on `PATH`**.
- Config: `implementation = "ffmpeg"` (default features already include `ffmpeg`).

### VLC

- Playback often **stops after a few seconds** (unstable stdin H.264 demux).
- Requires **VLC** installed (`vlc` / `cvlc` on `PATH`).
- Prefer GStreamer or FFmpeg for real use.
- Config: `implementation = "vlc"` with `--features vlc`.

### h264-dump

- Writes the video stream to a file (default **`dump.h264`**).
- No live window, no system media libraries required.
- Useful for protocol debugging: `ffplay -f h264 dump.h264`.
- Config: `implementation = "h264-dump"` (always available with default features).

| Backend | Video | Audio | Needs | Stability |
|---------|-------|-------|-------|-----------|
| **GStreamer** | Yes | ALAC + AAC-ELD | GStreamer 1.x (+ pkg-config to **build**) | Best for live A/V |
| **FFmpeg** | Yes (`ffplay`) | No (default path) | `ffplay` on `PATH` | Good for video window |
| **VLC** | Yes | No | VLC on `PATH` | Stops after a few seconds |
| **h264-dump** | File only | No | None | Stable dump for analysis |
| **auto** | dump + ffplay (+ GST if built) | via GST if enabled | `ffplay` on `PATH` | Recommended default |

## 播放器 / Players (繁體中文)

在 `config.toml` 設定 `player.implementation`（須與編譯進 binary 的 **Cargo feature** 一致）。預設 feature 為 **`h264-dump` + `ffmpeg` + `gstreamer`**。`implementation = "auto"` 會以 **GStreamer 為主**（即時影音），並同時 dump + 可選 ffplay。

### GStreamer

- 支援**視訊與音訊**流（**ALAC** + **AAC-ELD**）。
- 需安裝 [GStreamer](https://gstreamer.freedesktop.org/download/) 1.x（含 `h264parse`、`avdec_h264`、`avdec_alac` / `avdec_aac`、`autovideosink` 等外掛）。
- Windows 編譯 Rust binding 需要 **`pkg-config`**（位於 GStreamer 的 `bin`）與 `PKG_CONFIG_PATH` — 建議：`.\run.ps1 -GStreamer`。
- 設定：`implementation = "gstreamer"`，並以 `--features gstreamer` 編譯。

### FFmpeg

- **僅支援視訊**（透過 **`ffplay`** 播放 stdin 的 H.264 annex-B）。
- 預設路徑**不處理 AAC-ELD 音訊**；若要在 FFmpeg 生態完整支援 AAC-ELD，通常需自行編譯並啟用如 `--enable-libfdk-aac`（非本專案預設 `ffplay` 鏡像路徑）。
- 需安裝 **FFmpeg**，且 **`ffplay` 必須在 `PATH` 中**。
- 設定：`implementation = "ffmpeg"`（預設 feature 已含 `ffmpeg`）。

### VLC

- 播放常在**數秒後停止**（stdin H.264 demux 不穩定）。
- 需安裝 **VLC**（`vlc` / `cvlc` 在 `PATH`）。
- 實際使用請優先選 GStreamer 或 FFmpeg。
- 設定：`implementation = "vlc"`，並以 `--features vlc` 編譯。

### h264-dump

- 將視訊流寫入檔案（預設 **`dump.h264`**）。
- 無即時視窗、不需系統多媒體函式庫。
- 適合協定除錯：`ffplay -f h264 dump.h264`。
- 設定：`implementation = "h264-dump"`（預設 feature 可用）。

| 後端 | 視訊 | 音訊 | 需求 | 穩定性 |
|------|------|------|------|--------|
| **GStreamer** | 有 | ALAC + AAC-ELD | GStreamer 1.x（編譯需 pkg-config） | 即時影音較佳 |
| **FFmpeg** | 有（`ffplay`） | 無（預設路徑） | `PATH` 中有 `ffplay` | 視訊視窗可用 |
| **VLC** | 有 | 無 | `PATH` 中有 VLC | 數秒後易停 |
| **h264-dump** | 僅檔案 | 無 | 無 | 除錯用穩定 |
| **auto** | dump + ffplay（+ 可選 GST） | 若有 GST | `PATH` 中有 `ffplay` | **建議預設** |

## Prerequisites

### Rust

- Rust **1.70+** recommended (workspace uses edition 2021). Install via [rustup](https://rustup.rs/).

### Default run / build (player = **auto**, **GStreamer** primary)

Plain **`cargo run`** defaults to features **`h264-dump` + `ffmpeg` + `gstreamer`** and  
`player.implementation = "auto"`:

1. **GStreamer** — primary live window (video + ALAC / AAC-ELD audio)  
2. **h264-dump** — `dump.h264`  
3. **ffplay** — optional secondary video window  

Windows: install **GStreamer MSVC x86_64** (runtime + dev).  
If you see **`0xC0000135 STATUS_DLL_NOT_FOUND`**, GStreamer `bin` is not on **PATH** when the exe starts.

```powershell
cd F:\airplay2-rust

# Recommended (sets PATH for DLLs + pkg-config)
.\run.ps1

# cargo run also uses scripts\gst-runner.cmd (via .cargo/config.toml runner)
cargo run

# Release package — use the launcher, not the bare exe:
.\build-release.ps1
.\dist\run-airplay.cmd

# Without GStreamer:
.\run.ps1 -NoGStreamer
cargo run --no-default-features --features "h264-dump,ffmpeg"
```

You should see a log like:  
`player: auto (tee) — backends=h264-dump + gstreamer + ffmpeg/ffplay`

### System tray (taskbar)

While running, look for the **orange circle** tray icon (notification area). Right‑click:

| Menu | Action |
|------|--------|
| **Status / About** | Server name, port, player, resolution (also double‑click icon) |
| **Open config.toml** | Edit settings in your default editor |
| **Open dump folder** | Folder containing `dump.h264` |
| **Open install folder** | Folder of the `.exe` |
| **Exit** | Stop the receiver cleanly |

Also works with `Ctrl+C` in the console.

### GStreamer build on Windows (optional)

Rust’s `gstreamer` crates need **`pkg-config.exe`** (ships in GStreamer `bin`) **and** `PKG_CONFIG_PATH`:

```powershell
# Easiest:
.\run.ps1 -GStreamer

# Manual:
$gst = "C:\Program Files\gstreamer\1.0\msvc_x86_64"
$env:Path = "$gst\bin;" + $env:Path   # includes pkg-config.exe
$env:PKG_CONFIG_PATH = "$gst\lib\pkgconfig"
$env:GST_PLUGIN_PATH = "$gst\lib\gstreamer-1.0"
cargo run -p airplay-app --features "h264-dump,ffmpeg,gstreamer"
```

If you see `The pkg-config command could not be found`, you forgot to add GStreamer’s `bin` to `PATH` for that terminal.

File dump only:

```bash
cargo run -p airplay-app --no-default-features --features h264-dump
```

### Firewall / mDNS (all OSes)

The receiver advertises `_airplay._tcp` and `_raop._tcp` via mDNS (UDP **5353** multicast) and listens on a dynamic TCP control port (logged at start) plus media ports negotiated in SETUP.

| OS | Notes |
|----|--------|
| **Windows** | Allow the `airplay-app` binary through Windows Defender Firewall (Private networks). Multicast/mDNS may need “File and Printer Sharing” style LAN access. Run from an elevated shell only if UDP 5353 bind fails. |
| **Linux** | Ensure firewall (ufw/firewalld/nftables) allows UDP 5353 and the TCP control/media ports on the LAN interface. Some setups need `avahi-daemon` not conflicting; this stack uses `mdns-sd` directly. |
| **macOS** | Grant **Local Network** permission when prompted. System Settings → Privacy & Security → Local Network. Blocked multicast = device never sees the receiver. |

### GStreamer (`--features gstreamer`)

Needs **GStreamer 1.x** with base/good/libav (or equivalent) plugins so `h264parse`, `avdec_h264`, `videoconvert`, `autovideosink`, and for audio `avdec_alac` / `avdec_aac` are available.

**Windows**

1. Install [GStreamer MSVC 64-bit runtime + development](https://gstreamer.freedesktop.org/download/)
   (e.g. under `C:\Program Files\gstreamer\1.0\msvc_x86_64`).
2. Env (installer often sets the root):
   - `GSTREAMER_1_0_ROOT_MSVC_X86_64=C:\Program Files\gstreamer\1.0\msvc_x86_64`
   - Add `%GSTREAMER_1_0_ROOT_MSVC_X86_64%\bin` to `PATH`
   - `PKG_CONFIG_PATH=%GSTREAMER_1_0_ROOT_MSVC_X86_64%\lib\pkgconfig`
3. Build:

```powershell
$env:Path = "C:\Program Files\gstreamer\1.0\msvc_x86_64\bin;" + $env:Path
$env:PKG_CONFIG_PATH = "C:\Program Files\gstreamer\1.0\msvc_x86_64\lib\pkgconfig"
cargo build -p airplay-app --features gstreamer
```

**Linux (Debian/Ubuntu)**

```bash
sudo apt install \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-libav \
  gstreamer1.0-tools pkg-config
cargo build -p airplay-app --features gstreamer
```

**macOS (Homebrew)**

```bash
brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav pkg-config
cargo build -p airplay-app --features gstreamer
```

### FFmpeg / ffplay (`--features ffmpeg`)

Install FFmpeg so **`ffplay` is on `PATH`**.

| OS | Example |
|----|---------|
| Windows | [gyan.dev builds](https://www.gyan.dev/ffmpeg/builds/) or chocolatey `choco install ffmpeg` |
| Linux | `sudo apt install ffmpeg` |
| macOS | `brew install ffmpeg` |

### VLC (`--features vlc`)

Install VLC so **`vlc` or `cvlc` is on `PATH`**. Live stdin H.264 demux is **best-effort and can be unstable**; prefer GStreamer or FFmpeg for real use.

| OS | Example |
|----|---------|
| Windows | Install [VLC](https://www.videolan.org/) and add install dir to `PATH` |
| Linux | `sudo apt install vlc` |
| macOS | `brew install vlc` |

## Build

```bash
# entire workspace (default features: h264-dump)
cargo build --workspace

# tests (vector + unit)
cargo test --workspace
```

| Feature | Backend | System requirement |
|---------|---------|-------------------|
| `h264-dump` (**default**) | Write raw H.264 to a file | none |
| `gstreamer` | Live window via GStreamer | GStreamer 1.x + plugins + pkg-config |
| `ffmpeg` | Pipe H.264 to `ffplay` | FFmpeg (`ffplay` on `PATH`) |
| `vlc` | Best-effort VLC stdin | VLC CLI (`vlc` / `cvlc` on `PATH`) |

```bash
# default (h264-dump only)
cargo build -p airplay-app

# live GStreamer window
cargo build -p airplay-app --features gstreamer

# ffplay / VLC
cargo build -p airplay-app --features ffmpeg
cargo build -p airplay-app --features vlc

# several at once
cargo build -p airplay-app --features "gstreamer,ffmpeg,vlc,h264-dump"
```

`cargo build -p airplay-player` without extra features always works (h264-dump only).
Enabling `--features gstreamer` requires GStreamer 1.x development files to link.

## Run the receiver (`airplay-app`)

```bash
# optional: copy and edit config
cp crates/airplay-app/config.example.toml config.toml

# build & run (from workspace root) — default h264-dump
cargo run -p airplay-app

# GStreamer live playback
cargo run -p airplay-app --features gstreamer
# set implementation = "gstreamer" in config.toml

# FFmpeg / VLC
cargo run -p airplay-app --features ffmpeg   # implementation = "ffmpeg"
cargo run -p airplay-app --features vlc      # implementation = "vlc"

# explicit config path
cargo run -p airplay-app -- --config config.toml
```

Default config (when no `config.toml` is found):

| Section | Key | Default |
|---------|-----|---------|
| `[airplay]` | `server_name` | `airplay2-rust` |
| | `width` / `height` / `fps` | `1280` / `720` / `24` |
| `[player]` | `implementation` | `auto` |
| | `output` | `dump.h264` |

Supported `player.implementation` values (must match a compiled feature):

| Value | Feature | Notes |
|-------|---------|--------|
| `h264-dump` | `h264-dump` | Writes `player.output` (default `dump.h264`) |
| `gstreamer` | `gstreamer` | Live H.264 + ALAC/AAC-ELD via system GStreamer |
| `ffmpeg` | `ffmpeg` | Spawns `ffplay -f h264 -i -` |
| `vlc` | `vlc` | Best-effort `vlc` stdin; may be flaky |

If you select a player whose feature was not compiled in, the app exits with a clear error telling you which `--features` flag to use.

On start you should see a log line with the bound control port. On the same LAN, an iOS/macOS device should list **airplay2-rust** (or your `server_name`) as a screen-mirroring target. Stop with **Ctrl+C**.

Logging:

```bash
# Unix
RUST_LOG=debug cargo run -p airplay-app

# Windows PowerShell
$env:RUST_LOG = "debug"
cargo run -p airplay-app
```

### Config example

See [`crates/airplay-app/config.example.toml`](crates/airplay-app/config.example.toml):

```toml
[airplay]
server_name = "airplay2-rust"
width = 1920
height = 1080
fps = 60

[player]
# auto | h264-dump | gstreamer | ffmpeg | vlc
implementation = "auto"
# Used by h264-dump / auto
output = "dump.h264"
```


## Device mirror steps

### Path A — protocol validation with h264-dump (no media stack)

1. Build and run:

   ```bash
   cargo run -p airplay-app
   ```

2. Confirm logs show control port bound and Bonjour advertise (or a soft-fail warning if mDNS is blocked).
3. On an iPhone/iPad/Mac on the **same LAN**, open Screen Mirroring and select **airplay2-rust**.
4. After a short mirror session, stop the app (**Ctrl+C**). Inspect `dump.h264`:

   ```bash
   ffplay -f h264 dump.h264
   # or
   ffprobe dump.h264
   ```

   Non-empty Annex-B H.264 indicates pair + FairPlay + video path worked.

### Path B — live window with GStreamer

1. Install GStreamer (see [Prerequisites](#gstreamer---features-gstreamer)).
2. Config:

   ```toml
   [player]
   implementation = "gstreamer"
   ```

3. Run:

   ```bash
   cargo run -p airplay-app --features gstreamer
   ```

4. Mirror from the device; a live decode window should appear.
5. If discovery fails: check firewall, mDNS (UDP 5353), and that phone and PC share a LAN (not isolated guest Wi‑Fi).

### Path C — FFmpeg ffplay

```toml
[player]
implementation = "ffmpeg"
```

```bash
cargo run -p airplay-app --features ffmpeg
```

## Client usage sketch

`airplay-client` is a library (no default binary). Typical flow against a running receiver:

```rust
use std::time::Duration;
use airplay_client::{browse_airplay, ControlClient, FairPlayVideoEncryptor};

#[tokio::main]
async fn main() -> airplay_client::Result<()> {
    // 1) Discover receivers on the LAN
    let services = browse_airplay(Duration::from_secs(3)).await?;
    for s in &services {
        println!("{} @ {}:{}", s.name, s.host, s.port);
    }
    let svc = services.first().expect("no AirPlay service found");

    // 2) Control channel: info + pairing
    let mut client = ControlClient::connect(&svc.host, svc.port).await?;
    let _info_plist = client.get_info().await?;
    let _server_pk = client.pair_setup().await?;
    let shared = client.pair_verify().await?; // 32-byte ECDH secret

    // 3) After full SETUP (fp-setup + streams — extend as needed),
    //    encrypt video NALs for the sender path:
    // let enc = FairPlayVideoEncryptor::new(&aes_key, &shared, &stream_connection_id)?;
    let _ = shared;
    Ok(())
}
```

Limits: discovery/control/encrypt are present; full mirror **sender** (media sockets + continuous NAL push) is not a polished end-user app. Smoke against local `airplay-app` as needed.

## Known limitations

| Topic | Detail |
|-------|--------|
| **`overflow-checks = false`** | Root `Cargo.toml` disables integer overflow checks in `[profile.dev]` and `[profile.test]`. FairPlay **OmgHax / HandGarble** was ported for **Java-style silent `i32` wraparound**; enabling overflow checks breaks vector parity. This is **known technical debt** — prefer isolating wrap semantics (e.g. `wrapping_*` / explicit casts) and re-enabling checks later. |
| **mDNS permissions** | Advertise/browse soft-fail when UDP 5353 or multicast is blocked; device may not list the receiver. See firewall/mDNS notes above. |
| **Audio complexity** | ALAC / AAC-ELD decrypt and player paths exist; A/V sync, buffering, and edge formats are less battle-tested than video dump. |
| **VLC backend** | Best-effort stdin H.264; often flaky vs GStreamer/FFmpeg. |
| **Default player** | App default is `auto` (`h264-dump` + `ffplay`). GStreamer is optional (`--features gstreamer` / `.\run.ps1 -GStreamer`) for ALAC/AAC-ELD live audio. |
| **Client** | Discovery + pair + encrypt primitives; not a full AirPlay sender application. |
| **Platforms** | Intended for **Windows / Linux / macOS**. Real-device mirror acceptance is manual on at least one OS. |

## Docs

| Document | Path |
|----------|------|
| Design | [`docs/superpowers/specs/2026-08-01-airplay2-rust-design.md`](docs/superpowers/specs/2026-08-01-airplay2-rust-design.md) |
| Implementation plan | [`docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md`](docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md) |
| Acceptance checklist | [`docs/superpowers/plans/acceptance-checklist.md`](docs/superpowers/plans/acceptance-checklist.md) |

## License

MIT (see workspace `Cargo.toml`). Upstream Java project license and Apple trademarks remain separate obligations.
