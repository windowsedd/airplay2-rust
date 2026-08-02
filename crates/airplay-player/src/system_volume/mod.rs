//! Platform system-volume control for AirPlay volume sync.
//!
//! AirPlay protocol volume is in **decibels**:
//! - `0.0`  = maximum (0 dB), **not** mute
//! - `-144` = typical mute sentinel from iOS
//! - intermediate negative dB = attenuated level
//!
//! When `VolumeSyncMode::System` is active, AirPlay volume drives the OS master
//! output level and the GStreamer pipeline gain stays at unity.

mod mapping;
#[cfg(windows)]
mod windows;

// Public mapping API (used by tests and callers outside this module).
#[allow(unused_imports)] // re-exported for external crates / docs
pub use mapping::{
    airplay_db_is_mute, airplay_db_to_amplitude, clamp_db_to_range, AIRPLAY_MUTE_DB_THRESHOLD,
};

use std::sync::Arc;

/// How AirPlay volume updates are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VolumeSyncMode {
    /// Drive the OS default render endpoint master volume.
    System,
    /// Drive only the receiver GStreamer `volume` element (default).
    #[default]
    Player,
    /// Ignore AirPlay volume changes.
    Disabled,
}

impl VolumeSyncMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "system" | "os" | "windows" => Self::System,
            "disabled" | "off" | "none" | "false" | "0" => Self::Disabled,
            _ => Self::Player, // "player" | "gstreamer" | "pipeline" | default
        }
    }
}

/// Platform master-volume sink.
pub trait SystemVolumeController: Send + Sync {
    /// Apply AirPlay volume in decibels (`0.0` = max, `<= -100` ≈ mute).
    fn set_airplay_volume_db(&self, db: f64) -> Result<(), String>;

    /// Explicit mute flag (e.g. HTTP setProperty `muted`).
    fn set_muted(&self, muted: bool) -> Result<(), String>;
}

/// No-op controller used on non-Windows builds and in unit tests.
#[derive(Debug, Default)]
pub struct NoopSystemVolumeController;

impl SystemVolumeController for NoopSystemVolumeController {
    fn set_airplay_volume_db(&self, db: f64) -> Result<(), String> {
        tracing::debug!(db, "noop system volume (AirPlay dB ignored)");
        Ok(())
    }

    fn set_muted(&self, muted: bool) -> Result<(), String> {
        tracing::debug!(muted, "noop system mute");
        Ok(())
    }
}

/// Build the best available controller for this platform.
pub fn create_system_volume_controller() -> Arc<dyn SystemVolumeController> {
    #[cfg(windows)]
    {
        match windows::WindowsSystemVolumeController::new() {
            Ok(c) => {
                tracing::info!("Windows system volume controller ready (Core Audio endpoint)");
                Arc::new(c)
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Windows system volume unavailable; AirPlay volume will not change OS master level"
                );
                Arc::new(NoopSystemVolumeController)
            }
        }
    }
    #[cfg(not(windows))]
    {
        Arc::new(NoopSystemVolumeController)
    }
}

/// Recording mock for tests.
#[derive(Debug, Default)]
pub struct RecordingSystemVolumeController {
    pub volumes: std::sync::Mutex<Vec<f64>>,
    pub mutes: std::sync::Mutex<Vec<bool>>,
    pub fail: std::sync::atomic::AtomicBool,
}

impl RecordingSystemVolumeController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn volumes(&self) -> Vec<f64> {
        self.volumes.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn mutes(&self) -> Vec<bool> {
        self.mutes.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl SystemVolumeController for RecordingSystemVolumeController {
    fn set_airplay_volume_db(&self, db: f64) -> Result<(), String> {
        if self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("mock volume failure".into());
        }
        if let Ok(mut g) = self.volumes.lock() {
            g.push(db);
        }
        Ok(())
    }

    fn set_muted(&self, muted: bool) -> Result<(), String> {
        if self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("mock mute failure".into());
        }
        if let Ok(mut g) = self.mutes.lock() {
            g.push(muted);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_sync_mode_parse() {
        assert_eq!(VolumeSyncMode::parse("system"), VolumeSyncMode::System);
        assert_eq!(VolumeSyncMode::parse("player"), VolumeSyncMode::Player);
        assert_eq!(VolumeSyncMode::parse("disabled"), VolumeSyncMode::Disabled);
        assert_eq!(VolumeSyncMode::parse(""), VolumeSyncMode::Player);
        assert_eq!(VolumeSyncMode::default(), VolumeSyncMode::Player);
    }

    #[test]
    fn recording_controller_stores_updates() {
        let c = RecordingSystemVolumeController::new();
        c.set_airplay_volume_db(0.0).unwrap();
        c.set_airplay_volume_db(-20.0).unwrap();
        c.set_muted(true).unwrap();
        assert_eq!(c.volumes(), vec![0.0, -20.0]);
        assert_eq!(c.mutes(), vec![true]);
    }

    #[test]
    fn recording_failure_does_not_panic() {
        let c = RecordingSystemVolumeController::new();
        c.fail.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(c.set_airplay_volume_db(-10.0).is_err());
        assert!(c.set_muted(false).is_err());
    }
}
