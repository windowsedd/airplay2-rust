# airplay2-rust Design Spec

**Date:** 2026-08-01  
**Status:** Approved for implementation planning  
**Source reference:** `java-airplay-2open/` (humid1/java-airplay-2open, based on serezhka/java-airplay)  
**Approach:** A — Faithful Cargo workspace (1:1 with Java modules)

## 1. Goals and non-goals

### Goals

- Port the Java AirPlay **receiver** stack to idiomatic Rust with **full module parity**:
  - protocol/crypto library
  - server (control + video/audio)
  - all player backends (GStreamer, FFmpeg, VLC, h264-dump)
  - client (sender / discovery path)
- Support **Windows, Linux, and macOS**.
- Default live playback via **GStreamer**.
- Acceptance requires **both**:
  1. Ported Java **unit/vector tests** pass (crypto/protocol fixtures).
  2. **Real-device** screen mirroring works with GStreamer on at least one platform; other platforms build and are documented with a smoke path.

### Non-goals (v1 cut line)

- Affiliation with or endorsement by Apple Inc. Same educational/research disclaimer as upstream.
- Bit-rate/latency parity with commercial Apple TV receivers.
- Spring Boot, Gradle, or JVM interop.
- Perfect polish of HLS/YouTube playlist paths beyond what is needed for core mirror and existing Java hooks.
- System-tray / desktop menu parity is deferred unless needed for basic app usability.

### Disclaimer (carry into README)

Project is for learning, research, and technical exchange. AirPlay is a trademark of Apple Inc. Users are responsible for legal compliance in their jurisdiction.

## 2. Architecture

### 2.1 Workspace layout

```
airplay2-rust/
├── Cargo.toml                 # workspace root
├── crates/
│   ├── airplay-lib/           # pairing, FairPlay, RTSP setup, decrypt, Bonjour helpers
│   ├── airplay-server/        # RTSP/HTTP control, media sockets, sessions, consumer trait
│   ├── airplay-client/        # discovery + control + send path
│   ├── airplay-player/        # consumer implementations (Cargo features)
│   └── airplay-app/           # binary: config, select player, run server
├── java-airplay-2open/        # Java reference only; not built by Cargo
└── docs/superpowers/specs/    # design and plans
```

### 2.2 Crate responsibilities

| Crate | Java counterpart | Responsibility |
|-------|------------------|----------------|
| `airplay-lib` | `lib` | Pairing, FairPlay (OmgHax + tables), RTSP SETUP/TEARDOWN media info, video/audio decryptors, mDNS advertise helpers |
| `airplay-server` | `server` | Control server, session manager, video/audio/audio-control servers, packet parse, invoke consumer |
| `airplay-player` | `player/*` | `AirPlayConsumer`-equivalent trait and backends |
| `airplay-client` | `client` | Browse, control client, encrypt path, optional test source |
| `airplay-app` | `player/app` | Config loading, wiring, process lifecycle (no Spring) |

### 2.3 Dependency direction

```
airplay-app → airplay-server → airplay-lib
airplay-app → airplay-player → (implements server consumer trait)
airplay-client → airplay-lib
```

`airplay-lib` must not depend on server or players.  
`airplay-server` depends only on `airplay-lib` (plus networking crates).  
Player backends depend on the consumer interface defined in `airplay-server` (or a small shared traits module in `airplay-server` to avoid cycles).

### 2.4 Runtime and shared stack

| Concern | Choice |
|---------|--------|
| Async runtime | Tokio (multi-thread) |
| Logging | `tracing` + `tracing-subscriber` |
| Library errors | `thiserror` typed errors |
| App boundary | `anyhow` in `airplay-app` / binaries |
| Crypto (pairing) | Pure Rust: Ed25519, X25519, AES-128-CTR |
| FairPlay | Faithful port of Java `OmgHax` / tables / decryptors |
| Property lists | Binary plist encode/decode sufficient for AirPlay SETUP and info responses |
| mDNS | Cross-platform advertise/browse (e.g. `mdns-sd` + interface enumeration); document OS permissions/firewall |
| Default player | GStreamer via `gstreamer` Rust bindings |

## 3. Component design

### 3.1 `airplay-lib`

Public façade analogous to Java `AirPlay`:

