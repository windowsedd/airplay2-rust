//! Audio UDP packet framing (matches Java `AudioDecoder` / `AudioPacket`).

/// Parsed RTP-like audio packet (12-byte header + encrypted payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPacket {
    pub flag: u8,
    /// Payload type with high bit cleared (`b1 & 0x7F`).
    pub packet_type: u8,
    pub sequence_number: u16,
    /// Timestamp from header bytes 4–7 (big-endian layout as in Java).
    pub timestamp: u32,
    /// SSRC from header bytes 8–11 (big-endian; Java has a known index typo).
    pub ssrc: u32,
    /// Encrypted audio body after the 12-byte header.
    pub encoded_audio: Vec<u8>,
}

/// Parse a datagram: 12-byte header then the remaining body as encoded audio.
///
/// Header layout:
/// - b0: flag
/// - b1: type (`& 0x7F`)
/// - b2–3: sequence number (BE)
/// - b4–7: timestamp (BE; Java builds the same order via shifts)
/// - b8–11: ssrc (BE)
pub fn parse_audio_packet(data: &[u8]) -> Option<AudioPacket> {
    if data.len() < 12 {
        return None;
    }
    let flag = data[0];
    let packet_type = data[1] & 0x7f;
    let sequence_number = u16::from_be_bytes([data[2], data[3]]);
    // Java: (b7) | (b6<<8) | (b5<<16) | (b4<<24) ≡ big-endian u32 at b4..b8
    let timestamp = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    // Java ssrc accidentally uses b6 for one shift; correct BE is b8..b12.
    let ssrc = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let encoded_audio = data[12..].to_vec();
    Some(AudioPacket {
        flag,
        packet_type,
        sequence_number,
        timestamp,
        ssrc,
        encoded_audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_header_and_payload() {
        let mut data = vec![0u8; 12];
        data[0] = 0x80; // flag
        data[1] = 0x96; // type 0x16 with marker bit → & 0x7F = 0x16
        data[2] = 0x12;
        data[3] = 0x34; // seq = 0x1234
        data[4..8].copy_from_slice(&0x0102_0304u32.to_be_bytes());
        data[8..12].copy_from_slice(&0xaabb_ccddu32.to_be_bytes());
        data.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);

        let pkt = parse_audio_packet(&data).expect("packet");
        assert_eq!(pkt.flag, 0x80);
        assert_eq!(pkt.packet_type, 0x16);
        assert_eq!(pkt.sequence_number, 0x1234);
        assert_eq!(pkt.timestamp, 0x0102_0304);
        assert_eq!(pkt.ssrc, 0xaabb_ccdd);
        assert_eq!(pkt.encoded_audio, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_rejects_short_buffer() {
        assert!(parse_audio_packet(&[0u8; 11]).is_none());
    }
}
