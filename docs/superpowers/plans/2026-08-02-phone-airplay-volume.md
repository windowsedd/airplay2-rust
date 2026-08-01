# Phone-Controlled AirPlay Volume Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the iPhone AirPlay volume slider control only `airplay2-rust` audio playback, without changing Windows master volume.

**Architecture:** Parse the standard RTSP `text/parameters` `volume` value in `airplay-server` and deliver it through optional `AirPlayConsumer` methods. The GStreamer consumer converts AirPlay decibels to linear gain and applies it to both audio pipelines; other consumers remain no-ops. `GET_PARAMETER` reads the active consumer value so the sender receives consistent state.

**Tech Stack:** Rust 2021, Tokio integration tests, `AirPlayConsumer`, GStreamer `volume` elements.

## Global Constraints

- Do not call Windows audio APIs or change Windows master/session mixer state.
- Keep the existing crate direction: `airplay-server` must not depend on `airplay-player`.
- Preserve unrelated uncommitted changes already present in the workspace.
- Treat finite values at or below `-144.0 dB` as mute and clamp values above `0.0 dB` to `0.0 dB`.
- Ignore malformed values and non-volume parameters while returning a normal RTSP response.

---

### Task 1: RTSP volume delivery

**Files:**
- Modify: `crates/airplay-server/src/consumer.rs`
- Modify: `crates/airplay-server/src/control/handler.rs`
- Modify: `crates/airplay-server/tests/control_info_test.rs`

**Interfaces:**
- Produces: `AirPlayConsumer::on_volume(&self, volume_db: f64)` and `AirPlayConsumer::volume(&self) -> Option<f64>`.
- Consumes: RTSP bodies containing lines such as `volume: -20.000000`.

- [ ] **Step 1: Write failing integration tests**

Add a recording consumer backed by `Mutex<Option<f64>>`. Send `SET_PARAMETER` with `volume: -20.000000`, assert the callback recorded `-20.0`, then send `GET_PARAMETER` and assert its body is `volume: -20.000000\r\n`. Add cases showing `progress:` and non-finite/malformed volume values do not update the consumer.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p airplay-server --test control_info_test volume -- --nocapture`

Expected: compilation failure because `AirPlayConsumer` has no volume methods, proving the new API is not implemented.

- [ ] **Step 3: Implement the minimal server behavior**

Add default no-op methods to the trait:

```rust
fn on_volume(&self, _volume_db: f64) {}
fn volume(&self) -> Option<f64> { None }
```

Parse UTF-8 request lines with `split_once(':')`, match `volume` case-insensitively, accept only finite `f64`, clamp to `-144.0..=0.0`, and invoke `on_volume`. Format `GET_PARAMETER` using the consumer value or `0.0` when unsupported.

- [ ] **Step 4: Run tests and verify GREEN**

Run: `cargo test -p airplay-server --test control_info_test volume -- --nocapture`

Expected: all volume-filtered integration tests pass.

### Task 2: Player fan-out and GStreamer gain

**Files:**
- Modify: `crates/airplay-player/src/tee.rs`
- Modify: `crates/airplay-player/src/gstreamer_player.rs`

**Interfaces:**
- Consumes: `AirPlayConsumer::on_volume(volume_db)` and `volume()` from Task 1.
- Produces: `airplay_db_to_linear(volume_db: f64) -> f64` and AirPlay-driven gain on `vol_alac` and `vol_aac`.

- [ ] **Step 1: Write failing unit tests**

Assert `0.0 dB -> 1.0`, `-20.0 dB -> 0.1`, `-144.0 dB -> 0.0`, and positive values clamp to unity.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p airplay-player --features gstreamer airplay_db -- --nocapture`

Expected: compilation failure because `airplay_db_to_linear` does not exist.

- [ ] **Step 3: Implement the minimal player behavior**

Use `10_f64.powf(volume_db / 20.0)` for the audible range, exact zero for mute, and existing `set_volume` to update both pipelines. Forward `on_volume` through every `TeePlayer` child; return the first child reporting a volume.

- [ ] **Step 4: Run tests and verify GREEN**

Run: `cargo test -p airplay-player --features gstreamer airplay_db -- --nocapture`

Expected: all gain conversion tests pass.

### Task 3: Regression verification

**Files:**
- Verify only; no planned source changes.

**Interfaces:**
- Consumes: completed Tasks 1 and 2.
- Produces: evidence that the workspace remains buildable and tested.

- [ ] **Step 1: Format modified Rust files**

Run: `cargo fmt --all -- --check`; if it reports formatting differences, run `cargo fmt --all` and repeat the check.

- [ ] **Step 2: Run server and player tests**

Run: `cargo test -p airplay-server`

Run: `cargo test -p airplay-player --features gstreamer`

- [ ] **Step 3: Run workspace regression tests**

Run: `cargo test --workspace`

Expected: zero failed tests; ignored hardware/network tests may remain ignored.
