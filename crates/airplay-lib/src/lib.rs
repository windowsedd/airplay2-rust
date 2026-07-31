//! AirPlay protocol library: pairing, FairPlay, RTSP setup, decrypt, Bonjour helpers.

pub mod airplay;
pub mod bonjour;
pub mod crypto;
pub mod decrypt;
pub mod error;
pub mod fairplay;
pub mod pairing;
pub mod rtsp;
pub mod stream_info;

pub use airplay::AirPlay;
pub use bonjour::{
    airplay_txt_records, format_mac, raop_service_name, raop_txt_records, AirPlayBonjour,
    AIRPLAY_FEATURES, AIRPLAY_PK, AIRPLAY_SRCVERS,
};
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
