# Windows GStreamer Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Windows-preferred D3D11 GStreamer preview with strict H.264 access-unit handling, deterministic fallback, bounded latency modes, and backward-compatible configuration.

**Architecture:** Keep AirPlay packet framing in `airplay-server`, move GPU-independent preview policy and Annex-B codec gating into focused `airplay-player` modules, and keep GStreamer construction behind its optional feature. The app parses user settings and passes typed options into the player; the direct pipeline performs no re-encoding, decoded-frame sampling, cropping, rotation, or fixed-resolution scaling.

**Tech Stack:** Rust 2021, Tokio, serde/TOML, tracing, GStreamer 1.20+ with Rust bindings 0.23 and gstreamer-app 0.23 behind optional Cargo features. The minimum runtime is required by appsrc `max-buffers` and `leaky-type`.

## Global Constraints

- Preserve educational/research framing and the existing Java reference tree.
- Do not modify pairing, FairPlay, OmgHax, HandGarble, SapHash, or overflow profiles.
- Preserve crate direction: `lib <- server <- player/app`; no cycles.
- GStreamer stays optional; `h264-dump` must compile with no system GStreamer.
- Preserve sender resolution and aspect ratio; add no encoder, image conversion, frame extraction, crop, rotation, or fixed-resolution scaling to the direct GStreamer path.
- Use sender-provided SPS/PPS only and clear codec state on disconnect.
- Retain arrival-time GStreamer timestamps until AirPlay video timing is verified and exposed.
- Existing uncommitted changes are user-owned; edit only the narrow overlapping sections required by this feature.

---

### Task 1: Preview policy and fallback ordering

**Files:**
- Create: `crates/airplay-player/src/preview.rs`
- Modify: `crates/airplay-player/src/lib.rs`

**Interfaces:**
- Produces `PreviewMode::{Quality, Balanced, LowLatency}` with `FromStr<Err = String>`.
- Produces `PreviewOptions { queue_max_buffers: u32, queue_leaky: bool, sink_sync: bool }`.
- Produces `DecoderChoice::{D3d11H264Dec, AvdecH264, DecodeBin}` and `SinkChoice::{D3d11VideoSink, AutoVideoSink}`.
- Produces `decoder_candidates(hardware_decode: bool)`, `sink_candidates(is_windows: bool)`, `select_decoder`, and `select_sink` over a supplied availability predicate.

- [ ] **Step 1: Write failing preview policy tests**

Add tests in `preview.rs` specifying exact behavior:

```rust
#[test]
fn preview_modes_map_to_bounded_queue_options() {
    assert_eq!(PreviewMode::Quality.options(), PreviewOptions {
        queue_max_buffers: 8,
        queue_leaky: false,
        sink_sync: true,
    });
    assert_eq!(PreviewMode::Balanced.options(), PreviewOptions {
        queue_max_buffers: 3,
        queue_leaky: true,
        sink_sync: true,
    });
    assert_eq!(PreviewMode::LowLatency.options(), PreviewOptions {
        queue_max_buffers: 2,
        queue_leaky: true,
        sink_sync: false,
    });
}

#[test]
fn parses_only_supported_preview_modes() {
    assert_eq!("quality".parse(), Ok(PreviewMode::Quality));
    assert_eq!("balanced".parse(), Ok(PreviewMode::Balanced));
    assert_eq!("low-latency".parse(), Ok(PreviewMode::LowLatency));
    assert!("fast-ish".parse::<PreviewMode>().is_err());
}

#[test]
fn decoder_order_respects_hardware_toggle() {
    assert_eq!(decoder_candidates(true), vec![
        DecoderChoice::D3d11H264Dec,
        DecoderChoice::AvdecH264,
        DecoderChoice::DecodeBin,
    ]);
    assert_eq!(decoder_candidates(false), vec![
        DecoderChoice::AvdecH264,
        DecoderChoice::DecodeBin,
    ]);
}

#[test]
fn selection_skips_unavailable_factories_in_order() {
    let available = ["avdec_h264", "decodebin", "autovideosink"];
    assert_eq!(select_decoder(true, |name| available.contains(&name)), Some(DecoderChoice::AvdecH264));
    assert_eq!(select_sink(true, |name| available.contains(&name)), Some(SinkChoice::AutoVideoSink));
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cargo test -p airplay-player --no-default-features preview::tests -- --nocapture`  
Expected: compile failure because `preview` types and functions do not exist.

- [ ] **Step 3: Implement the minimal typed policy**

Implement exact factory names through `factory_name()` methods and derive `Debug`, `Clone`, `Copy`, `PartialEq`, and `Eq`. Implement `Display` for `PreviewMode` so structured logs and config errors use canonical strings.

