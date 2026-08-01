# airplay2-rust Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `java-airplay-2open` to a Rust Cargo workspace with full module parity (lib, server, all players, client), GStreamer as default player, Windows primary (+ optional Linux; no macOS receiver), Java vector tests + real-device mirror acceptance.

**Architecture:** Multi-crate workspace mirroring Java modules. `airplay-lib` owns pairing/FairPlay/RTSP setup/decrypt/Bonjour helpers (no TCP servers). `airplay-server` owns RTSP/HTTP control, media sockets, sessions, and the `AirPlayConsumer` trait. Players implement that trait behind Cargo features. `airplay-app` wires config → player → server. `airplay-client` is the sender path.

**Tech Stack:** Rust 2021 / edition 2024-compatible with rustc 1.96+, Tokio, tracing, thiserror, anyhow, ed25519-dalek, x25519-dalek, aes + ctr, sha2, plist (or custom binary plist), mdns-sd, gstreamer (optional feature), bytes, tokio-util.

**Spec:** `docs/superpowers/specs/2026-08-01-airplay2-rust-design.md`  
**Java reference root:** `java-airplay-2open/`

## Global Constraints

- Platforms: **Windows primary**, Linux optional; **macOS is not a supported receiver build/run target** (iOS/macOS senders only). First real-device mirror on Windows.
- Default player: GStreamer (`player.implementation = gstreamer`).
- Acceptance: ported Java unit/vector tests green **and** real-device screen mirror with GStreamer.
- Pure Rust crypto for pairing/FairPlay path (no OpenSSL required for core).
- `java-airplay-2open/` remains reference only; never built by Cargo.
- Educational/research disclaimer in root README (same intent as upstream).
- Prefer TDD: write failing tests from Java fixtures first, then port.
- Commit after each task; small focused commits.
- FairPlay table assets and `encrypted_payload` must be **byte-identical** to Java resources.

---

## File structure (target)

```
airplay2-rust/
├── Cargo.toml                          # workspace
├── README.md
├── .gitignore
├── crates/
│   ├── airplay-lib/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs                  # re-exports + AirPlay façade
│   │   │   ├── error.rs
│   │   │   ├── airplay.rs              # AirPlay struct (pair/fp/rtsp/decrypt)
│   │   │   ├── stream_info.rs          # VideoStreamInfo, AudioStreamInfo, MediaStreamInfo
│   │   │   ├── pairing.rs
│   │   │   ├── fairplay.rs
│   │   │   ├── rtsp.rs
│   │   │   ├── bonjour.rs
│   │   │   ├── decrypt/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── video.rs
│   │   │   │   └── audio.rs
│   │   │   └── crypto/
│   │   │       ├── mod.rs
│   │   │       ├── omg_hax.rs
│   │   │       ├── omg_hax_const.rs
│   │   │       ├── hand_garble.rs
│   │   │       ├── modified_md5.rs
│   │   │       └── sap_hash.rs
│   │   ├── resources/                  # table_s1..s10 (copied from Java)
│   │   └── tests/
│   │       ├── pairing_test.rs
│   │       ├── fairplay_test.rs
│   │       ├── omg_hax_test.rs
│   │       ├── hand_garble_test.rs
│   │       ├── sap_hash_test.rs
│   │       └── fixtures/
│   │           └── encrypted_payload   # from Java test resources
│   ├── airplay-server/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs
│   │       ├── consumer.rs             # AirPlayConsumer trait
│   │       ├── server.rs               # AirPlayServer
│   │       ├── control/
│   │       │   ├── mod.rs
│   │       │   ├── codec.rs            # RTSP/HTTP framing
│   │       │   └── handler.rs
│   │       ├── session.rs
│   │       ├── media/
│   │       │   ├── mod.rs
│   │       │   ├── video.rs
│   │       │   ├── audio.rs
│   │       │   └── audio_control.rs
│   │       ├── packet/
│   │       │   ├── mod.rs
│   │       │   ├── video.rs
│   │       │   └── audio.rs
│   │       └── plist_util.rs
│   ├── airplay-player/
│   │   ├── Cargo.toml                  # features: gstreamer, ffmpeg, vlc, h264-dump
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── h264_dump.rs
│   │       ├── gstreamer_player.rs
│   │       ├── ffmpeg_player.rs
│   │       └── vlc_player.rs
│   ├── airplay-client/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── discovery.rs
│   │       ├── control.rs
│   │       └── encrypt.rs
│   └── airplay-app/
│       ├── Cargo.toml
│       ├── src/main.rs
│       └── config.example.toml
└── java-airplay-2open/                 # unchanged reference
```

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `crates/airplay-lib/Cargo.toml`
- Create: `crates/airplay-lib/src/lib.rs`
- Create: `crates/airplay-server/Cargo.toml`
- Create: `crates/airplay-server/src/lib.rs`
- Create: `crates/airplay-player/Cargo.toml`
- Create: `crates/airplay-player/src/lib.rs`
- Create: `crates/airplay-client/Cargo.toml`
- Create: `crates/airplay-client/src/lib.rs`
- Create: `crates/airplay-app/Cargo.toml`
- Create: `crates/airplay-app/src/main.rs`
- Create: `README.md` (short stub + disclaimer)

