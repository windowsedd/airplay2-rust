use thiserror::Error;

#[derive(Debug, Error)]
pub enum AirPlayError {
    #[error("pairing error: {0}")]
    Pairing(String),
    #[error("fairplay error: {0}")]
    FairPlay(String),
    #[error("rtsp setup error: {0}")]
    Rtsp(String),
    #[error("decrypt error: {0}")]
    Decrypt(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
}

pub type Result<T> = std::result::Result<T, AirPlayError>;
