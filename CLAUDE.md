# CLAUDE.md

Claude Code (and other Claude-based agents) project entry for **airplay2-rust**.

## Primary instructions

**Read and follow [AGENTS.md](./AGENTS.md)** for architecture, hard rules, build/test, and conventions.

This file exists so Claude discovers the repo rules quickly; **AGENTS.md is the source of truth** for agent behavior. Keep the two in sync when rules change (prefer editing AGENTS.md, then a one-line summary here if needed).

## Quick context

- **What:** Rust AirPlay *receiver* (mirror + media), ported from `java-airplay-2open`.
- **Why:** Learning / research only — not an Apple product.
- **Run with window:**  
  `cargo run -p airplay-app --features gstreamer -- --config config.toml`  
  (GStreamer on `PATH`; `player.implementation = "gstreamer"`.)
- **Run dump-only:**  
  `implementation = "h264-dump"` → writes `dump.h264` (no UI).

## Essential links

| Doc | Link |
|-----|------|
| Agent rules (canonical) | [AGENTS.md](./AGENTS.md) |
| Human README | [README.md](./README.md) |
| Design spec | [docs/superpowers/specs/2026-08-01-airplay2-rust-design.md](docs/superpowers/specs/2026-08-01-airplay2-rust-design.md) |
| Implementation plan | [docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md](docs/superpowers/plans/2026-08-01-airplay2-rust-implementation.md) |
| Acceptance checklist | [docs/superpowers/plans/acceptance-checklist.md](docs/superpowers/plans/acceptance-checklist.md) |

## Claude-specific notes

1. Prefer **small, testable changes**; run `cargo test -p <crate>` for the crate you touch.
2. For FairPlay / OmgHax work: **port from Java**, do not invent algorithms. Fixtures live under `crates/airplay-lib/tests/` and `resources/`.
3. Do not “fix” workspace `overflow-checks = false` without a full wrapping-arithmetic pass on HandGarble.
4. Live video UI requires the **gstreamer** feature; default dump player will not open a window.
5. If both this file and AGENTS.md disagree, **AGENTS.md wins** until they are reconciled.

## Session start checklist

- [ ] Skim [AGENTS.md](./AGENTS.md)
- [ ] Confirm branch / worktree (often `feature/airplay2-rust-impl` under `.worktrees/`)
- [ ] `cargo test --workspace` before claiming crypto or control changes are done
