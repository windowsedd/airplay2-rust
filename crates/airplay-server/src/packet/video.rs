//! Video packet header parse and NAL unit conversion (AVCC → Annex-B).
//!
//! Framing matches Java `VideoDecoder` / `VideoHandler`.

/// Fixed video stream header size (bytes).
pub const VIDEO_HEADER_LEN: usize = 128;

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

/// Convert AVCC length-prefixed NAL units to Annex-B start codes **in place**.
///
/// For each complete unit: read big-endian `nalu_size` at `idx`, replace the
/// 4-byte length with `00 00 00 01`, then advance by `nalu_size + 4`.
///
/// Stops on incomplete trailing data, zero/invalid size, or the Java sentinel
/// `nalu_size == 1`.
pub fn prepare_picture_nal_units(payload: &mut [u8]) {
    let mut idx = 0usize;
    while idx + 4 <= payload.len() {
        let nalu_size = u32::from_be_bytes([
            payload[idx],
            payload[idx + 1],
            payload[idx + 2],
            payload[idx + 3],
        ]) as usize;

        // Java VideoHandler: if (naluSize == 1) return;
        if nalu_size == 1 {
            return;
        }

        // Prefer all complete NAL units; abort on invalid / truncated size.
        if nalu_size == 0 || idx + 4 + nalu_size > payload.len() {
            return;
        }

        payload[idx] = 0;
        payload[idx + 1] = 0;
        payload[idx + 2] = 0;
        payload[idx + 3] = 1;
        idx += nalu_size + 4;
    }
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
        prepare_picture_nal_units(&mut payload);
        assert_eq!(
            payload,
            vec![
                0x00, 0x00, 0x00, 0x01, 0xaa, 0xbb, 0xcc, //
                0x00, 0x00, 0x00, 0x01, 0xdd, 0xee,
            ]
        );
    }

    #[test]
    fn annex_b_stops_on_truncated() {
        let mut payload = vec![
            0x00, 0x00, 0x00, 0x02, 0xaa, 0xbb, //
            0x00, 0x00, 0x00, 0x05, 0xcc, // truncated nalu (claims 5, has 1)
        ];
        prepare_picture_nal_units(&mut payload);
        // First NAL converted; second length left intact (incomplete).
        assert_eq!(&payload[0..4], &[0, 0, 0, 1]);
        assert_eq!(&payload[4..6], &[0xaa, 0xbb]);
        assert_eq!(&payload[6..10], &[0, 0, 0, 5]);
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
        assert_eq!(
            out,
            vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68]
        );
    }
}