- [ ] **Step 4: Export policy types and verify GREEN**

Add `mod preview;` and public re-exports in `lib.rs`.  
Run: `cargo test -p airplay-player --no-default-features preview::tests -- --nocapture`  
Expected: all preview policy tests pass without linking GStreamer.

---

### Task 2: Strict AirPlay picture conversion

**Files:**
- Modify: `crates/airplay-server/src/packet/video.rs`
- Modify: `crates/airplay-server/src/media/video.rs`

**Interfaces:**
- Replaces `prepare_picture_nal_units(&mut [u8])` with `prepare_picture_nal_units(&mut [u8]) -> Result<usize, PictureNalError>`.
- Produces `PictureNalError::{Empty, IncompleteLength, ZeroLength, Truncated { declared, remaining }}`.
- Media forwards a type-0 payload only after full validation succeeds.

- [ ] **Step 1: Replace permissive expectations with failing strict tests**

Keep the valid two-NAL assertion and add:

```rust
#[test]
fn annex_b_conversion_reports_nal_count() {
    let mut payload = vec![0, 0, 0, 2, 0x65, 0xaa, 0, 0, 0, 1, 0x06];
    assert_eq!(prepare_picture_nal_units(&mut payload), Ok(2));
    assert_eq!(&payload[..4], &[0, 0, 0, 1]);
    assert_eq!(&payload[6..10], &[0, 0, 0, 1]);
}

#[test]
fn truncated_picture_is_rejected_without_partial_mutation() {
    let original = vec![0, 0, 0, 2, 0xaa, 0xbb, 0, 0, 0, 5, 0xcc];
    let mut payload = original.clone();
    assert!(matches!(prepare_picture_nal_units(&mut payload), Err(PictureNalError::Truncated { .. })));
    assert_eq!(payload, original);
}

#[test]
fn incomplete_length_and_zero_length_are_rejected() {
    assert_eq!(prepare_picture_nal_units(&mut [0, 0, 0]), Err(PictureNalError::IncompleteLength));
    assert_eq!(prepare_picture_nal_units(&mut [0, 0, 0, 0]), Err(PictureNalError::ZeroLength));
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p airplay-server packet::video::tests -- --nocapture`  
Expected: compile failures because the function still returns `()` and `PictureNalError` is absent.

- [ ] **Step 3: Implement validate-then-mutate conversion**

First walk the immutable bytes and collect each four-byte length offset. Reject the whole buffer on any invalid layout. Only after validation succeeds, replace all collected prefixes with `00 00 00 01` and return the number of NAL units.

- [ ] **Step 4: Reject invalid media payloads with structured logs**

Change the media type-0 path to call the result-returning converter. On error, increment `malformed_picture`, log the error and byte count with `tracing::warn!`, and skip `consumer.on_video`.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test -p airplay-server packet::video::tests -- --nocapture`  
Expected: all video packet tests pass.

---

### Task 3: Annex-B classification and codec gate

**Files:**
- Create: `crates/airplay-player/src/h264.rs`
- Modify: `crates/airplay-player/src/lib.rs`

**Interfaces:**
- Produces `NalKinds { has_sps, has_pps, has_idr, has_vcl }`.
- Produces `classify_annex_b(&[u8]) -> Result<NalKinds, H264InputError>`.
- Produces `CodecGate::push(&mut self, &[u8]) -> GateResult` where `GateResult` is `CodecConfigUpdated`, `WaitingForConfig`, `WaitingForIdr`, `AccessUnit(Vec<u8>)`, or `Rejected(H264InputError)`.
- Produces `CodecGate::reset()` and cached SPS/PPS state scoped to one session.

- [ ] **Step 1: Write failing NAL classification tests**

```rust
#[test]
fn classifies_parameter_sets_and_picture_types() {
    let config = annex_b(&[&[0x67, 1], &[0x68, 2]]);
    let kinds = classify_annex_b(&config).unwrap();
    assert!(kinds.has_sps && kinds.has_pps);
    assert!(!kinds.has_vcl);

    let idr = annex_b(&[&[0x65, 3]]);
    assert!(classify_annex_b(&idr).unwrap().has_idr);
}
```

- [ ] **Step 2: Write failing codec-gate lifecycle tests**

```rust
#[test]
fn gate_waits_for_config_then_idr_and_prefixes_config() {
    let mut gate = CodecGate::default();
    assert_eq!(gate.push(&annex_b(&[&[0x41, 1]])), GateResult::WaitingForConfig);
    assert_eq!(gate.push(&annex_b(&[&[0x67, 2], &[0x68, 3]])), GateResult::CodecConfigUpdated);
    assert_eq!(gate.push(&annex_b(&[&[0x41, 4]])), GateResult::WaitingForIdr);
    let GateResult::AccessUnit(bytes) = gate.push(&annex_b(&[&[0x65, 5]])) else { panic!("expected IDR"); };
    let kinds = classify_annex_b(&bytes).unwrap();
    assert!(kinds.has_sps && kinds.has_pps && kinds.has_idr);
}

