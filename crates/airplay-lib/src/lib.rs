//! AirPlay protocol library: pairing, FairPlay, RTSP setup, decrypt, Bonjour helpers.

pub mod airplay;
pub mod error;
pub mod pairing;
pub mod stream_info;

pub use airplay::AirPlay;
pub use error::{AirPlayError, Result};
pub use pairing::Pairing;
pub use stream_info::{
    AudioFormat, AudioStreamInfo, CompressionType, MediaStreamInfo, VideoStreamInfo,
};

pub fn workspace_smoke() -> &'static str {
    "airplay-lib"
}
