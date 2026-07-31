//! AirPlay receiver server (control + media).

pub mod config;
pub mod consumer;
pub mod control;
pub mod plist_util;
pub mod server;
pub mod session;

pub use config::AirPlayConfig;
pub use consumer::{AirPlayConsumer, PlaybackInfo};
pub use server::AirPlayServer;
pub use session::{Session, SessionManager};
