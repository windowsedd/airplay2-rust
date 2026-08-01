# Windows GStreamer Preview Design

**Date:** 2026-08-01  
**Status:** Approved for implementation planning  
**Scope:** High-quality, low-latency Windows preview through the optional GStreamer player backend

**Prerequisite:** GStreamer 1.20+; bounded nonblocking appsrc ingress uses the `max-buffers` and `leaky-type` properties introduced in 1.20.

## 1. Goal

Provide a deterministic Windows-preferred GStreamer video path that preserves the sender's H.264 resolution and aspect ratio, avoids re-encoding, prefers D3D11 hardware decode/rendering, and falls back cleanly when preferred runtime elements cannot be used.

The receiver remains an educational and research project. This change does not alter pairing, FairPlay, OmgHax, HandGarble, SapHash, or other cryptographic behavior.

## 2. Current video path

The video TCP server reads one 128-byte AirPlay header followed by the declared payload:

- Type `0` is one encrypted picture payload. After FairPlay decryption, the payload contains one or more AVCC length-prefixed NAL units belonging to that picture. The server replaces each four-byte AVCC length with the Annex-B start code `00 00 00 01` and sends the entire picture in one `AirPlayConsumer::on_video` callback.
- Type `1` contains codec configuration. The server extracts the genuine SPS and PPS, converts them to Annex-B, and sends them together in one callback.
- Type-1 headers also expose source and stream dimensions. The server reports valid changes through `on_video_size`.
- The video header has an unused timestamp field in the Java reference, but neither the Java nor Rust server exposes a verified video clock or timescale to the player.

The current GStreamer path declares Annex-B, access-unit-aligned H.264, but it always uses `avdec_h264`, blocks `appsrc`, has no bounded leaky queue, and renders with `sync=false`. Uncommitted orientation work additionally forces decoded frames through BGRx conversion, frame sampling, `videoflip`, and `videocrop`. That path prevents D3D11 zero-copy rendering and can modify the decoded picture.

## 3. Selected approach

Construct a deterministic pipeline from runtime-selected decoder and sink candidates, following the repository's existing GStreamer parse-launch architecture:

```text
appsrc name=video_src
    is-live=true
    format=time
    do-timestamp=true
    block=false
    max-buffers=<mode bound>
    max-bytes=4194304
    leaky-type=<upstream for quality, downstream otherwise>
    caps="video/x-h264,stream-format=byte-stream,alignment=au"
! queue name=video_queue
    max-size-buffers=<mode>
    max-size-bytes=0
    max-size-time=0
    leaky=<mode>
! h264parse config-interval=-1
! <selected decoder>
! <selected sink> force-aspect-ratio=true sync=<mode>
```

No encoder, image conversion, frame extraction, fixed-resolution caps, scaling element, rotation element, crop element, or CPU frame probe belongs in this direct preview path. Existing orientation helpers may remain for other backends or future opt-in processed rendering, but GStreamer direct preview bypasses them.

## 4. Runtime decoder and sink selection

Decoder candidates are ordered:

1. `d3d11h264dec` when `hardware_decode = true`
2. `avdec_h264`
3. `decodebin`

Windows sink candidates are ordered:

1. `d3d11videosink`
2. `autovideosink`

Non-Windows builds use `autovideosink`; GStreamer remains optional on every platform.

Selection first checks `gst::ElementFactory` availability. Candidate decoder/sink combinations are then built in deterministic order, given three seconds to complete the transition to actual `READY`, and checked for asynchronous bus errors. The first combination that succeeds is retained; failed candidates are returned to `NULL` and logged before the next candidate is tried. This handles an installed D3D11 factory that cannot initialize on the active machine more cleanly than a name-only check.

The selected decoder, whether it is hardware accelerated, the selected sink, input caps, and fallback reasons are logged with `tracing`.

## 5. Preview modes

The player configuration adds backward-compatible fields:

```toml
[player]
preview_mode = "balanced"
hardware_decode = true
```

Supported modes are:

| Mode | Queue | Leaky | Sink sync | Intent |
|------|-------|-------|-----------|--------|
| `quality` | 8 buffers | no | `true` | Preserve short bursts when latency is secondary |
| `balanced` | 3 buffers | downstream | `true` | Bounded latency with clocked presentation |
| `low-latency` | 2 buffers | downstream | `false` | Prefer the newest frames and immediate rendering |

Missing fields deserialize to `balanced` and `hardware_decode = true`. Invalid preview-mode strings produce a clear configuration error rather than silently selecting a different mode. `appsrc` remains non-blocking for all modes so the media socket is not stalled by a slow renderer.

## 6. Access-unit validation and codec state

### 6.1 Strict AVCC conversion

Type-0 AirPlay packets are the available picture/access-unit boundary. AVCC conversion must validate every declared NAL length before mutating or forwarding the payload. A zero length, truncated NAL, incomplete trailing length, or otherwise invalid layout rejects the whole picture and produces a structured warning. Partially converted payloads are never sent to a player.

Pure Rust NAL inspection recognizes Annex-B start codes and at least these H.264 types:

- SPS: `7`
- PPS: `8`
- IDR slice: `5`
- non-IDR VCL slices: `1` through `4`

### 6.2 Per-session configuration gate

The GStreamer player maintains codec state scoped to the active video session:

