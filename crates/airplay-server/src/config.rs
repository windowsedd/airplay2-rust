//! Server configuration for the AirPlay receiver.

/// Configuration for the AirPlay receiver server.
#[derive(Debug, Clone)]
pub struct AirPlayConfig {
    /// Display / advertised server name.
    pub server_name: String,
    /// Advertised video width in pixels.
    pub width: u32,
    /// Advertised video height in pixels.
    pub height: u32,
    /// Advertised frames per second.
    pub fps: u32,
}

impl Default for AirPlayConfig {
    fn default() -> Self {
        Self {
            server_name: "airplay2-rust".into(),
            width: 1280,
            height: 720,
            fps: 24,
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
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);
        assert_eq!(cfg.fps, 24);
    }
}