- `pair_setup` → Ed25519 public key bytes  
- `pair_verify` → two-phase Curve25519 + AES-CTR encrypted Ed25519 signatures; exposes shared secret when verified  
- `fair_play_setup` → fp-setup request/response  
- `rtsp_setup` / `rtsp_teardown` → parse binary plist → `VideoStreamInfo` / `AudioStreamInfo`; store ekey, eiv, stream connection id  
- `decrypt_video` / `decrypt_audio` → lazy-init decryptors when keys ready  
- FairPlay AES key derivation from ekey + internal tables (port of `decryptAesKey`)

Internal modules (names may be Rust-idiomatic but map 1:1 for porting):

- `pairing`, `fairplay`, `rtsp`, `hand_garble`, `modified_md5`, `omg_hax`, `sap_hash`  
- Video/audio decryptors  
- Bonjour: register `_airplay._tcp` and `_raop._tcp` with TXT records matching Java (`deviceid`, `features`, `srcvers`, `flags`, `vv`, `model`, `pw`, `pk`, etc.)

**Assets:** copy `table_s1`…`table_s10` (and any other binary fixtures) into the crate; load via `include_bytes!` or build-script copy. Bytes must match Java resources for vector tests.

### 3.2 `airplay-server`

- **`AirPlayConfig`:** server name, width, height, fps (and any ports if made configurable).  
- **`AirPlayConsumer` trait:**  
  - `on_video_format`, `on_video`, `on_video_src_disconnect`  
  - `on_audio_format`, `on_audio`, `on_audio_src_disconnect`  
  - Optional HLS hooks: playlist set/remove/pause/resume, `playback_info`  
- **`AirPlayServer`:** start control server → register Bonjour on control port; stop tears down both.  
- **Control plane:** single TCP port speaking RTSP 1.0 and HTTP 1.1 (custom codec/parser; not a generic REST framework). Handle the same method/URI matrix as Java `ControlHandler` (including unknown → 404).  
- **`Session` / `SessionManager`:** per active AirPlay session state including `AirPlay` lib instance and media server handles.  
- **Media:** `VideoServer`, `AudioServer`, `AudioControlServer` — bind ports, return them in SETUP responses, parse packets, decrypt, call consumer.  
- **Plist responses:** port `PropertyListUtil` builders for `/info`, SETUP responses, server-info, playback-info as required.

### 3.3 `airplay-player`

- Implement `AirPlayConsumer` for each backend.  
- Cargo features: `gstreamer` (default for the app), `ffmpeg`, `vlc`, `h264-dump`.  
- **GStreamer:** H.264 video + ALAC / AAC-ELD audio (match Java intent).  
- **FFmpeg:** prefer subprocess `ffplay` or documented pipeline for video; audio limitations noted in README.  
- **VLC:** best-effort; document instability (as upstream).  
- **h264-dump:** write raw bitstream for debugging without GPU/UI.

### 3.4 `airplay-client`

- Service discovery (browse AirPlay/RAOP).  
- Control client for pairing / FairPlay / SETUP path as in Java client.  
- Video encrypt path (`FairPlayVideoEncryptor` equivalent) where present in Java.  
- Optional GStreamer (or simple) test source for development.

### 3.5 `airplay-app`

- Load config (TOML preferred; optional properties-compatible keys for migrants from Java).  
- Keys at minimum: `server_name`, `width`, `height`, `fps`, `player` implementation.  
- Construct chosen player as consumer, start `AirPlayServer`, graceful shutdown on Ctrl+C / SIGTERM.  
- No embedded web framework required.

## 4. Data flow (receiver / screen mirror)

```
Sender (iOS/iPadOS/macOS)
        │ mDNS browse
        ▼
Bonjour advertise (_airplay._tcp / _raop._tcp) ── control port
        │
        ▼
Control TCP (RTSP/HTTP)
  GET /info
  POST /pair-setup → /pair-verify (×2) → /fp-setup
  RTSP SETUP (video) → spawn VideoServer → on_video_format
  RECORD / SET_PARAMETER / feedback as needed
  RTSP SETUP (audio) → spawn AudioServer → on_audio_format
        │
        ▼
Media sockets
  Video packets → FairPlay video decrypt → Annex-B/NAL bytes → on_video
  Audio packets → FairPlay audio decrypt → on_audio
        │
        ▼
Player backend (default GStreamer)
        │
RTSP TEARDOWN → stop media → on_*_disconnect → session cleanup
```

