//! Fan-out consumer: send the same A/V events to multiple backends.
//!
//! Used by the default "auto" player so a live window can open via ffplay
//! and/or GStreamer while also writing `dump.h264` for debugging.

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::{AirPlayConsumer, PlaybackInfo};

/// Delivers each callback to every inner consumer (in order).
pub struct TeePlayer {
    inners: Vec<Box<dyn AirPlayConsumer>>,
}

impl TeePlayer {
    pub fn new(inners: Vec<Box<dyn AirPlayConsumer>>) -> Self {
        Self { inners }
    }
}

impl AirPlayConsumer for TeePlayer {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        for c in &self.inners {
            c.on_video_format(info);
        }
    }

    fn on_video(&self, data: &[u8]) {
        for c in &self.inners {
            c.on_video(data);
        }
    }

    fn on_video_src_disconnect(&self) {
        for c in &self.inners {
            c.on_video_src_disconnect();
        }
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        for c in &self.inners {
            c.on_audio_format(info);
        }
    }

    fn on_audio(&self, data: &[u8]) {
        for c in &self.inners {
            c.on_audio(data);
        }
    }

    fn on_audio_src_disconnect(&self) {
        for c in &self.inners {
            c.on_audio_src_disconnect();
        }
    }

    fn on_media_playlist(&self, playlist_uri: &str) {
        for c in &self.inners {
            c.on_media_playlist(playlist_uri);
        }
    }

    fn on_media_playlist_remove(&self) {
        for c in &self.inners {
            c.on_media_playlist_remove();
        }
    }

    fn on_media_playlist_pause(&self) {
        for c in &self.inners {
            c.on_media_playlist_pause();
        }
    }

    fn on_media_playlist_resume(&self) {
        for c in &self.inners {
            c.on_media_playlist_resume();
        }
    }

    fn playback_info(&self) -> PlaybackInfo {
        self.inners
            .first()
            .map(|c| c.playback_info())
            .unwrap_or(PlaybackInfo {
                duration: 0.0,
                position: 0.0,
            })
    }
}
