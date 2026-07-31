//! RTSP/HTTP control channel: framing + request handlers.

pub mod codec;
pub mod handler;

pub use codec::{read_request, write_response, ControlRequest, ControlResponse};
pub use handler::ControlHandler;
