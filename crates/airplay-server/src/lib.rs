//! AirPlay receiver server (control + media).

pub mod config;
pub mod consumer;
pub mod control;
pub mod media;
pub mod packet;
pub mod plist_util;
pub mod server;
pub mod session;

pub use config::AirPlayConfig;
pub use consumer::{AirPlayConsumer, PlaybackInfo, StreamGeneration};
pub use control::{
    classify_play_request, local_playlist_url, parse_action, parse_play_request,
    prepare_event_request, remote_playlist_url, rewrite_playlist, AppCompatibilityClass,
    ClassifiedPlay, ConnectionDirective, HandlerResult, MediaAction, MediaKind, MediaSupport,
    OutboundRequest, PlayRequest,
};
pub use server::AirPlayServer;
pub use session::{Session, SessionManager};
