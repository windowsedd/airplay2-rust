//! Media packet framing (video TCP headers, audio UDP headers).

pub mod audio;
pub mod video;

pub use audio::{parse_audio_packet, AudioPacket};
pub use video::{
    parse_video_header, prepare_picture_nal_units, prepare_sps_pps_nal_units, VideoHeader,
    VIDEO_HEADER_LEN,
};
