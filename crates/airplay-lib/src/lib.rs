//! AirPlay protocol library: pairing, FairPlay, RTSP setup, decrypt, Bonjour helpers.

pub mod airplay;
pub mod crypto;
pub mod decrypt;
pub mod error;
pub mod fairplay;
pub mod pairing;
pub mod rtsp;
pub mod stream_info;

pub use airplay::AirPlay;
pub use decrypt::{FairPlayAudioDecryptor, FairPlayVideoDecryptor};
pub use error::{AirPlayError, Result};
pub use fairplay::FairPlay;
pub use pairing::Pairing;
pub use rtsp::Rtsp;
pub use stream_info::{
    AudioFormat, AudioStreamInfo, CompressionType, MediaStreamInfo, VideoStreamInfo,
};

pub fn workspace_smoke() -> &'static str {
    "airplay-lib"
}
