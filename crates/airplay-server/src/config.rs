//! Server configuration for the AirPlay receiver.

/// Configuration for the AirPlay receiver server.
///
/// `fps` is sent to the iPhone as display **`maxFPS`** (an upper bound the sender
/// *may* use). The phone still chooses the real encode rate — often lower when
/// the UI is static, the device is warm, or Wi‑Fi is weak. This receiver never
/// generates frames; it only decrypts what the phone sends.
#[derive(Debug, Clone)]
pub struct AirPlayConfig {
    /// Display / advertised server name (Screen Mirroring list).
    pub server_name: String,
    /// Advertised video width in pixels.
    pub width: u32,
    /// Advertised video height in pixels.
    pub height: u32,
    /// Advertised max stream FPS (`maxFPS` in RTSP `/info`). Typical 24–60.
    pub fps: u32,
    /// Advertised display refresh rate (`refreshRate`). Usually 60.
    pub refresh_rate: u32,
}

impl Default for AirPlayConfig {
    fn default() -> Self {
        // High-quality portrait advertise (≈ phone FHD+); app presets may override.
        Self {
            server_name: "airplay2-rust".into(),
            width: 1170,
            height: 2532,
            fps: 60,
            refresh_rate: 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = AirPlayConfig::default();
        assert_eq!(cfg.server_name, "airplay2-rust");
        assert_eq!(cfg.width, 1170);
        assert_eq!(cfg.height, 2532);
        assert_eq!(cfg.fps, 60);
        assert_eq!(cfg.refresh_rate, 60);
        assert!(
            cfg.height > cfg.width,
            "default advertise is portrait (home UI)"
        );
    }
}
