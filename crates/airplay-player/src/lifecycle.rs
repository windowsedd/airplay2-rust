//! Stream-generation gates and idempotent player shutdown helpers.
//!
//! Used by player backends so TEARDOWN / disconnect from session N cannot stop
//! resources already claimed by session N+1, and so stop is safe to call twice.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// Active stream ownership keyed by a monotonic generation id.
#[derive(Debug, Default)]
pub struct GenerationGate {
    active: AtomicU64,
}

impl GenerationGate {
    pub const fn new() -> Self {
        Self {
            active: AtomicU64::new(0),
        }
    }

    /// Claim ownership for `generation`. Newer SETUP must call this before
    /// starting pipelines so late disconnects from older gens are ignored.
    pub fn claim(&self, generation: u64) {
        self.active.store(generation, Ordering::Release);
    }

    pub fn active(&self) -> u64 {
        self.active.load(Ordering::Acquire)
    }

    pub fn is_owner(&self, generation: u64) -> bool {
        generation != 0 && self.active() == generation
    }

    /// Returns true only when `generation` still owns the gate.
    pub fn stop_if_owner(&self, generation: u64) -> bool {
        if !self.is_owner(generation) {
            tracing::debug!(
                generation,
                active = self.active(),
                "ignoring stale disconnect from replaced session"
            );
            return false;
        }
        true
    }
}

/// Coarse player pipeline lifecycle for idempotent shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerState {
    Idle = 0,
    Starting = 1,
    Playing = 2,
    Stopping = 3,
    Stopped = 4,
}

impl PlayerState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Starting,
            2 => Self::Playing,
            3 => Self::Stopping,
            4 => Self::Stopped,
            _ => Self::Idle,
        }
    }
}

/// Atomic lifecycle flag: only one caller transitions into `Stopping`.
#[derive(Debug, Default)]
pub struct PlayerLifecycle {
    state: AtomicU8,
}

impl PlayerLifecycle {
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(PlayerState::Idle as u8),
        }
    }

    pub fn get(&self) -> PlayerState {
        PlayerState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn mark_starting(&self) {
        self.state
            .store(PlayerState::Starting as u8, Ordering::Release);
    }

    pub fn mark_playing(&self) {
        self.state
            .store(PlayerState::Playing as u8, Ordering::Release);
    }

    /// Returns `true` if this caller should perform shutdown work.
    pub fn begin_stop(&self) -> bool {
        loop {
            let current = self.state.load(Ordering::Acquire);
            match PlayerState::from_u8(current) {
                PlayerState::Stopping | PlayerState::Stopped | PlayerState::Idle => {
                    return false;
                }
                PlayerState::Starting | PlayerState::Playing => {
                    if self
                        .state
                        .compare_exchange_weak(
                            current,
                            PlayerState::Stopping as u8,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return true;
                    }
                }
            }
        }
    }

    pub fn mark_stopped(&self) {
        self.state
            .store(PlayerState::Stopped as u8, Ordering::Release);
    }

    /// Reset to idle so a later start can run (used when claiming a new generation).
    pub fn reset_for_start(&self) {
        self.state.store(PlayerState::Idle as u8, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn stale_generation_stop_is_ignored() {
        let gate = GenerationGate::new();
        gate.claim(1);
        assert!(gate.stop_if_owner(1));
        gate.claim(2);
        assert!(!gate.stop_if_owner(1));
        assert!(gate.stop_if_owner(2));
    }

    #[test]
    fn zero_generation_never_owns() {
        let gate = GenerationGate::new();
        assert!(!gate.is_owner(0));
        gate.claim(0);
        assert!(!gate.stop_if_owner(0));
    }

    #[test]
    fn begin_stop_is_idempotent_and_single_winner() {
        let life = Arc::new(PlayerLifecycle::new());
        life.mark_starting();
        life.mark_playing();

        let life2 = Arc::clone(&life);
        let t = thread::spawn(move || life2.begin_stop());
        let a = life.begin_stop();
        let b = t.join().expect("join");
        assert_eq!(a as u8 + b as u8, 1, "exactly one stopper wins");
        assert!(!life.begin_stop());
        life.mark_stopped();
        assert!(!life.begin_stop());
    }

    #[test]
    fn reset_allows_start_after_stop() {
        let life = PlayerLifecycle::new();
        life.mark_playing();
        assert!(life.begin_stop());
        life.mark_stopped();
        life.reset_for_start();
        life.mark_starting();
        life.mark_playing();
        assert!(life.begin_stop());
    }

    #[test]
    fn stale_stop_can_restore_playing_for_newer_owner() {
        // Simulates: gen1 wins begin_stop after gen2 already claimed Playing.
        let life = PlayerLifecycle::new();
        life.mark_playing(); // gen2
        assert!(life.begin_stop()); // stale gen1
        // Stale stopper must restore so gen2 can still stop later.
        life.mark_playing();
        assert!(life.begin_stop());
    }
}
