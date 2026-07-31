//! Media / video / audio stream info types (ported from Java).

use crate::error::{AirPlayError, Result};

/// Compression type codes used in AirPlay audio setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum CompressionType {
    Lpcm = 1,
    Alac = 2,
    Aac = 4,
    AacEld = 8,
    Opus = 32,
}

impl CompressionType {
    pub fn from_code(code: u64) -> Result<Self> {
        match code {
            1 => Ok(Self::Lpcm),
            2 => Ok(Self::Alac),
            4 => Ok(Self::Aac),
            8 => Ok(Self::AacEld),
            32 => Ok(Self::Opus),
            other => Err(AirPlayError::InvalidState(format!(
                "unknown compression type code: {other:#x}"
            ))),
        }
    }

    pub fn code(self) -> u64 {
        self as u64
    }
}

/// Audio format codes used in AirPlay (u64; some values exceed u32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioFormat {
    /// PCM_8000_16_1 (0x4)
    Pcm8000_16_1,
    /// PCM_8000_16_2 (0x8)
    Pcm8000_16_2,
    /// PCM_16000_16_1 (0x10)
    Pcm16000_16_1,
    /// PCM_16000_16_2 (0x20)
    Pcm16000_16_2,
    /// PCM_24000_16_1 (0x40)
    Pcm24000_16_1,
    /// PCM_24000_16_2 (0x80)
    Pcm24000_16_2,
    /// PCM_32000_16_1 (0x100)
    Pcm32000_16_1,
    /// PCM_32000_16_2 (0x200)
    Pcm32000_16_2,
    /// PCM_44100_16_1 (0x400)
    Pcm44100_16_1,
    /// PCM_44100_16_2 (0x800)
    Pcm44100_16_2,
    /// PCM_44100_24_1 (0x1000)
    Pcm44100_24_1,
    /// PCM_44100_24_2 (0x2000)
    Pcm44100_24_2,
    /// PCM_48000_16_1 (0x4000)
    Pcm48000_16_1,
    /// PCM_48000_16_2 (0x8000)
    Pcm48000_16_2,
    /// PCM_48000_24_1 (0x10000)
    Pcm48000_24_1,
    /// PCM_48000_24_2 (0x20000)
    Pcm48000_24_2,
    /// ALAC_44100_16_2 (0x40000)
    Alac44100_16_2,
    /// ALAC_44100_24_2 (0x80000)
    Alac44100_24_2,
    /// ALAC_48000_16_2 (0x100000)
    Alac48000_16_2,
    /// ALAC_48000_24_2 (0x200000)
    Alac48000_24_2,
    /// AAC_LC_44100_2 (0x400000)
    AacLc44100_2,
    /// AAC_LC_48000_2 (0x800000)
    AacLc48000_2,
    /// AAC_ELD_44100_2 (0x1000000)
    AacEld44100_2,
    /// AAC_ELD_48000_2 (0x2000000)
    AacEld48000_2,
    /// AAC_ELD_16000_1 (0x4000000)
    AacEld16000_1,
    /// AAC_ELD_24000_1 (0x8000000)
    AacEld24000_1,
    /// OPUS_16000_1 (0x10000000)
    Opus16000_1,
    /// OPUS_24000_1 (0x20000000)
    Opus24000_1,
    /// OPUS_48000_1 (0x40000000)
    Opus48000_1,
    /// AAC_ELD_44100_1 (0x80000000)
    AacEld44100_1,
    /// AAC_ELD_48000_1 (0x100000000)
    AacEld48000_1,
}

impl AudioFormat {
    pub fn from_code(code: u64) -> Result<Self> {
        match code {
            0x4 => Ok(Self::Pcm8000_16_1),
            0x8 => Ok(Self::Pcm8000_16_2),
            0x10 => Ok(Self::Pcm16000_16_1),
            0x20 => Ok(Self::Pcm16000_16_2),
            0x40 => Ok(Self::Pcm24000_16_1),
            0x80 => Ok(Self::Pcm24000_16_2),
            0x100 => Ok(Self::Pcm32000_16_1),
            0x200 => Ok(Self::Pcm32000_16_2),
            0x400 => Ok(Self::Pcm44100_16_1),
            0x800 => Ok(Self::Pcm44100_16_2),
            0x1000 => Ok(Self::Pcm44100_24_1),
            0x2000 => Ok(Self::Pcm44100_24_2),
            0x4000 => Ok(Self::Pcm48000_16_1),
            0x8000 => Ok(Self::Pcm48000_16_2),
            0x10000 => Ok(Self::Pcm48000_24_1),
            0x20000 => Ok(Self::Pcm48000_24_2),
            0x40000 => Ok(Self::Alac44100_16_2),
            0x80000 => Ok(Self::Alac44100_24_2),
            0x100000 => Ok(Self::Alac48000_16_2),
            0x200000 => Ok(Self::Alac48000_24_2),
            0x400000 => Ok(Self::AacLc44100_2),
            0x800000 => Ok(Self::AacLc48000_2),
            0x1000000 => Ok(Self::AacEld44100_2),
            0x2000000 => Ok(Self::AacEld48000_2),
            0x4000000 => Ok(Self::AacEld16000_1),
            0x8000000 => Ok(Self::AacEld24000_1),
            0x10000000 => Ok(Self::Opus16000_1),
            0x20000000 => Ok(Self::Opus24000_1),
            0x40000000 => Ok(Self::Opus48000_1),
            0x80000000 => Ok(Self::AacEld44100_1),
            0x100000000 => Ok(Self::AacEld48000_1),
            other => Err(AirPlayError::InvalidState(format!(
                "unknown audio format code: {other:#x}"
            ))),
        }
    }

