//! Player backends implementing airplay-server consumers.
//!
//! Backends are Cargo-feature gated:
//! - `h264-dump` (default): write raw H.264 to a file
//! - `gstreamer`: live window via system GStreamer 1.x
//! - `ffmpeg`: pipe H.264 to `ffplay` on PATH
//! - `vlc`: best-effort pipe H.264 to `vlc`/`cvlc` on PATH

#![cfg_attr(
    not(any(
        feature = "h264-dump",
        feature = "gstreamer",
        feature = "ffmpeg",
        feature = "vlc"
    )),
    allow(dead_code)
)]

#[cfg(feature = "h264-dump")]
mod h264_dump;

#[cfg(feature = "gstreamer")]
mod gstreamer_player;

#[cfg(feature = "ffmpeg")]
mod ffmpeg_player;

#[cfg(feature = "vlc")]
mod vlc_player;

#[cfg(feature = "h264-dump")]
pub use h264_dump::H264Dump;

#[cfg(feature = "gstreamer")]
pub use gstreamer_player::GStreamerPlayer;

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_player::FFmpegPlayer;

#[cfg(feature = "vlc")]
pub use vlc_player::VlcPlayer;
