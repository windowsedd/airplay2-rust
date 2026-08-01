//! Video packet header parse and NAL unit conversion (AVCC → Annex-B).
//!
//! Framing matches Java `VideoDecoder` / `VideoHandler`.
//! Size fields on type-1 (SPS/PPS) headers match UxPlay / RPiPlay layout.

/// Fixed video stream header size (bytes).
pub const VIDEO_HEADER_LEN: usize = 128;

/// Invalid AVCC picture framing. Conversion validates the complete payload
/// before replacing any length prefix, so errors never leak partial Annex-B.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PictureNalError {
    #[error("empty picture payload")]
    Empty,
    #[error("incomplete four-byte NAL length")]
    IncompleteLength,
    #[error("zero-length NAL unit")]
    ZeroLength,
    #[error("truncated NAL unit: declared {declared} bytes, only {remaining} remain")]
    Truncated { declared: usize, remaining: usize },
}

/// Parsed fields from the 128-byte video header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoHeader {
    /// Payload body length in bytes.
    pub payload_size: u32,
    /// Payload type (low 8 bits of the u16 LE at offset 4).
    ///
    /// - `0` — encrypted picture (decrypt + AVCC→Annex-B)
    /// - `1` — SPS/PPS parameter sets (no decrypt)
    /// - other — skip body
    pub payload_type: u8,
}

/// Image size carried in type-1 (SPS/PPS) 128-byte headers.
///
/// Layout (IEEE-754 `f32` little-endian), as reverse-engineered by UxPlay:
/// - 16: `width_source`, 20: `height_source`
/// - 40 / 44: source size (often duplicates of 16/20)
/// - 56: stream `width`, 60: stream `height`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoSize {
    pub width_source: f32,
    pub height_source: f32,
    pub width: f32,
    pub height: f32,
}

impl VideoSize {
    /// Rounded stream width in pixels (0 if invalid).
    pub fn width_px(&self) -> u32 {
        if self.width.is_finite() && self.width > 0.0 {
            self.width.round() as u32
        } else {
            0
        }
    }

    /// Rounded stream height in pixels (0 if invalid).
    pub fn height_px(&self) -> u32 {
        if self.height.is_finite() && self.height > 0.0 {
            self.height.round() as u32
        } else {
            0
        }
    }

    /// True when the stream (or source) reports portrait (taller than wide).
    pub fn is_portrait(&self) -> bool {
        let w = if self.width > 0.0 {
            self.width
        } else {
            self.width_source
        };
        let h = if self.height > 0.0 {
            self.height
        } else {
            self.height_source
        };
        h > w && w > 0.0 && h > 0.0
    }
}

/// Parse the first 6 meaningful bytes of a 128-byte video header.
///
/// Layout (little-endian):
/// - offset 0: `payload_size` u32 LE
/// - offset 4: `payload_type` u16 LE, use `& 0xff`
pub fn parse_video_header(header: &[u8]) -> Option<VideoHeader> {
    if header.len() < 6 {
        return None;
    }
    let payload_size = u32::from_le_bytes(header[0..4].try_into().ok()?);
    let payload_type_u16 = u16::from_le_bytes(header[4..6].try_into().ok()?);
    let payload_type = (payload_type_u16 & 0xff) as u8;
    Some(VideoHeader {
        payload_size,
        payload_type,
    })
}

/// Parse image size floats from a full 128-byte type-1 video header.
pub fn parse_video_size(header: &[u8]) -> Option<VideoSize> {
    if header.len() < 64 {
        return None;
    }
    let f = |off: usize| -> Option<f32> {
        let bytes: [u8; 4] = header[off..off + 4].try_into().ok()?;
        Some(f32::from_le_bytes(bytes))
    };
    let width_source = f(16)?;
    let height_source = f(20)?;
    // Prefer stream size at 56/60; fall back to source at 16/20 or 40/44.
    let mut width = f(56)?;
    let mut height = f(60)?;
    if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        width = f(40).unwrap_or(width_source);
        height = f(44).unwrap_or(height_source);
    }
    if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        width = width_source;
        height = height_source;
    }
    if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        return None;
    }
    Some(VideoSize {
        width_source,
        height_source,
        width,
        height,
    })
}

/// Validate and convert AVCC length-prefixed NAL units to Annex-B start codes.
///
/// For each complete unit: read big-endian `nalu_size` at `idx`, replace the
/// 4-byte length with `00 00 00 01`, then advance by `nalu_size + 4`.
///
/// The complete payload is validated before mutation. Returns the NAL count;
/// malformed input is left unchanged.
pub fn prepare_picture_nal_units(payload: &mut [u8]) -> Result<usize, PictureNalError> {
    if payload.is_empty() {
        return Err(PictureNalError::Empty);
    }

    let mut length_offsets = Vec::new();
    let mut idx = 0usize;
    while idx < payload.len() {
        if payload.len() - idx < 4 {
            return Err(PictureNalError::IncompleteLength);
        }
        let nalu_size = u32::from_be_bytes([
            payload[idx],
            payload[idx + 1],
            payload[idx + 2],
            payload[idx + 3],
        ]) as usize;

        if nalu_size == 0 {
            return Err(PictureNalError::ZeroLength);
        }
        let remaining = payload.len() - idx - 4;
        if nalu_size > remaining {
            return Err(PictureNalError::Truncated {
                declared: nalu_size,
                remaining,
            });
        }
        length_offsets.push(idx);
        idx += nalu_size + 4;
    }

    for idx in &length_offsets {
        payload[*idx..*idx + 4].copy_from_slice(&[0, 0, 0, 1]);
    }
    Ok(length_offsets.len())
}