    pub fn code(self) -> u64 {
        match self {
            Self::Pcm8000_16_1 => 0x4,
            Self::Pcm8000_16_2 => 0x8,
            Self::Pcm16000_16_1 => 0x10,
            Self::Pcm16000_16_2 => 0x20,
            Self::Pcm24000_16_1 => 0x40,
            Self::Pcm24000_16_2 => 0x80,
            Self::Pcm32000_16_1 => 0x100,
            Self::Pcm32000_16_2 => 0x200,
            Self::Pcm44100_16_1 => 0x400,
            Self::Pcm44100_16_2 => 0x800,
            Self::Pcm44100_24_1 => 0x1000,
            Self::Pcm44100_24_2 => 0x2000,
            Self::Pcm48000_16_1 => 0x4000,
            Self::Pcm48000_16_2 => 0x8000,
            Self::Pcm48000_24_1 => 0x10000,
            Self::Pcm48000_24_2 => 0x20000,
            Self::Alac44100_16_2 => 0x40000,
            Self::Alac44100_24_2 => 0x80000,
            Self::Alac48000_16_2 => 0x100000,
            Self::Alac48000_24_2 => 0x200000,
            Self::AacLc44100_2 => 0x400000,
            Self::AacLc48000_2 => 0x800000,
            Self::AacEld44100_2 => 0x1000000,
            Self::AacEld48000_2 => 0x2000000,
            Self::AacEld16000_1 => 0x4000000,
            Self::AacEld24000_1 => 0x8000000,
            Self::Opus16000_1 => 0x10000000,
            Self::Opus24000_1 => 0x20000000,
            Self::Opus48000_1 => 0x40000000,
            Self::AacEld44100_1 => 0x80000000,
            Self::AacEld48000_1 => 0x100000000,
        }
    }
}

/// Video stream setup info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStreamInfo {
    pub stream_connection_id: String,
}

impl VideoStreamInfo {
    pub fn new(stream_connection_id: impl Into<String>) -> Self {
        Self {
            stream_connection_id: stream_connection_id.into(),
        }
    }
}

/// Audio stream setup info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStreamInfo {
    pub compression_type: CompressionType,
    pub audio_format: AudioFormat,
    pub samples_per_frame: i32,
}

impl AudioStreamInfo {
    pub fn new(
        compression_type: CompressionType,
        audio_format: AudioFormat,
        samples_per_frame: i32,
    ) -> Self {
        Self {
            compression_type,
            audio_format,
            samples_per_frame,
        }
    }
}

/// Media stream info: either video or audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaStreamInfo {
    Video(VideoStreamInfo),
    Audio(AudioStreamInfo),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_type_from_code_alac() {
        assert_eq!(
            CompressionType::from_code(2).unwrap(),
            CompressionType::Alac
        );
        assert_eq!(CompressionType::Alac.code(), 2);
    }

    #[test]
    fn audio_format_from_code_alac_48000_16_2() {
        assert_eq!(
            AudioFormat::from_code(0x100000).unwrap(),
            AudioFormat::Alac48000_16_2
        );
        assert_eq!(AudioFormat::Alac48000_16_2.code(), 0x100000);
    }

    #[test]
    fn unknown_compression_type_returns_err() {
        assert!(CompressionType::from_code(99).is_err());
    }

    #[test]
    fn unknown_audio_format_returns_err() {
        assert!(AudioFormat::from_code(0x1).is_err());
    }

    #[test]
    fn audio_format_u64_codes_roundtrip() {
        // Codes that require u64 (bit 31 and beyond)
        let formats = [
            AudioFormat::AacEld44100_1, // 0x80000000
            AudioFormat::AacEld48000_1, // 0x100000000
        ];
        for fmt in formats {
            let code = fmt.code();
            assert_eq!(AudioFormat::from_code(code).unwrap(), fmt);
        }
    }
}