**Interfaces:**
- Consumes: nothing
- Produces: workspace members that `cargo build` / `cargo test` succeed

- [ ] **Step 1: Create root workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/airplay-lib",
    "crates/airplay-server",
    "crates/airplay-player",
    "crates/airplay-client",
    "crates/airplay-app",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/local/airplay2-rust"

[workspace.dependencies]
airplay-lib = { path = "crates/airplay-lib" }
airplay-server = { path = "crates/airplay-server" }
airplay-player = { path = "crates/airplay-player" }
airplay-client = { path = "crates/airplay-client" }
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "2"
anyhow = "1"
bytes = "1"
```

- [ ] **Step 2: Create `.gitignore`**

```
/target
**/*.rs.bk
.idea/
.vscode/
*.h264
dump.h264
Cargo.lock
# keep lockfile for binary apps if preferred — if committing lockfile, remove Cargo.lock from gitignore
```

Prefer **committing** `Cargo.lock` for the application workspace; if so, do not ignore `Cargo.lock`.

- [ ] **Step 3: Create stub crates**

`crates/airplay-lib/Cargo.toml`:
```toml
[package]
name = "airplay-lib"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
thiserror = { workspace = true }
tracing = { workspace = true }
bytes = { workspace = true }
```

`crates/airplay-lib/src/lib.rs`:
```rust
//! AirPlay protocol library: pairing, FairPlay, RTSP setup, decrypt, Bonjour helpers.

pub fn workspace_smoke() -> &'static str {
    "airplay-lib"
}
```

`crates/airplay-server/Cargo.toml`:
```toml
[package]
name = "airplay-server"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
airplay-lib = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
```

`crates/airplay-server/src/lib.rs`:
```rust
//! AirPlay receiver server (control + media).
```

`crates/airplay-player/Cargo.toml`:
```toml
[package]
name = "airplay-player"
version.workspace = true
edition.workspace = true
license.workspace = true

[features]
default = ["h264-dump"]
h264-dump = []
gstreamer = []
ffmpeg = []
vlc = []

[dependencies]
airplay-server = { workspace = true }
tracing = { workspace = true }
```

`crates/airplay-player/src/lib.rs`:
```rust
//! Player backends implementing airplay-server consumers.
```

`crates/airplay-client/Cargo.toml`:
```toml
[package]
name = "airplay-client"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
airplay-lib = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
```

`crates/airplay-client/src/lib.rs`:
```rust
//! AirPlay sender / discovery client.
```

`crates/airplay-app/Cargo.toml`:
```toml
[package]
name = "airplay-app"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "airplay-app"
path = "src/main.rs"

[dependencies]
airplay-server = { workspace = true }
airplay-player = { workspace = true, features = ["h264-dump"] }
anyhow = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

`crates/airplay-app/src/main.rs`:
```rust
fn main() {
    println!("airplay-app scaffold — not ready for devices yet");
}
```

- [ ] **Step 4: README stub with disclaimer**

```markdown
# airplay2-rust

Rust port of [java-airplay-2open](java-airplay-2open/) (AirPlay receiver).

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
```

- [ ] **Step 5: Verify build**

Run: `cargo build`
Expected: all crates compile.

Run: `cargo test -p airplay-lib`
Expected: 0 tests, success (or only doc tests).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore README.md crates/
git commit -m "chore: scaffold airplay2-rust cargo workspace"
```

---

### Task 2: `airplay-lib` errors + stream info types

**Files:**
- Create: `crates/airplay-lib/src/error.rs`
- Create: `crates/airplay-lib/src/stream_info.rs`
- Modify: `crates/airplay-lib/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub enum AirPlayError` with variants used later
  - `pub type Result<T> = std::result::Result<T, AirPlayError>`
  - `MediaStreamInfo::{Video(VideoStreamInfo), Audio(AudioStreamInfo)}`
  - `VideoStreamInfo { stream_connection_id: String }`
  - `AudioStreamInfo` with compression type, sample rate, channels, etc. matching Java `AudioStreamInfo`

- [ ] **Step 1: Implement `error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AirPlayError {
    #[error("pairing error: {0}")]
    Pairing(String),
    #[error("fairplay error: {0}")]
    FairPlay(String),
    #[error("rtsp setup error: {0}")]
    Rtsp(String),
    #[error("decrypt error: {0}")]
    Decrypt(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
}

pub type Result<T> = std::result::Result<T, AirPlayError>;
```

- [ ] **Step 2: Implement `stream_info.rs`** (mirror Java fields)

Port field names from:
- `java-airplay-2open/lib/.../VideoStreamInfo.java`
- `java-airplay-2open/lib/.../AudioStreamInfo.java`
- `java-airplay-2open/lib/.../MediaStreamInfo.java`

```rust
#[derive(Debug, Clone)]
pub enum MediaStreamInfo {
    Video(VideoStreamInfo),
    Audio(AudioStreamInfo),
}

#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub stream_connection_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    Unknown,
    Alac,
    Aac,
    AacEld,
    // extend if Java has more
}

#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    pub compression_type: CompressionType,
    pub samples_per_frame: u32,
    pub sample_rate: u32,
    pub channels: u32,
    // add remaining Java fields 1:1 when porting RTSP.rs
}
```

Read the Java files and add **every** field Java uses in RTSP SETUP parsing.

- [ ] **Step 3: Export from `lib.rs`**

```rust
mod error;
mod stream_info;

pub use error::{AirPlayError, Result};
pub use stream_info::{AudioStreamInfo, CompressionType, MediaStreamInfo, VideoStreamInfo};
```

- [ ] **Step 4: `cargo test -p airplay-lib` and commit**

```bash
cargo test -p airplay-lib
git add crates/airplay-lib
git commit -m "feat(lib): add errors and stream info types"
```

---

### Task 3: Pairing (TDD against Java semantics)

**Files:**
- Create: `crates/airplay-lib/src/pairing.rs`
- Create: `crates/airplay-lib/tests/pairing_test.rs`
- Modify: `crates/airplay-lib/Cargo.toml` (crypto deps)
- Modify: `crates/airplay-lib/src/lib.rs`
- Create: `crates/airplay-lib/src/airplay.rs` (façade starting with pairing)

**Java reference:** `java-airplay-2open/lib/.../internal/Pairing.java`, `AirPlayPairingTest.java`

**Interfaces:**
- Produces:
  - `pub struct Pairing`
  - `Pairing::pair_setup(&self) -> [u8; 32]` (Ed25519 public key)
  - `Pairing::pair_verify(&mut self, request: &[u8]) -> Result<Vec<u8>>`
  - `Pairing::is_pair_verified(&self) -> bool`
  - `Pairing::shared_secret(&self) -> Option<&[u8; 32]>`
  - `AirPlay::pair_setup`, `pair_verify`, `is_pair_verified`

**Dependencies to add:**
```toml
ed25519-dalek = { version = "2", features = ["rand_core"] }
x25519-dalek = { version = "2", features = ["static_secrets"] }
aes = "0.8"
ctr = "0.9"
sha2 = "0.10"
rand_core = { version = "0.6", features = ["getrandom"] }
hex = "0.4" # dev optional
```

- [ ] **Step 1: Write failing integration test `tests/pairing_test.rs`**

Port logic from `AirPlayPairingTest.pairingTest`:

1. `pair_setup` → 32-byte public key  
2. Build pair-verify flag=1 request: `[1,0,0,0] + client_curve_pub[32] + client_ed_pub[32]`  
3. `pair_verify` → response length 96 (32 ecdh pub + 64 encrypted sig)  
4. Build pair-verify flag=0 request with client encrypted signature (AES-CTR key/iv from SHA-512 of `"Pair-Verify-AES-Key"|"Pair-Verify-AES-IV"` + shared secret)  
5. Assert `is_pair_verified() == true`

Use `ed25519-dalek` + `x25519-dalek` on the **test client** side the same way the Java test acts as the iOS client.

- [ ] **Step 2: Run test — expect FAIL**

Run: `cargo test -p airplay-lib --test pairing_test`
Expected: compile error or FAIL (API missing).

- [ ] **Step 3: Implement `pairing.rs`**

Port `Pairing.java` method-by-method:

- Key generation: Ed25519 keypair at `Pairing::new()`
- `pair_setup`: write 32-byte public key  
- `pair_verify` flag > 0: read ecdh_theirs + ed_theirs; generate X25519; shared secret; sign `ecdh_ours || ecdh_theirs` with Ed25519; AES-CTR encrypt signature with key/iv = first 16 bytes of SHA-512(`Pair-Verify-AES-Key`||secret) and SHA-512(`Pair-Verify-AES-IV`||secret); response = ecdh_ours || encrypted_sig  
- `pair_verify` flag == 0: decrypt and verify peer signature over `ecdh_theirs || ecdh_ours`; set `pair_verified`

**AES-CTR note:** Java `Cipher.update` before `doFinal` advances CTR state — match Java's two-step use in verify phase 2 (see test: `update` on prior ciphertext then encrypt new signature). Port the exact CTR counter progression.

- [ ] **Step 4: Implement thin `AirPlay` façade methods for pairing**

```rust
pub struct AirPlay {
    pairing: Pairing,
    // fairplay, rtsp later
}

impl AirPlay {
    pub fn new() -> Self { /* ... */ }
    pub fn pair_setup(&self) -> [u8; 32] { self.pairing.pair_setup() }
    pub fn pair_verify(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.pairing.pair_verify(request)
    }
    pub fn is_pair_verified(&self) -> bool { self.pairing.is_pair_verified() }
}
```

- [ ] **Step 5: Run tests — expect PASS**

Run: `cargo test -p airplay-lib --test pairing_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/airplay-lib
git commit -m "feat(lib): implement AirPlay pairing (pair-setup/verify)"
```

---

### Task 4: FairPlay setup messages (fp-setup)

**Files:**
- Create: `crates/airplay-lib/src/fairplay.rs`
- Create: `crates/airplay-lib/tests/fairplay_setup_test.rs`
- Modify: `crates/airplay-lib/src/airplay.rs`, `lib.rs`

**Java reference:** `FairPlay.java`, first half of `AirPlayFairPlayTest.java`

**Interfaces:**
- `FairPlay::fair_play_setup(&mut self, request: &[u8]) -> Result<Vec<u8>>`
- `FairPlay::decrypt_aes_key(&self, ekey: &[u8]) -> Result<[u8; 16]>` (implemented fully in Task 5–6; stub error until OmgHax ready)
- Stores `key_msg: [u8; 164]` from second setup

- [ ] **Step 1: Write failing test with exact Java byte arrays**

Use the exact `fairPlaySetup1Request/Response` and `fairPlaySetup2Request/Response` byte arrays from `AirPlayFairPlayTest.java` (signed Java bytes → cast to `u8` via `as u8` / `wrapping`).

```rust
#[test]
fn fairplay_setup_phase1_matches_java() {
    let mut fp = FairPlay::new();
    let req: [u8; 16] = [/* Java bytes as u8 */];
    let resp = fp.fair_play_setup(&req).unwrap();
    let expected: Vec<u8> = vec![/* ... */];
    assert_eq!(resp, expected);
}
```

- [ ] **Step 2: Implement `fair_play_setup` only** (static reply tables from Java lines 25–38)

- version check `data[4] == 3`
- len 16 → mode = `data[14]`, return `reply_message[mode]`
- len 164 → save key_msg, return header + last 20 bytes of request

- [ ] **Step 3: Wire `AirPlay::fair_play_setup`**

- [ ] **Step 4: `cargo test -p airplay-lib --test fairplay_setup_test` PASS + commit**

```bash
git commit -m "feat(lib): implement FairPlay fp-setup handshake messages"
```

---

### Task 5: Port OmgHax crypto primitives (tables + helpers)

**Files:**
- Create: `crates/airplay-lib/resources/table_s1` … `table_s10` (copy from `java-airplay-2open/lib/src/main/resources/`)
- Create: `crates/airplay-lib/src/crypto/mod.rs`
- Create: `crates/airplay-lib/src/crypto/omg_hax_const.rs`
- Create: `crates/airplay-lib/src/crypto/modified_md5.rs`
- Create: `crates/airplay-lib/src/crypto/sap_hash.rs`
- Create: `crates/airplay-lib/src/crypto/hand_garble.rs`
- Create: `crates/airplay-lib/src/crypto/omg_hax.rs`
- Create: tests ported from Java `OmgHaxTest`, `HandGarbleTest`, `SapHashTest`

**Java reference (line-by-line port, do not reimplement algorithmically from scratch):**
- `OmgHax.java` (~346 lines)
- `OmgHaxConst.java`
- `HandGarble.java` (~253 lines)
- `ModifiedMD5.java`
- `SapHash.java`

**Interfaces:**
- `OmgHax::decrypt_aes_key(message: &[u8], encrypted_aes_key: &[u8], out: &mut [u8; 16])`
- Internal helpers as private fns matching Java private methods

- [ ] **Step 1: Copy resources**

```bash
# PowerShell
Copy-Item java-airplay-2open/lib/src/main/resources/table_s* crates/airplay-lib/resources/
```

Embed with `include_bytes!("../resources/table_s1")` etc. in `omg_hax_const.rs` or load module.

- [ ] **Step 2: Port Java unit tests first** (copy expected hex/arrays from Java tests)

Run: expect FAIL

- [ ] **Step 3: Port `ModifiedMD5` and `SapHash`** — smallest units; tests green

- [ ] **Step 4: Port `HandGarble`** — tests green

- [ ] **Step 5: Port `OmgHax` + `OmgHaxConst`** — tests green

**Porting rules:**
- Java `byte` is signed; convert carefully with `as i8` / `as u8` at boundaries.
- Prefer `i32` where Java uses `int` for bit ops that assume 32-bit.
- Keep method names close to Java for review diffs.
- No “simplify” until all tests pass.

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(lib): port FairPlay OmgHax crypto stack and tables"
```