#[test]
fn new_config_and_disconnect_require_a_new_idr() {
    let mut gate = ready_gate();
    assert!(matches!(gate.push(&annex_b(&[&[0x41, 6]])), GateResult::AccessUnit(_)));
    assert_eq!(gate.push(&annex_b(&[&[0x67, 7], &[0x68, 8]])), GateResult::CodecConfigUpdated);
    assert_eq!(gate.push(&annex_b(&[&[0x41, 9]])), GateResult::WaitingForIdr);
    gate.reset();
    assert_eq!(gate.push(&annex_b(&[&[0x65, 10]])), GateResult::WaitingForConfig);
}
```

- [ ] **Step 3: Run and verify RED**

Run: `cargo test -p airplay-player --no-default-features h264::tests -- --nocapture`  
Expected: compile failure because the module and gate do not exist.

- [ ] **Step 4: Implement Annex-B iterator and gate**

Support three- and four-byte start codes for inspection, but emit cached configuration with four-byte start codes. Cache complete SPS and PPS NAL units copied from genuine input. Prefix configuration to every IDR, set readiness after the first IDR, and reset readiness whenever configuration changes.

- [ ] **Step 5: Export and verify GREEN**

Run: `cargo test -p airplay-player --no-default-features h264::tests -- --nocapture`  
Expected: all classifier and codec-state tests pass.

---

### Task 4: Backward-compatible application configuration

**Files:**
- Modify: `crates/airplay-app/src/main.rs`
- Modify: `config.toml`
- Modify: `crates/airplay-app/config.example.toml`

**Interfaces:**
- `PlayerSection` gains `preview_mode: String` with default `balanced` and `hardware_decode: bool` with default `true`.
- `parse_preview_settings(&PlayerSection) -> Result<(PreviewMode, bool)>` returns a contextual config error for invalid values.
- Consumer builders receive typed `PreviewMode` and `hardware_decode`.

- [ ] **Step 1: Add failing serde/default tests in `main.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_player_config_gets_safe_preview_defaults() {
        let cfg: AppConfig = toml::from_str(r#"
            [player]
            implementation = "gstreamer"
            output = "dump.h264"
        "#).unwrap();
        assert_eq!(cfg.player.preview_mode, "balanced");
        assert!(cfg.player.hardware_decode);
    }

    #[test]
    fn all_preview_modes_parse_and_invalid_mode_fails() {
        for mode in ["quality", "balanced", "low-latency"] {
            let player = PlayerSection { preview_mode: mode.into(), ..PlayerSection::default() };
            assert!(parse_preview_settings(&player).is_ok());
        }
        let player = PlayerSection { preview_mode: "turbo".into(), ..PlayerSection::default() };
        assert!(parse_preview_settings(&player).is_err());
    }
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test -p airplay-app --no-default-features --features h264-dump --bin airplay-app -- --nocapture`  
Expected: compile failures for missing configuration fields and parser.

- [ ] **Step 3: Implement config fields and propagation**

Parse once after loading config. Pass the typed values through `build_consumer` and `build_auto_consumer`. Preserve existing orientation arguments for FFmpeg, but the direct GStreamer constructor receives preview mode and hardware choice and logs that decoded-frame orientation/crop processing is bypassed.

- [ ] **Step 4: Update examples and verify GREEN**

Add to both example configs:

```toml
preview_mode = "balanced"
hardware_decode = true
```

Run: `cargo test -p airplay-app --no-default-features --features h264-dump --bin airplay-app -- --nocapture`  
Expected: all app tests pass without GStreamer.

---

### Task 5: Direct GStreamer pipeline and runtime preflight fallback

**Files:**
- Modify: `crates/airplay-player/src/gstreamer_player.rs`

**Interfaces:**
- Add `GStreamerPlayer::with_preview(initial: f64, preview_mode: PreviewMode, hardware_decode: bool) -> Result<Self, String>`.
- Keep `new()` and `with_volume()` as compatibility wrappers using balanced mode and hardware decode.
- `VideoPipeline` owns the selected pipeline, appsrc, decoder choice, sink choice, and atomic queue-overrun counter.

- [ ] **Step 1: Add feature-gated pipeline description tests**

Factor a pure `video_pipeline_description(decoder, sink, options) -> String` and test that it contains:

```rust
#[test]
fn direct_pipeline_has_required_caps_and_no_processing_or_encoding() {
    let text = video_pipeline_description(
        DecoderChoice::D3d11H264Dec,
        SinkChoice::D3d11VideoSink,
        PreviewMode::Balanced.options(),
    );
    for required in ["block=false", "stream-format=byte-stream", "alignment=au", "max-size-buffers=3", "leaky=downstream", "h264parse config-interval=-1", "d3d11h264dec", "d3d11videosink", "force-aspect-ratio=true", "sync=true"] {
        assert!(text.contains(required), "missing {required}: {text}");
    }
    for forbidden in ["x264enc", "openh264enc", "jpeg", "png", "videoscale", "videocrop", "videoflip", "BGRx"] {
        assert!(!text.contains(forbidden), "forbidden {forbidden}: {text}");
    }
}
```

- [ ] **Step 2: Run and verify RED**

Run with the configured GStreamer environment: `cargo test -p airplay-player --features gstreamer direct_pipeline_has_required_caps_and_no_processing_or_encoding -- --nocapture`  
Expected: compile failure because the description builder is absent.

- [ ] **Step 3: Build and preflight candidate pipelines**

For every available decoder candidate and sink candidate in order:

1. Generate the direct pipeline description.
2. Parse and downcast it.
3. Request `READY`, wait up to three seconds for the actual state, and reject asynchronous bus errors.
4. On failure, set `NULL`, log decoder/sink/reason, and continue.
5. Store and log the first successful combination.

Set explicit H.264 caps on appsrc, `format=time`, `is-live=true`, stream type `Stream`, and `block=false`. Bound appsrc by both buffers and bytes with an explicit leaky policy: quality drops new input to preserve older queued access units, while balanced and low-latency drop old input to keep the newest data. Rate-limit both appsrc saturation and downstream queue-overrun warnings.

- [ ] **Step 4: Integrate codec gate and structured bus logging**

On video input, pass bytes through `CodecGate`; cache configuration, rate-limit waiting logs, and only push returned access units. On disconnect, reset the gate, send end-of-stream where safe, and set the pipeline to `NULL`. Bus logs must include pipeline state transitions and the source element for errors/warnings.

- [ ] **Step 5: Verify GREEN and feature isolation**

Run:

```powershell
cargo test -p airplay-player --features gstreamer
cargo check -p airplay-player --no-default-features
```

Expected: GStreamer tests pass with installed development files; no-feature player still compiles.

---

### Task 6: Documentation, acceptance status, and full verification

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/plans/acceptance-checklist.md`

**Interfaces:**
- Documents Windows plugin requirements, inspection commands, config, modes, fallback order, sender-quality limit, timing limitation, and live-validation status.

- [ ] **Step 1: Update Windows GStreamer documentation**

Document that `gst-plugins-bad` provides D3D11 elements, `gst-libav` provides `avdec_h264`, and the direct preview performs no re-encoding. Include exact `gst-inspect-1.0` commands and this example:

```toml
[player]
implementation = "gstreamer"
preview_mode = "balanced"
hardware_decode = true
```

Explain quality/balanced/low-latency behavior and that receiver quality cannot exceed the H.264 stream sent by the AirPlay sender.

- [ ] **Step 2: Record automated versus manual acceptance honestly**

Mark element inspection and feature compilation with the exact observed environment. Leave real-device Windows mirroring unchecked until it is actually performed.

- [ ] **Step 3: Run formatting and requested verification**

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo check -p airplay-app --no-default-features --features h264-dump
cargo check -p airplay-app --features gstreamer
```

Expected: all commands exit 0. If an environment-specific failure occurs, capture the exact output and do not claim that check passed.

- [ ] **Step 4: Inspect Windows runtime elements**

```powershell
gst-inspect-1.0 d3d11h264dec
gst-inspect-1.0 d3d11videosink
gst-inspect-1.0 h264parse
gst-inspect-1.0 avdec_h264
```

Expected on the current machine: GStreamer 1.28.5 factories exist for all four elements.

- [ ] **Step 5: Review the final diff and preserve unrelated work**

Run `git diff --check` and `git status --short`. Confirm crypto, Java reference files, overflow profiles, and unrelated untracked artifacts were not modified by this task.

---

## Self-review

- Spec coverage: Tasks 1-6 cover mode parsing, fallback selection, strict framing, SPS/PPS/IDR recovery, optional GStreamer construction, structured logs, docs, and every requested verification command.
- Type consistency: `PreviewMode`, `PreviewOptions`, `DecoderChoice`, `SinkChoice`, `CodecGate`, and `GateResult` retain the same names across producer and consumer tasks.
- Scope: audio pipelines and existing non-GStreamer backends remain unchanged except for app argument plumbing required to preserve their current behavior.
- Manual boundary: Windows element availability can be verified locally; real-device AirPlay preview remains a separate live acceptance step.
