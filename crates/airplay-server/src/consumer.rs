//! Media consumer callbacks for decoded AirPlay streams.

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};

/// Playback position report for HLS / media playlist paths.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackInfo {
    pub duration: f64,
    pub position: f64,
}

/// Sink for decrypted video/audio (and optional HLS) events from the server.
///
/// Implementors must be `Send + Sync` so the control and media tasks can share
/// a single `Arc<dyn AirPlayConsumer>`.
pub trait AirPlayConsumer: Send + Sync {
    fn on_video_format(&self, info: &VideoStreamInfo);
    fn on_video(&self, data: &[u8]);
    fn on_video_src_disconnect(&self);

    /// Stream pixel size from type-1 (SPS/PPS) headers — used for auto portrait rotate.
    /// Default no-op.
    fn on_video_size(&self, _width: u32, _height: u32) {}

    fn on_audio_format(&self, info: &AudioStreamInfo);
    fn on_audio(&self, data: &[u8]);
    fn on_audio_src_disconnect(&self);

    /// AirPlay sender volume in decibels (`0.0` = unity, `-144.0` = mute).
    /// Default no-op for consumers without audio playback.
    fn on_volume(&self, _volume_db: f64) {}

    /// Current AirPlay sender volume in decibels, when supported.
    fn volume(&self) -> Option<f64> {
        None
    }

    /// HLS / media playlist hooks — default no-ops.
    fn on_media_playlist(&self, _playlist_uri: &str) {}
    fn on_media_playlist_remove(&self) {}
    fn on_media_playlist_pause(&self) {}
    fn on_media_playlist_resume(&self) {}

    fn playback_info(&self) -> PlaybackInfo {
        PlaybackInfo {
            duration: 0.0,
            position: 0.0,
        }
    }
}