---

### Task 6: Video decryptor + full FairPlay vector test

**Files:**
- Create: `crates/airplay-lib/src/decrypt/mod.rs`
- Create: `crates/airplay-lib/src/decrypt/video.rs`
- Create: `crates/airplay-lib/src/decrypt/audio.rs`
- Create: `crates/airplay-lib/tests/fixtures/encrypted_payload` (copy)
- Create: `crates/airplay-lib/tests/fairplay_decrypt_test.rs`
- Create: `crates/airplay-lib/src/rtsp.rs` (minimal: store ekey/eiv/stream id from plist)
- Modify: `airplay.rs` for `rtsp_setup`, `get_fairplay_aes_key`, `decrypt_video`

**Java reference:** `FairPlayVideoDecryptor.java`, `FairPlayAudioDecryptor.java`, `RTSP.java`, remainder of `AirPlayFairPlayTest`

**Interfaces:**
- `AirPlay::rtsp_setup(&mut self, plist_bytes: &[u8]) -> Result<Option<MediaStreamInfo>>`
- `AirPlay::get_fairplay_aes_key(&self) -> Result<[u8; 16]>`
- `AirPlay::decrypt_video(&mut self, buf: &mut [u8]) -> Result<()>`
- `AirPlay::decrypt_audio(&mut self, buf: &mut [u8], audio_length: usize) -> Result<()>`
- `is_fairplay_video_decryptor_ready` / `is_fairplay_audio_decryptor_ready`