**Invariant:** Pairing shared secret, ekey/eiv, and stream connection id handling must match Java semantics so existing encrypted fixtures and live clients interoperate.

## 5. Cross-platform (Windows / Linux / macOS)

| Area | Policy |
|------|--------|
| I/O | Tokio only for async networking |
| mDNS | Prefer one cross-platform stack; document multicast/firewall and macOS Local Network permission |
| GStreamer | System GStreamer 1.x required for default player; README install steps per OS |
| Optional players | Feature-gated; may be weaker on some OSes |
| Crypto core | Pure Rust; no OpenSSL required for pairing/FairPlay path |
| CI | Prefer Linux (and where available Windows/macOS) `cargo test` for lib; device tests remain manual |

## 6. Error handling

- **lib:** typed errors (`PairingError`, `FairPlayError`, `RtspError`, `DecryptError`, …); no panics on malformed peer input.  
- **server:** log with `tracing`; map failures to RTSP/HTTP status codes consistent with Java (e.g. 404 unknown, 500 internal where appropriate).  
- **media path:** if decryptor not ready or decrypt fails, log and drop packet (or fail the stream cleanly); do not crash the process.  
- **app:** fatal only for unrecoverable bind/config failures; non-zero exit code.

## 7. Testing and acceptance

### 7.1 Automated

| Layer | Content |
|-------|---------|
| Unit | Port Java tests: Pairing, FairPlay, HandGarble, OmgHax, SapHash; ship `encrypted_payload` and table resources |
| Golden / protocol | Use bins under Java `server` test resources (`one_mirroring_app`, reverse_engineering) where assertions are clear |
| Integration | Start control server in-process; replay recorded request sequences without a physical device |
| Feature builds | `cargo check` / smoke for each player feature |

### 7.2 Manual / device

- Real iPhone or iPad screen mirror → GStreamer display on primary dev OS.  
- Checklist for Linux and macOS (or Windows if primary is another): discoverable, pair, video visible.  
- Client smoke: discover and exercise control against local receiver when both are ready.

### 7.3 Definition of done (full parity)

1. Ported vector/unit tests green in CI or local `cargo test`.  
2. Real-device mirror works with GStreamer on at least one of Win/Linux/macOS.  
3. Remaining OSes: project builds; run instructions documented; smoke as feasible.  
4. All four player backends exist; h264-dump and GStreamer verified; FFmpeg/VLC at least compile + basic smoke.  
5. Client crate present with discovery/control path and documented limits.

## 8. Implementation phases

Phases deliver full scope in order; later phases do not rewrite earlier crate boundaries.

| Phase | Deliverable |
|-------|-------------|
| 1 | Workspace scaffold; `airplay-lib` pairing + FairPlay + decryptors; vector tests green |
| 2 | RTSP/plist setup; Bonjour; control server skeleton (`/info`, pair, fp-setup, SETUP plumbing) |
| 3 | Video/audio servers; decrypt to consumer; `h264-dump` (and raw audio dump if needed) |
| 4 | GStreamer backend + `airplay-app`; real-device mirror on primary OS |
| 5 | FFmpeg and VLC backends |
| 6 | `airplay-client` |
| 7 | Cross-platform polish, README, optional tray/UI extras |

**Rough effort (focused work, not a calendar guarantee):**

- Usable mirror (through phase 4): ~2–5 weeks  
- Full parity (through phase 7): ~1–2.5 months  

Device debugging and mDNS/OS issues dominate risk, not boilerplate.

## 9. Configuration (app)

Minimum settings (names illustrative; TOML example):

```toml
[airplay]
server_name = "airplay2-rust"
width = 1280
height = 720
fps = 24

[player]
implementation = "gstreamer"  # gstreamer | ffmpeg | vlc | h264-dump
```

## 10. Open decisions (resolved)

| Topic | Decision |
|-------|----------|
| Scope | Full parity (lib, server, all players, client) |
| Architecture | A — multi-crate workspace |
| Platforms | Windows + Linux + macOS |
| Default player | GStreamer |
| Acceptance | Real-device mirror + Java test vectors |

## 11. Next step

After stakeholder review of this written spec, create an implementation plan via the writing-plans workflow (`docs/superpowers/plans/…`) and execute phase by phase.
