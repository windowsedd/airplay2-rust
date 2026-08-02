use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewMode {
    Quality,
    #[default]
    Balanced,
    LowLatency,
}

impl PreviewMode {
    pub fn options(self) -> PreviewOptions {
        match self {
            Self::Quality => PreviewOptions {
                queue_max_buffers: 8,
                queue_leaky: false,
                sink_sync: true,
            },
            Self::Balanced => PreviewOptions {
                queue_max_buffers: 3,
                queue_leaky: true,
                sink_sync: true,
            },
            // Keep sink_sync=true so video/audio share paced presentation.
            // Latency comes from a small leaky queue, not freerunning video
            // (which desyncs lips from audio on a separate pipeline).
            Self::LowLatency => PreviewOptions {
                queue_max_buffers: 2,
                queue_leaky: true,
                sink_sync: true,
            },
        }
    }
}

impl fmt::Display for PreviewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Quality => "quality",
            Self::Balanced => "balanced",
            Self::LowLatency => "low-latency",
        })
    }
}

impl FromStr for PreviewMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "quality" => Ok(Self::Quality),
            "balanced" => Ok(Self::Balanced),
            "low-latency" | "low_latency" => Ok(Self::LowLatency),
            other => Err(format!(
                "invalid preview mode '{other}' (expected quality, balanced, or low-latency)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewOptions {
    pub queue_max_buffers: u32,
    pub queue_leaky: bool,
    pub sink_sync: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderChoice {
    D3d11H264Dec,
    AvdecH264,
    DecodeBin,
}

impl DecoderChoice {
    pub const fn factory_name(self) -> &'static str {
        match self {
            Self::D3d11H264Dec => "d3d11h264dec",
            Self::AvdecH264 => "avdec_h264",
            Self::DecodeBin => "decodebin",
        }
    }

    pub const fn is_hardware(self) -> bool {
        matches!(self, Self::D3d11H264Dec)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkChoice {
    D3d11VideoSink,
    AutoVideoSink,
}

impl SinkChoice {
    pub const fn factory_name(self) -> &'static str {
        match self {
            Self::D3d11VideoSink => "d3d11videosink",
            Self::AutoVideoSink => "autovideosink",
        }
    }
}

pub fn decoder_candidates(hardware_decode: bool) -> Vec<DecoderChoice> {
    let mut choices = Vec::with_capacity(3);
    if hardware_decode {
        choices.push(DecoderChoice::D3d11H264Dec);
    }
    choices.push(DecoderChoice::AvdecH264);
    choices.push(DecoderChoice::DecodeBin);
    choices
}

pub fn sink_candidates(is_windows: bool) -> Vec<SinkChoice> {
    let mut choices = Vec::with_capacity(2);
    if is_windows {
        choices.push(SinkChoice::D3d11VideoSink);
    }
    choices.push(SinkChoice::AutoVideoSink);
    choices
}

pub fn select_decoder(
    hardware_decode: bool,
    mut available: impl FnMut(&str) -> bool,
) -> Option<DecoderChoice> {
    decoder_candidates(hardware_decode)
        .into_iter()
        .find(|choice| available(choice.factory_name()))
}

pub fn select_sink(
    is_windows: bool,
    mut available: impl FnMut(&str) -> bool,
) -> Option<SinkChoice> {
    sink_candidates(is_windows)
        .into_iter()
        .find(|choice| available(choice.factory_name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_modes_map_to_bounded_queue_options() {
        assert_eq!(
            PreviewMode::Quality.options(),
            PreviewOptions {
                queue_max_buffers: 8,
                queue_leaky: false,
                sink_sync: true,
            }
        );
        assert_eq!(
            PreviewMode::Balanced.options(),
            PreviewOptions {
                queue_max_buffers: 3,
                queue_leaky: true,
                sink_sync: true,
            }
        );
        assert_eq!(
            PreviewMode::LowLatency.options(),
            PreviewOptions {
                queue_max_buffers: 2,
                queue_leaky: true,
                sink_sync: true,
            }
        );
    }

    #[test]
    fn parses_only_supported_preview_modes() {
        assert_eq!("quality".parse(), Ok(PreviewMode::Quality));
        assert_eq!("balanced".parse(), Ok(PreviewMode::Balanced));
        assert_eq!("low-latency".parse(), Ok(PreviewMode::LowLatency));
        assert!("fast-ish".parse::<PreviewMode>().is_err());
    }

    #[test]
    fn decoder_order_respects_hardware_toggle() {
        assert_eq!(
            decoder_candidates(true),
            vec![
                DecoderChoice::D3d11H264Dec,
                DecoderChoice::AvdecH264,
                DecoderChoice::DecodeBin,
            ]
        );
        assert_eq!(
            decoder_candidates(false),
            vec![DecoderChoice::AvdecH264, DecoderChoice::DecodeBin]
        );
    }

    #[test]
    fn windows_sink_order_prefers_d3d11() {
        assert_eq!(
            sink_candidates(true),
            vec![SinkChoice::D3d11VideoSink, SinkChoice::AutoVideoSink]
        );
        assert_eq!(sink_candidates(false), vec![SinkChoice::AutoVideoSink]);
    }

    #[test]
    fn selection_skips_unavailable_factories_in_order() {
        let available = ["avdec_h264", "decodebin", "autovideosink"];
        assert_eq!(
            select_decoder(true, |name| available.contains(&name)),
            Some(DecoderChoice::AvdecH264)
        );
        assert_eq!(
            select_sink(true, |name| available.contains(&name)),
            Some(SinkChoice::AutoVideoSink)
        );
    }
}
