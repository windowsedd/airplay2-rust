# airplay2-rust

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

## Prerequisites

### Rust

- Rust **1.70+** recommended (workspace uses edition 2021). Install via [rustup](https://rustup.rs/).

### Default run (live window via **ffplay**)

Plain **`cargo run`** does **not** need GStreamer. It uses **h264-dump + ffmpeg/ffplay** (install FFmpeg so `ffplay` is on `PATH`).

```powershell
# Windows (from repo root)
.\run.ps1
# or:
cargo run
```

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
| `[player]` | `implementation` | `h264-dump` |
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
width = 1280
height = 720
fps = 24

[player]
# h264-dump | gstreamer | ffmpeg | vlc
implementation = "h264-dump"
# Used by h264-dump only
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
| **Default player** | App default is `h264-dump` so clean builds work without system media libs. Production-style live mirror uses GStreamer when installed. |
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