- [ ] **Step 1: Copy `encrypted_payload` fixture**

From `java-airplay-2open/lib/src/test/resources/encrypted_payload`

- [ ] **Step 2: Add binary plist dependency**

```toml
plist = "1"  # or `quick-xml` is wrong; need binary plist — use `plist` crate binary support
```

If `plist` crate lacks a needed type, parse SETUP dictionaries with a minimal binary plist subset.

- [ ] **Step 3: Write full `fairplay_decrypt_test` port of `AirPlayFairPlayTest.fairPlayTest`**

Sequence: fp-setup 1+2 → rtsp setup ekey/eiv → rtsp setup stream → construct decryptor with fixed shared secret from Java test → decrypt fixture → assert NAL length field.

Note: Java test injects shared secret via direct `FairPlayVideoDecryptor` constructor; either:
- expose test-only constructor, or
- set pairing shared secret via `#[cfg(test)]` hook on `AirPlay`.

- [ ] **Step 4: Implement RTSP setup storage + video decryptor port**

AES key schedule and IV derivation: port Java exactly (SHA, stream connection id string, etc.).

- [ ] **Step 5: Implement audio decryptor** (unit test if Java has one; otherwise compile + mirror Java API)

- [ ] **Step 6: All lib tests PASS + commit**

```bash
cargo test -p airplay-lib
git commit -m "feat(lib): RTSP setup state and FairPlay video/audio decryptors"
```

