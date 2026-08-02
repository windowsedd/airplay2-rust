//! Media consumer callbacks for decoded AirPlay streams.

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};

/// Monotonic stream generation assigned by the control server on each SETUP.
///
/// Disconnect / TEARDOWN callbacks carry the generation that owned the stream so
/// players can ignore stale cleanup after a newer session has claimed the sink.
pub type StreamGeneration = u64;

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
///
/// ## Session generations
///
/// `on_video_format` / `on_audio_format` receive a fresh [`StreamGeneration`].
/// Matching `on_*_src_disconnect` calls must only tear down resources if that
/// generation is still the active owner — never a newer stream.
pub trait AirPlayConsumer: Send + Sync {
    fn on_video_format(&self, info: &VideoStreamInfo, generation: StreamGeneration);
    fn on_video(&self, data: &[u8]);
    fn on_video_src_disconnect(&self, generation: StreamGeneration);

    /// Stream pixel size from type-1 (SPS/PPS) headers — used for auto portrait rotate.
    /// Default no-op.
    fn on_video_size(&self, _width: u32, _height: u32) {}

    fn on_audio_format(&self, info: &AudioStreamInfo, generation: StreamGeneration);
    fn on_audio(&self, data: &[u8]);
    fn on_audio_src_disconnect(&self, generation: StreamGeneration);

    /// AirPlay sender volume in decibels (`0.0` = maximum / 0 dB, **not** mute;
    /// values `<= -100` are treated as mute by system-volume sync).
    /// Default no-op for consumers without audio playback.
    fn on_volume(&self, _volume_db: f64) {}

    /// Explicit mute flag from HTTP `setProperty` (when present).
    /// Default no-op.
    fn on_mute(&self, _muted: bool) {}

    /// Current AirPlay sender volume in decibels, when supported.
    fn volume(&self) -> Option<f64> {
        None
    }

    /// HLS / media playlist hooks — default no-ops.
    fn on_media_playlist(&self, _playlist_uri: &str) {}
    fn on_media_playlist_remove(&self) {}
    fn on_media_playlist_pause(&self) {}
    fn on_media_playlist_resume(&self) {}
    /// Seek media playback to absolute position in seconds.
    fn on_media_playlist_seek(&self, _position_seconds: f64) {}
    /// Seek to a fraction of duration in `0.0..=1.0` when duration is known.
    fn on_media_playlist_seek_fraction(&self, _fraction: f64) {}
    /// Non-fatal media error (protected content, unsupported codec, etc.).
    /// Receiver must stay alive; default logs at warn via implementors.
    fn on_media_error(&self, _message: &str) {}

    fn playback_info(&self) -> PlaybackInfo {
        PlaybackInfo {
            duration: 0.0,
            position: 0.0,
        }
    }
}
