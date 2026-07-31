//! AirPlay sender / discovery client.
//!
//! - [`discovery`]: mDNS browse for `_airplay._tcp`
//! - [`control`]: RTSP control channel (info, pair-setup, pair-verify)
//! - [`encrypt`]: FairPlay video AES-CTR encryptor (sender path)

pub mod control;
pub mod discovery;
pub mod encrypt;

pub use control::{format_request, ControlClient, ControlResponse};
pub use discovery::{browse_airplay, browse_airplay_blocking, AirPlayService, AIRPLAY_SERVICE_TYPE};
pub use encrypt::FairPlayVideoEncryptor;

/// Re-export library result type for callers.
pub use airplay_lib::{AirPlayError, Result};
