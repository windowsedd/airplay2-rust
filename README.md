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

## Run the receiver (`airplay-app`)

The first runnable backend is **h264-dump**: it advertises an AirPlay receiver
via Bonjour/mDNS and writes decrypted video to a raw H.264 file.

```bash
# optional: copy and edit config
cp crates/airplay-app/config.example.toml config.toml

# build & run (from workspace root)
cargo run -p airplay-app

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

On start you should see a log line with the bound control port. On the same LAN,
an iOS/macOS device should list **airplay2-rust** (or your `server_name`) as a
screen-mirroring target. After mirroring, `dump.h264` grows; stop with **Ctrl+C**.

Inspect the dump (example):

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

Other player backends (`gstreamer`, `ffmpeg`, `vlc`) are planned; currently only
`h264-dump` is implemented.