1. Type-1 input updates the cached genuine SPS and PPS but is not pushed as a falsely labelled standalone access unit.
2. Until both SPS and PPS exist, picture buffers are rejected with rate-limited `waiting_for_codec_config` logs.
3. After configuration arrives, non-IDR pictures are dropped until an IDR arrives.
4. Each IDR is prefixed with the cached SPS and PPS and pushed as one complete Annex-B access unit. `h264parse config-interval=-1` may then repeat configuration on later IDRs as needed.
5. Subsequent validated picture access units are pushed normally.
6. A new SPS/PPS pair marks a codec reconfiguration, logs the reported resolution change, and returns to the wait-for-IDR state.
7. Disconnect clears cached SPS, PPS, IDR readiness, dimensions, and counters so reconnection cannot reuse stale codec state.

TCP transport prevents network-level packet reordering within a connection. The receiver has no verified video sequence number or keyframe-request mechanism, so recovery consists of retaining genuine configuration, rejecting undecodable inter frames, and resuming at the next sender-provided IDR.

## 7. Timing

The implementation retains the existing arrival-time behavior:

- `appsrc format=time`
- `do-timestamp=true`
- no invented PTS, DTS, or duration

The AirPlay video header's timestamp is not yet exposed with a verified timescale or clock mapping. Manually treating it as nanoseconds or deriving a fixed frame duration would risk unstable playback. A future change must validate the timestamp semantics against the sender and then extend the consumer boundary with explicit video timing metadata.

## 8. State, errors, and logging

Structured logs cover:

- selected decoder and hardware/software status;
- selected video sink;
- unavailable factories and failed candidate initialization;
- input caps and preview-mode queue settings;
- pipeline state transitions at pipeline scope;
- decoder and sink errors or warnings, including source element names;
- queue overruns and downstream-leaky drops;
- rejected AVCC pictures and failed `appsrc` pushes;
- missing SPS/PPS, wait-for-IDR, recovery, and session reset;
- reported stream resolution changes.

Queue overrun notification maintains a drop counter. High-frequency events are rate-limited so normal mirroring does not flood logs.

## 9. Code boundaries

- `crates/airplay-server/src/packet/video.rs`: strict AVCC validation/conversion and pure NAL helpers where server framing owns the decision.
- `crates/airplay-server/src/media/video.rs`: reject malformed pictures; preserve one callback per validated picture; report genuine SPS/PPS and dimensions.
- `crates/airplay-player/src/`: feature-independent preview-mode, fallback-selection, and codec-state logic plus feature-gated GStreamer construction.
- `crates/airplay-app/src/main.rs`: TOML parsing, defaults, and passing preview settings to GStreamer.
- `config.toml` and `crates/airplay-app/config.example.toml`: example settings without breaking older files.
- `README.md` and acceptance documentation: Windows plugins, inspection commands, fallback behavior, preview-mode trade-offs, sender-quality limitation, and live-test status.

The crate dependency graph remains unchanged, and `h264-dump` continues to build without GStreamer.

## 10. Tests

GPU-independent unit tests cover:

- parsing `quality`, `balanced`, and `low-latency`;
- queue and sink-sync options for every mode;
- backward-compatible config parsing without the new keys;
- invalid preview-mode rejection;
- decoder order with hardware decode enabled and disabled;
- Windows sink ordering;
- simulated runtime availability and candidate selection;
- strict multi-NAL AVCC conversion and malformed/truncated rejection;
- Annex-B SPS, PPS, IDR, and non-IDR classification;
- codec gate start, configuration update, IDR recovery, and disconnect reset.

Tests do not create a visible window or require a GPU. Feature builds verify Rust/GStreamer integration when development files are present.

## 11. Verification and acceptance

Automated verification:

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo check -p airplay-app --no-default-features --features h264-dump
cargo check -p airplay-app --features gstreamer
```

Windows element inspection:

```powershell
gst-inspect-1.0 d3d11h264dec
gst-inspect-1.0 d3d11videosink
gst-inspect-1.0 h264parse
gst-inspect-1.0 avdec_h264
```

The current development machine has GStreamer 1.28.5 factories for all four preferred elements plus `decodebin` and `autovideosink`. This confirms availability, not live AirPlay correctness.

Final acceptance still requires a Windows real-device session confirming:

- the log selects `d3d11h264dec` and `d3d11videosink` when usable;
- the window preserves the sender's geometry and aspect ratio;
- balanced and low-latency modes remain responsive under motion;
- software fallback displays video when D3D11 decoding is disabled or unavailable;
- reconnect and sender rotation/resolution changes recover at a new IDR.

Until that session is performed, the implementation must be reported as compile-tested and awaiting Windows live validation, not as a proven high-quality preview.

## 12. Risks

- AirPlay type-0 packets are treated as picture/access-unit boundaries based on the existing Java/Rust framing. Captures from additional senders may reveal unusual fragmentation and would require an explicit assembler before retaining `alignment=au`.
- The server does not expose verified video timing, so quality and balanced modes clock arrival timestamps rather than sender timestamps.
- Resolution renegotiation depends on genuine new SPS/PPS followed by an IDR.
- A decoder can fail after reaching `READY`; the bus will report the failure, but automatic mid-stream reconstruction is intentionally deferred unless live testing demonstrates that preflight fallback is insufficient.
- Bypassing decoded-frame orientation and crop processing preserves the stream and D3D11 path, but devices that emit incorrectly oriented pixels will require a separate opt-in processed pipeline rather than changing the direct preview.