/// Parse SPS/PPS from a type-1 payload and emit Annex-B NAL units.
///
/// Layout after 6-byte skip:
/// - u16 BE `sps_len`, `sps` bytes
/// - 1 byte (PPS count, skipped)
/// - u16 BE `pps_len`, `pps` bytes
///
/// Output: `00 00 00 01` + sps + `00 00 00 01` + pps.
pub fn prepare_sps_pps_nal_units(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 6 {
        return None;
    }
    let mut idx = 6usize;

    if idx + 2 > payload.len() {
        return None;
    }
    let sps_len = u16::from_be_bytes([payload[idx], payload[idx + 1]]) as usize;
    idx += 2;
    if idx + sps_len > payload.len() {
        return None;
    }
    let sps = &payload[idx..idx + sps_len];
    idx += sps_len;

    // PPS count (1 byte)
    if idx + 1 > payload.len() {
        return None;
    }
    idx += 1;

    if idx + 2 > payload.len() {
        return None;
    }
    let pps_len = u16::from_be_bytes([payload[idx], payload[idx + 1]]) as usize;
    idx += 2;
    if idx + pps_len > payload.len() {
        return None;
    }
    let pps = &payload[idx..idx + pps_len];

    let mut out = Vec::with_capacity(8 + sps_len + pps_len);
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(sps);
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(pps);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_header_type_1() {
        let mut header = [0u8; VIDEO_HEADER_LEN];
        // payload_size = 20 LE
        header[0..4].copy_from_slice(&20u32.to_le_bytes());
        // payload_type = 1 LE (low 8 bits)
        header[4..6].copy_from_slice(&1u16.to_le_bytes());

        let h = parse_video_header(&header).expect("header");
        assert_eq!(h.payload_size, 20);
        assert_eq!(h.payload_type, 1);
    }

    #[test]
    fn parse_header_type_uses_low_8_bits() {
        let mut header = [0u8; 8];
        header[0..4].copy_from_slice(&100u32.to_le_bytes());
        // 0x0100 LE bytes = [0x00, 0x01] → value 0x0100, low 8 bits = 0
        header[4] = 0x00;
        header[5] = 0x01;
        let h = parse_video_header(&header).unwrap();
        assert_eq!(h.payload_type, 0);
    }

    #[test]
    fn parse_header_too_short() {
        assert!(parse_video_header(&[0u8; 5]).is_none());
    }

    #[test]
    fn annex_b_conversion_two_nalus() {
        // [len=3][aa bb cc][len=2][dd ee]
        let mut payload = vec![
            0x00, 0x00, 0x00, 0x03, 0xaa, 0xbb, 0xcc, //
            0x00, 0x00, 0x00, 0x02, 0xdd, 0xee,
        ];
        assert_eq!(prepare_picture_nal_units(&mut payload), Ok(2));
        assert_eq!(
            payload,
            vec![
                0x00, 0x00, 0x00, 0x01, 0xaa, 0xbb, 0xcc, //
                0x00, 0x00, 0x00, 0x01, 0xdd, 0xee,
            ]
        );
    }

    #[test]
    fn truncated_picture_is_rejected_without_partial_mutation() {
        let original = vec![
            0x00, 0x00, 0x00, 0x02, 0xaa, 0xbb, //
            0x00, 0x00, 0x00, 0x05, 0xcc, // truncated nalu (claims 5, has 1)
        ];
        let mut payload = original.clone();
        assert!(matches!(
            prepare_picture_nal_units(&mut payload),
            Err(PictureNalError::Truncated { .. })
        ));
        assert_eq!(payload, original);
    }

    #[test]
    fn incomplete_length_and_zero_length_are_rejected() {
        assert_eq!(
            prepare_picture_nal_units(&mut [0, 0, 0]),
            Err(PictureNalError::IncompleteLength)
        );
        assert_eq!(
            prepare_picture_nal_units(&mut [0, 0, 0, 0]),
            Err(PictureNalError::ZeroLength)
        );
    }

    #[test]
    fn prepare_sps_pps_emits_annex_b() {
        // 6-byte skip prefix + sps_len=2 + sps + pps_count + pps_len=1 + pps
        let mut payload = vec![0u8; 6];
        payload.extend_from_slice(&2u16.to_be_bytes());
        payload.extend_from_slice(&[0x67, 0x42]); // fake SPS
        payload.push(1); // pps count
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.push(0x68); // fake PPS

        let out = prepare_sps_pps_nal_units(&payload).expect("sps/pps");
        assert_eq!(out, vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68]);
    }

    #[test]
    fn parse_video_size_portrait() {
        let mut header = [0u8; VIDEO_HEADER_LEN];
        header[16..20].copy_from_slice(&1080f32.to_le_bytes());
        header[20..24].copy_from_slice(&1920f32.to_le_bytes());
        header[40..44].copy_from_slice(&1080f32.to_le_bytes());
        header[44..48].copy_from_slice(&1920f32.to_le_bytes());
        header[56..60].copy_from_slice(&1080f32.to_le_bytes());
        header[60..64].copy_from_slice(&1920f32.to_le_bytes());
        let s = parse_video_size(&header).expect("size");
        assert_eq!(s.width_px(), 1080);
        assert_eq!(s.height_px(), 1920);
        assert!(s.is_portrait());
    }

    #[test]
    fn parse_video_size_landscape() {
        let mut header = [0u8; VIDEO_HEADER_LEN];
        header[56..60].copy_from_slice(&1920f32.to_le_bytes());
        header[60..64].copy_from_slice(&1080f32.to_le_bytes());
        let s = parse_video_size(&header).expect("size");
        assert!(!s.is_portrait());
        assert_eq!(s.width_px(), 1920);
        assert_eq!(s.height_px(), 1080);
    }
}
