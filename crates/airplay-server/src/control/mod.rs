//! RTSP/HTTP control channel: framing + request handlers.

pub mod codec;
pub mod handler;
pub mod media_protocol;
pub mod playlist;

pub use codec::{
    read_request, read_response, write_response, ControlRequest, ControlResponse,
    ControlResponseHead, OutboundRequest,
};
pub use handler::{ConnectionDirective, ControlHandler, HandlerResult};
pub use media_protocol::{
    classify_play_request, parse_action, parse_play_request, parse_rate_value,
    parse_scrub_position, playlist_looks_protected, prepare_event_request, redact_media_url,
    AppCompatibilityClass, ClassifiedPlay, MediaAction, MediaKind, MediaProtocolError,
    MediaSupport, PlayRequest,
};
pub use playlist::{local_playlist_url, remote_playlist_url, rewrite_playlist, PlaylistError};
