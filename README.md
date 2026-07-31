# airplay2-rust

Rust port of java-airplay-2open (AirPlay receiver).

## Disclaimer

1. Educational / research only. No commercial or illegal use.
2. User bears legal responsibility.
3. Provided "as is" without warranty.
4. AirPlay is a trademark of Apple Inc. This project is not affiliated with Apple.

## Build

```bash
cargo build
cargo test
```

Default build uses the **h264-dump** player (no system media libs required).

Optional player backends:

| Feature | Backend | System requirement |
|---------|---------|-------------------|
| `h264-dump` (default) | Write raw H.264 to a file | none |
| `gstreamer` | Live window via GStreamer | GStreamer 1.x + plugins |
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

## Player prerequisites

### GStreamer (`--features gstreamer`)

Needs **GStreamer 1.x** with base/good/libav (or equivalent) plugins so
`h264parse`, `avdec_h264`, `videoconvert`, `autovideosink`, and for audio
`avdec_alac` / `avdec_aac` are available.

**Windows**

1. Install [GStreamer MSVC 64-bit runtime + development](https://gstreamer.freedesktop.org/download/)
   (e.g. under `C:\Program Files\gstreamer\1.0\msvc_x86_64`).
2. Ensure env vars (installer usually sets the root):
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

Install FFmpeg so **`ffplay` is on `PATH`**, then:

```bash
cargo run -p airplay-app --features ffmpeg
# config: player.implementation = "ffmpeg"
```

### VLC (`--features vlc`)

Install VLC so **`vlc` or `cvlc` is on `PATH`**. Live stdin H.264 demux is
**best-effort and can be unstable**; prefer GStreamer or FFmpeg for real use.

```bash
cargo run -p airplay-app --features vlc
# config: player.implementation = "vlc"
```

## Run the receiver (`airplay-app`)

```bash
# optional: copy and edit config
cp crates/airplay-app/config.example.toml config.toml

# build & run (from workspace root) — default h264-dump
cargo run -p airplay-app

# GStreamer live playback
cargo run -p airplay-app --features gstreamer
# set implementation = "gstreamer" in config.toml

# or with an explicit config path
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

If you select a player whose feature was not compiled in, the app exits with a
clear error telling you which `--features` flag to use.

On start you should see a log line with the bound control port. On the same LAN,
an iOS/macOS device should list **airplay2-rust** (or your `server_name`) as a
screen-mirroring target. Stop with **Ctrl+C**.

### h264-dump inspection

```bash
ffplay -f h264 dump.h264
# or
ffprobe dump.h264
```

Logging is controlled by `RUST_LOG` (default filter: `info`):

```bash
RUST_LOG=debug cargo run -p airplay-app
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
implementation = "h264-dump"
output = "dump.h264"
```
