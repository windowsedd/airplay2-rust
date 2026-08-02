//! AirPlay dB ↔ amplitude / mute helpers (pure, no OS APIs).

/// iOS commonly sends approximately this level (or lower) for mute.
pub const AIRPLAY_MUTE_DB_THRESHOLD: f64 = -100.0;

/// True when AirPlay dB should be treated as mute.
///
/// **Important:** AirPlay `0.0` means **maximum** volume (0 dB), not mute.
pub fn airplay_db_is_mute(db: f64) -> bool {
    !db.is_finite() || db <= AIRPLAY_MUTE_DB_THRESHOLD
}

/// Convert AirPlay dB to linear amplitude (`0.0..=1.0`).
///
/// `0.0 dB → 1.0`, `-20 dB → 0.1`, mute threshold → `0.0`.
pub fn airplay_db_to_amplitude(db: f64) -> f64 {
    if airplay_db_is_mute(db) {
        return 0.0;
    }
    let db = db.clamp(-144.0, 0.0);
    10_f64.powf(db / 20.0).clamp(0.0, 1.0)
}

/// Clamp AirPlay dB into a Windows endpoint volume range.
pub fn clamp_db_to_range(db: f64, min_db: f64, max_db: f64) -> f64 {
    if !db.is_finite() {
        return min_db;
    }
    db.clamp(min_db.min(max_db), max_db.max(min_db))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_db_is_maximum_not_mute() {
        assert!(!airplay_db_is_mute(0.0));
        assert!((airplay_db_to_amplitude(0.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn mute_sentinel_is_mute() {
        assert!(airplay_db_is_mute(-144.0));
        assert!(airplay_db_is_mute(-100.0));
        assert!(!airplay_db_is_mute(-99.9));
        assert_eq!(airplay_db_to_amplitude(-144.0), 0.0);
    }

    #[test]
    fn low_non_muted_volume() {
        assert!(!airplay_db_is_mute(-30.0));
        let a = airplay_db_to_amplitude(-30.0);
        assert!((a - 0.031622776).abs() < 1e-6);
    }

    #[test]
    fn clamp_to_endpoint_range() {
        assert_eq!(clamp_db_to_range(-200.0, -96.0, 0.0), -96.0);
        assert_eq!(clamp_db_to_range(6.0, -96.0, 0.0), 0.0);
        assert_eq!(clamp_db_to_range(-20.0, -96.0, 0.0), -20.0);
    }
}
