//! Player backends implementing airplay-server consumers.

#![cfg_attr(not(feature = "h264-dump"), allow(dead_code))]

#[cfg(feature = "h264-dump")]
mod h264_dump;

#[cfg(feature = "h264-dump")]
pub use h264_dump::H264Dump;
