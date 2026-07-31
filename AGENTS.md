# AGENTS.md — airplay2-rust

Instructions for AI coding agents (Grok, Claude Code, Cursor, Codex, etc.) working in this repository.

**Also see:** [CLAUDE.md](./CLAUDE.md) (Claude-oriented entry; links here).

## Project

Rust port of **java-airplay-2open** (AirPlay **receiver**: screen mirror + media). Educational / research only; not affiliated with Apple.

| Path | Purpose |
|------|---------|
| `crates/airplay-lib` | Pairing, FairPlay/OmgHax, RTSP setup, decrypt, Bonjour helpers |
| `crates/airplay-server` | Control (RTSP/HTTP), sessions, media sockets → `AirPlayConsumer` |
| `crates/airplay-player` | Backends: `h264-dump`, `gstreamer`, `ffmpeg`, `vlc` (features) |
| `crates/airplay-client` | mDNS browse + control + video encrypt |
| `crates/airplay-app` | Binary: TOML config → server + player |
| `java-airplay-2open/` | **Reference only** (if present); never built by Cargo |
| `docs/superpowers/specs/` | Design spec |
| `docs/superpowers/plans/` | Implementation plan + acceptance checklist |

Canonical docs:

- [README.md](./README.md) — build/run/prerequisites
- [Design](docs/superpowers/specs/2026-08-01-airplay2-rust-design.md)
- [Plan](docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md)
- [Acceptance](docs/superpowers/plans/acceptance-checklist.md)

## Hard rules

1. **Disclaimer:** keep educational/research framing; do not market as commercial AirPlay product.
2. **Crypto fidelity:** FairPlay / OmgHax / HandGarble / SapHash / pairing must stay **line-faithful** to Java. Prefer porting over “cleaner” rewrites. Java `byte` is signed; `int` wrap is intentional.
3. **`overflow-checks = false`** in workspace `dev`/`test` profiles exists for HandGarble Java wrap parity. Do **not** re-enable without replacing bare `i32` ops with `wrapping_*` first.
4. **Crate graph:** `lib` ← `server` ← `player` / `app`; `client` → `lib` only. No cycles.
5. **Java tree:** do not delete or “clean up” `java-airplay-2open` as part of normal work; it is the protocol oracle + fixtures.

## Build & test

```bash
cargo build --workspace
cargo test --workspace
# Live window (default features include gstreamer):
cargo run
# or:
cargo run -p airplay-app -- --config config.toml
# File-only dump (no GStreamer):
cargo run -p airplay-app --no-default-features --features h264-dump
```

Windows GStreamer (typical):

```powershell
$env:Path = "C:\Program Files\gstreamer\1.0\msvc_x86_64\bin;" + $env:Path
$env:PKG_CONFIG_PATH = "C:\Program Files\gstreamer\1.0\msvc_x86_64\lib\pkgconfig"
$env:GST_PLUGIN_PATH = "C:\Program Files\gstreamer\1.0\msvc_x86_64\lib\gstreamer-1.0"
```

Config: root [`config.toml`](./config.toml) or [`crates/airplay-app/config.example.toml`](./crates/airplay-app/config.example.toml).  
**Live window:** `player.implementation = "gstreamer"` + `--features gstreamer`.  
**File only:** `h264-dump` → `dump.h264` (no UI).

## Conventions

- Edition **2021**, workspace deps in root `Cargo.toml`.
- Errors: `thiserror` in libs (`airplay_lib::Result`), `anyhow` at binary edge.
- Logging: `tracing` (prefer structured fields).
- Async: Tokio for server/app/client I/O.
- Player backends: optional Cargo features; never make GStreamer required for default `cargo test`.
- When porting Java tests, convert signed Java bytes with `as u8` / `i8 as u8` and keep expected arrays exact.
- Prefer small commits: `feat(lib|server|player|client|app): …` / `fix: …` / `docs: …`.

## Where to change what

| Goal | Touch |
|------|--------|
| Pairing / FairPlay / decrypt | `crates/airplay-lib/src/` (+ `resources/table_s*`) |
| RTSP/HTTP handlers, SETUP ports | `crates/airplay-server/src/control/`, `plist_util.rs` |
| Video/audio framing | `crates/airplay-server/src/packet/`, `media/` |
| Live window / dump / ffplay | `crates/airplay-player/src/` |
| CLI / config defaults | `crates/airplay-app/src/main.rs`, `config.toml` |
| Discovery / sender | `crates/airplay-client/src/` |

## Verification before “done”

- `cargo test --workspace` green (ignored mDNS tests may need UDP 5353).
- Crypto changes: existing FairPlay/pairing/OmgHax tests still pass.
- UI claim: only valid with GStreamer (or FFmpeg/VLC) feature + matching `implementation`.

## Security / scope

- Do not add exfiltration, telemetry, or remote update channels.
- Do not commit secrets, device dumps of private content, or large binary captures unless they are existing test fixtures.
- Protocol reverse-engineering code stays research-oriented.

## Agent file map

| File | Role |
|------|------|
| **[AGENTS.md](./AGENTS.md)** | This file — shared agent instructions |
| **[CLAUDE.md](./CLAUDE.md)** | Claude Code entrypoint; points here |
| [README.md](./README.md) | Human-facing docs |
