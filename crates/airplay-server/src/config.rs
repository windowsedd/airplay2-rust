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
        // Higher defaults → phone often picks higher encode bitrate/resolution.
        Self {
            server_name: "airplay2-rust".into(),
            width: 1920,
            height: 1080,
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
        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.fps, 60);
        assert_eq!(cfg.refresh_rate, 60);
    }
}