---

### Task 7: Bonjour / mDNS advertise

**Files:**
- Create: `crates/airplay-lib/src/bonjour.rs`
- Modify: `lib.rs`, `Cargo.toml` (`mdns-sd`, `if-addrs` or `local-ip-address`)

**Java reference:** `AirPlayBonjour.java`

**Interfaces:**
- `pub struct AirPlayBonjour { server_name: String, /* handles */ }`
- `AirPlayBonjour::start(&mut self, air_tunes_port: u16) -> Result<()>`
- `AirPlayBonjour::stop(&mut self)`
- TXT keys identical to Java for `_airplay._tcp` and `_raop._tcp`

- [ ] **Step 1: Implement advertise with same TXT map as Java** (`features`, `srcvers`, `model`, `pk`, …)

- [ ] **Step 2: Manual smoke** (document): run a tiny example or unit that starts/stops without panic; full discovery verified later with device.

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(lib): mDNS AirPlay/RAOP service advertisement"
```

---

### Task 8: Server config + consumer trait + session

**Files:**
- Create: `crates/airplay-server/src/config.rs`
- Create: `crates/airplay-server/src/consumer.rs`
- Create: `crates/airplay-server/src/session.rs`
- Modify: `crates/airplay-server/src/lib.rs`

**Interfaces:**
```rust
#[derive(Debug, Clone)]
pub struct AirPlayConfig {
    pub server_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

pub trait AirPlayConsumer: Send + Sync {
    fn on_video_format(&self, info: &VideoStreamInfo);
    fn on_video(&self, data: &[u8]);
    fn on_video_src_disconnect(&self);
    fn on_audio_format(&self, info: &AudioStreamInfo);
    fn on_audio(&self, data: &[u8]);
    fn on_audio_src_disconnect(&self);
    // optional HLS defaults as no-ops via default impl if using async_trait carefully —
    // for sync trait, provide empty default methods in a macro or separate defaulting wrapper
}

pub struct Session {
    pub airplay: AirPlay,
    // media task handles later
}

pub struct SessionManager { /* DashMap or Mutex<HashMap<String, Session>> */ }
```

Use `active_remote` / session id keys exactly as Java `SessionManager`.

- [ ] **Step 1: Implement types**
- [ ] **Step 2: Unit test session insert/get**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(server): config, AirPlayConsumer trait, session manager"
```

---

### Task 9: RTSP/HTTP control codec + handler (core paths)

**Files:**
- Create: `crates/airplay-server/src/control/mod.rs`
- Create: `crates/airplay-server/src/control/codec.rs`
- Create: `crates/airplay-server/src/control/handler.rs`
- Create: `crates/airplay-server/src/plist_util.rs`
- Create: `crates/airplay-server/src/server.rs`
- Create: `crates/airplay-server/tests/control_info_test.rs` (optional golden)
- Modify: deps — `tokio-util`, `http` or custom RTSP types

**Java reference:** `ControlServer.java`, `ControlHandler.java`, `PropertyListUtil.java`

**Interfaces:**
- `AirPlayServer::new(config, consumer: Arc<dyn AirPlayConsumer>) -> Self`
- `AirPlayServer::start(&mut self) -> Result<()>`  // bind random port, bonjour, spawn accept loop
- `AirPlayServer::port(&self) -> u16`
- `AirPlayServer::stop(&mut self)`

**Minimum handler paths for this task:**
- RTSP `GET /info`
- `POST /pair-setup`, `/pair-verify`, `/fp-setup`
- RTSP `SETUP` (parse body via `AirPlay::rtsp_setup`; start media servers in Task 10)
- RTSP `TEARDOWN`, `RECORD`, `GET_PARAMETER`, `SET_PARAMETER`, `FLUSH`
- `POST /feedback`
- HTTP `GET /server-info`
- Unknown → 404

- [ ] **Step 1: Implement framing** — decode RTSP/HTTP requests from TCP stream into method, path, headers, body (Content-Length)

- [ ] **Step 2: Implement response builder** — status, CSeq echo, body

- [ ] **Step 3: Port PropertyListUtil binary responses for `/info` and SETUP replies** (compare against Java test bins under `server/src/test/resources/one_mirroring_app/` when possible)

- [ ] **Step 4: Integration test** — connect TCP, send minimal GET /info (from golden bin or crafted), assert 200 and non-empty body

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(server): RTSP/HTTP control server and core handlers"
```

---

### Task 10: Video/audio media servers + packets

**Files:**
- Create: `crates/airplay-server/src/media/*`
- Create: `crates/airplay-server/src/packet/*`
- Modify: control SETUP handler to bind ports and fill SETUP response plist

**Java reference:** `VideoServer`, `AudioServer`, `AudioControlServer`, `VideoHandler`, `AudioHandler`, `VideoPacket`, `AudioPacket`, decoders if needed for framing

**Interfaces:**
- On video SETUP: spawn video listener; put port in response; on data → `session.airplay.decrypt_video` → `consumer.on_video`
- On audio SETUP: same for audio
- TEARDOWN stops listeners and calls disconnect hooks

- [ ] **Step 1: Port packet header parsing**
- [ ] **Step 2: Video TCP/UDP path (match Java transport)**
- [ ] **Step 3: Audio path**
- [ ] **Step 4: Audio control path**
- [ ] **Step 5: Manual or recorded-packet unit tests if fixtures exist (`video_packet_type_*`, `audio_packet_type_96`)**
- [ ] **Step 6: Commit**

```bash
git commit -m "feat(server): video/audio media servers and decrypt-to-consumer path"
```

---

### Task 11: h264-dump player + app binary (first runnable receiver)

**Files:**
- Create: `crates/airplay-player/src/h264_dump.rs`
- Modify: `crates/airplay-player/src/lib.rs`
- Modify: `crates/airplay-app/src/main.rs`
- Create: `crates/airplay-app/config.example.toml`
- Add: `toml`, `clap` or env-based config

**Interfaces:**
```rust
pub struct H264Dump {
    // file: Mutex<File>
}
impl AirPlayConsumer for H264Dump { /* write on_video to dump.h264 */ }
```

- [ ] **Step 1: Implement H264Dump consumer**
- [ ] **Step 2: App loads config, starts `AirPlayServer` with dump consumer, Ctrl+C stop**
- [ ] **Step 3: Document run steps in README**
- [ ] **Step 4: Device smoke (manual):** iOS mirror → file grows; commit code even if device not available in CI

```bash
git commit -m "feat: h264-dump player and runnable airplay-app"
```

---

### Task 12: GStreamer player (default)

**Files:**
- Create: `crates/airplay-player/src/gstreamer_player.rs`
- Modify: `Cargo.toml` features and `gstreamer` / `gstreamer-app` deps (optional)
- Modify: `airplay-app` default feature to gstreamer when available

**Java reference:** `player/gstreamer/*`

**Interfaces:**
- `GStreamerPlayer::new() -> Result<Self>`
- Push H.264 buffers into appsrc; ALAC/AAC-ELD audio pipeline as in Java

- [ ] **Step 1: Feature-gated dependency**
- [ ] **Step 2: Video pipeline to window**
- [ ] **Step 3: Audio pipeline**
- [ ] **Step 4: README install notes for Windows (+ optional Linux) GStreamer**
- [ ] **Step 5: Real-device acceptance on primary OS**
- [ ] **Step 6: Commit**

```bash
git commit -m "feat(player): GStreamer backend for live mirror playback"
```

---

### Task 13: FFmpeg + VLC backends

**Files:**
- Create: `crates/airplay-player/src/ffmpeg_player.rs`
- Create: `crates/airplay-player/src/vlc_player.rs`

**Java reference:** `FFmpegPlayer.java`, `VlcPlayer.java`

- [ ] **Step 1: FFmpeg via `ffplay` subprocess or piped stdin** (video-first)
- [ ] **Step 2: VLC best-effort** (document instability)
- [ ] **Step 3: `cargo check -p airplay-player --features ffmpeg,vlc`**
- [ ] **Step 4: Commit**

```bash
git commit -m "feat(player): FFmpeg and VLC player backends"
```

---

### Task 14: Client crate

**Files:**
- Create: `crates/airplay-client/src/discovery.rs`
- Create: `crates/airplay-client/src/control.rs`
- Create: `crates/airplay-client/src/encrypt.rs`
- Optional bin example under `crates/airplay-client` or `airplay-app` feature

**Java reference:** `client/src/main/java/...`

- [ ] **Step 1: mDNS browse for AirPlay services**
- [ ] **Step 2: Control client pair + fp-setup + SETUP**
- [ ] **Step 3: FairPlay video encryptor port if present in Java**
- [ ] **Step 4: Smoke against local `airplay-app`**
- [ ] **Step 5: Commit**

```bash
git commit -m "feat(client): discovery and control client"
```

---

### Task 15: Cross-platform polish + docs + acceptance checklist

**Files:**
- Modify: `README.md` (full build/run matrix)
- Create: `docs/superpowers/plans/acceptance-checklist.md` (optional)
- Fix: compile warnings, feature docs, firewall/mDNS notes

- [ ] **Step 1: README sections** — prerequisites per OS, `cargo run -p airplay-app`, player features, disclaimer
- [ ] **Step 2: Verify `cargo test` workspace**
- [ ] **Step 3: Verify `cargo build --workspace` on available OSes**
- [ ] **Step 4: Fill acceptance checklist** (vectors, device, players, client)
- [ ] **Step 5: Final commit**

```bash
git commit -m "docs: cross-platform run guide and acceptance notes"
```

---

## Self-review (spec coverage)

| Spec requirement | Task(s) |
|------------------|---------|
| Workspace crates lib/server/player/client/app | 1 |
| Pairing Ed25519/X25519/AES-CTR | 3 |
| FairPlay fp-setup | 4 |
| OmgHax + tables | 5 |
| Decrypt video/audio + RTSP ekey | 6 |
| Bonjour | 7 |
| Consumer + sessions | 8 |
| Control RTSP/HTTP | 9 |
| Media servers | 10 |
| h264-dump + app | 11 |
| GStreamer default | 12 |
| FFmpeg + VLC | 13 |
| Client | 14 |
| Win (+ optional Linux) docs + acceptance | 15 |
| Java vector tests | 3–6 |
| Real-device mirror | 11–12 manual |

**Placeholder scan:** No TBD steps; OmgHax tasks specify line-by-line port + Java tests rather than embedding full algorithm text (files are 200–350 lines each — implementers port from listed Java paths).

**Type consistency:** `AirPlay`, `AirPlayConfig`, `AirPlayConsumer`, `MediaStreamInfo` names stable across tasks 2–12.

---

## Execution notes

- **Riskiest tasks:** 5 (OmgHax byte semantics), 9–10 (RTSP + media timing), 12 (GStreamer + device).
- **Do not skip TDD on Tasks 3–6** — device debugging without green vectors wastes days.
- After Task 11, a file-based mirror path unblocks protocol work without GStreamer installed.
- Full parity = all tasks 1–15; usable mirror = tasks 1–12.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — this session runs tasks with executing-plans checkpoints  

Which approach?
