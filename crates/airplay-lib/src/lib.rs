//! AirPlay protocol library: pairing, FairPlay, RTSP setup, decrypt, Bonjour helpers.

pub mod error;
pub mod stream_info;

pub use error::{AirPlayError, Result};
pub use stream_info::{
    AudioFormat, AudioStreamInfo, CompressionType, MediaStreamInfo, VideoStreamInfo,
};

pub fn workspace_smoke() -> &'static str {
    "airplay-lib"
}
