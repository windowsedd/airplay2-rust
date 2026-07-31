# Acceptance checklist — airplay2-rust

Track automated vs manual acceptance for the Rust AirPlay receiver port.
Update checkboxes as items are verified on each environment.

**Related:** [design](../specs/2026-08-01-airplay2-rust-design.md) · [plan](2026-08-01-airplay2-rust-implementation.md) · [README](../../../README.md)

---

## Automated (local / CI)

- [x] **Vector tests (crypto)** — pairing, FairPlay setup/decrypt, HandGarble, OmgHax, SapHash fixtures green under `cargo test --workspace`
- [x] **Server unit / control tests** — config, codec, packets, plist, sessions, in-process control (`/info`, pair-setup, 404)
- [x] **Player h264-dump test** — writes video bytes to file
- [x] **Client unit tests** — control framing, encrypt round-trip (mDNS browse smoke ignored without network)
- [x] **Default workspace build** — `cargo build --workspace` succeeds without GStreamer
- [x] **Feature builds (no system GStreamer)** — `cargo build -p airplay-app --features "ffmpeg,vlc,h264-dump"` links
- [ ] **Feature build gstreamer** — `cargo build -p airplay-app --features gstreamer` on a machine with GStreamer 1.x + pkg-config (not verified in docs-only environment without GStreamer installed)
- [x] **All four player backends exist** — `h264_dump`, `gstreamer_player`, `ffmpeg_player`, `vlc_player` under `crates/airplay-player`
- [x] **Client crate exists** — discovery, control, FairPlay video encryptor under `crates/airplay-client`
- [x] **Runnable app exists** — `cargo run -p airplay-app` with TOML config / defaults

### Latest `cargo test --workspace` snapshot

Recorded during Task 15 docs polish (Windows worktree):

| Suite | Result |
|-------|--------|
| airplay-client lib | 8 passed, 1 ignored (mDNS browse) |
| airplay-lib unit | 11 passed, 1 ignored (mDNS advertise) |
| fairplay_decrypt_test | 3 passed |
| fairplay_setup_test | 5 passed |
| hand_garble_test | 7 passed |
| omg_hax_test | 6 passed |
| pairing_test | 1 passed |
| sap_hash_test | 6 passed |
| airplay-player | 1 passed |
| airplay-server unit | 18 passed |
| control_info_test | 3 passed |
| **Total** | **~69 passed, 2 ignored, 0 failed** |

Re-run: `cargo test --workspace`

---

## Manual / device

- [ ] **Real-device mirror → h264-dump** — iOS/macOS Screen Mirroring appears as receiver; `dump.h264` non-empty / playable with `ffplay -f h264`
- [ ] **Real-device mirror → GStreamer** — live window shows mirrored screen on primary OS
- [ ] **Real-device audio** — ALAC or AAC-ELD path audible without crashing process (best-effort)
- [ ] **Client smoke** — `browse_airplay` finds local `airplay-app`; `ControlClient` GET `/info` + pair-setup/verify against it

---

## Cross-platform build / run notes verified

- [ ] **Windows** — build notes (default + optional GStreamer env) verified on a clean machine
- [ ] **Linux** — apt GStreamer packages + `cargo test --workspace` verified
- [ ] **macOS** — Homebrew GStreamer + Local Network permission path verified

*(Automated tests on Windows during Task 15: default build + test OK; GStreamer feature not linked without system install.)*

---

## Docs / polish

- [x] **README disclaimer** — educational / research; Apple trademark; as-is
- [x] **README crate map + prerequisites per OS**
- [x] **README build/run + feature flags + config example**
- [x] **README device mirror steps** (h264-dump then GStreamer)
- [x] **README client usage sketch + known limitations**
- [x] **overflow-checks=false documented** as HandGarble/OmgHax Java-parity technical debt
- [x] **Links to design/plan under docs/superpowers/**
- [x] **Firewall / mDNS notes** (Win/Linux/macOS)

---

## Known open items (not blockers for docs task)

| Item | Notes |
|------|--------|
| `overflow-checks = false` in dev/test profiles | Required for silent i32 wrap in FairPlay port; debt to replace with explicit wrapping ops |
| GStreamer default vs app default | Spec preferred GStreamer as “default player”; app still defaults to `h264-dump` so zero-dep builds work — set `implementation = "gstreamer"` when installed |
| VLC stability | Best-effort only |
| Full sender client | Library primitives only; no polished mirror-sender binary |

---

## Definition of done (from design §7.3)

1. [x] Ported vector/unit tests green under `cargo test`
2. [ ] Real-device mirror works with GStreamer on at least one of Win/Linux/macOS
3. [ ] Remaining OSes: project builds; run instructions documented; smoke as feasible  
     *(instructions documented; multi-OS smoke pending)*
4. [x] All four player backends exist; h264-dump verified in tests; FFmpeg/VLC compile with features
5. [x] Client crate present with discovery/control path and documented limits
