//! Full FairPlay vector test ported from Java `AirPlayFairPlayTest.fairPlayTest`.

use std::fs;
use std::path::PathBuf;

use airplay_lib::{AirPlay, FairPlayVideoDecryptor, MediaStreamInfo};
use plist::{Dictionary, Value};

/// Java signed-byte → u8 (two's complement).
fn j(bytes: &[i8]) -> Vec<u8> {
    bytes.iter().map(|&b| b as u8).collect()
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_binary_plist(dict: Dictionary) -> Vec<u8> {
    let mut buf = Vec::new();
    Value::Dictionary(dict)
        .to_writer_binary(&mut buf)
        .expect("write binary plist");
    buf
}

#[test]
fn fairplay_full_decrypt_vector() {
    let mut airplay = AirPlay::new();

    // /fp-setup 1 request
    let fair_play_setup1_request = j(&[70, 80, 76, 89, 3, 1, 1, 0, 0, 0, 0, 4, 2, 0, 0, -69]);
    let fair_play_setup1_response = airplay
        .fair_play_setup(&fair_play_setup1_request)
        .expect("fp-setup 1");
    let fair_play_setup1_response_bytes = j(&[
        70, 80, 76, 89, 3, 1, 2, 0, 0, 0, 0, -126, 2, 0, 15, -97, 63, -98, 10, 37, 33, -37, -33, 49,
        42, -78, -65, -78, -98, -115, 35, 43, 99, 118, -88, -56, 24, 112, 29, 34, -82, -109, -40, 39,
        55, -2, -81, -99, -76, -3, -12, 28, 45, -70, -99, 31, 73, -54, -86, -65, 101, -111, -84, 31,
        123, -58, -9, -32, 102, 61, 33, -81, -32, 21, 101, -107, 62, -85, -127, -12, 24, -50, -19, 9,
        90, -37, 124, 61, 14, 37, 73, 9, -89, -104, 49, -44, -100, 57, -126, -105, 52, 52, -6, -53,
        66, -58, 58, 28, -39, 17, -90, -2, -108, 26, -118, 109, 74, 116, 59, 70, -61, -89, 100, -98,
        68, -57, -119, 85, -28, -99, -127, 85, 0, -107, 73, -60, -30, -9, -93, -10, -43, -70,
    ]);
    assert_eq!(fair_play_setup1_response, fair_play_setup1_response_bytes);

    // /fp-setup 2 request
    let fair_play_setup2_request = j(&[
        70, 80, 76, 89, 3, 1, 3, 0, 0, 0, 0, -104, 0, -113, 26, -100, -40, -92, -10, 52, 109, 20,
        120, 6, -62, -67, -118, 75, -47, -71, -109, -45, -61, 106, -95, 1, 36, -104, -7, 78, -1, -13,
        70, 123, -49, 27, 49, -104, 98, 92, -94, 69, -114, 62, -48, 30, -35, 53, -25, 41, 53, 125, -7,
        75, -128, -51, 10, -50, 35, 84, -42, -116, -29, 127, 94, 24, -16, -49, -46, 109, 65, 103, 21,
        63, -64, -76, 54, 35, 22, 111, 8, -58, 111, -45, 1, 56, 14, -80, -98, -97, -115, -24, 59, -46,
        -82, -57, -92, 1, -15, -5, -67, -13, 46, 10, -43, 81, -24, 121, 63, -25, -63, 25, 35, 51,
        -103, -91, 53, 76, -59, 67, 7, 30, -68, -50, -32, -84, -123, 34, -82, 27, -85, 51, -44, 65,
        -60, 120, -11, 99, -50, -3, 66, 117, -5, 85, 90, 58, -29, 58, -40, -71, -7, -108, -7, -75,
    ]);
    let fair_play_setup2_response = airplay
        .fair_play_setup(&fair_play_setup2_request)
        .expect("fp-setup 2");
    let fair_play_setup2_response_bytes = j(&[
        70, 80, 76, 89, 3, 1, 4, 0, 0, 0, 0, 20, -60, 120, -11, 99, -50, -3, 66, 117, -5, 85, 90,
        58, -29, 58, -40, -71, -7, -108, -7, -75,
    ]);
    assert_eq!(fair_play_setup2_response, fair_play_setup2_response_bytes);

    // RTSP SETUP 1: ekey + eiv
    // NOTE: eiv is raw ASCII of the base64 string, NOT decoded base64 (matches Java test).
    let encrypted_aes_key = j(&[
        70, 80, 76, 89, 1, 2, 1, 0, 0, 0, 0, 60, 0, 0, 0, 0, 63, 121, 70, -69, 3, -8, 117, -13, 83,
        72, 105, -51, -11, -43, -1, 17, 0, 0, 0, 16, 24, -109, 13, 105, -32, -125, -73, -128, 21, 29,
        -31, 72, -41, 112, -36, -75, 57, 110, 71, -72, -25, -59, 102, 22, 19, -43, 35, 74, -20, 86,
        15, 16, 126, 5, 15, -45,
    ]);
    let eiv = b"91IdM6RTh4keicMei2GfQA==".to_vec();
    let mut rtsp_setup1 = Dictionary::new();
    rtsp_setup1.insert("ekey".into(), Value::Data(encrypted_aes_key));
    rtsp_setup1.insert("eiv".into(), Value::Data(eiv));
    let rtsp_setup1_bytes = write_binary_plist(rtsp_setup1);
    let info1 = airplay
        .rtsp_setup(&rtsp_setup1_bytes)
        .expect("rtsp setup ekey/eiv");
    assert!(info1.is_none());

    // RTSP SETUP 2: video stream type 110 + streamConnectionID
    let stream_connection_id: i64 = -3907568444900622110;
    let mut data_stream = Dictionary::new();
    data_stream.insert("type".into(), Value::Integer(110.into()));
    data_stream.insert(
        "streamConnectionID".into(),
        Value::Integer(stream_connection_id.into()),
    );
    let mut rtsp_setup2 = Dictionary::new();
    rtsp_setup2.insert(
        "streams".into(),
        Value::Array(vec![Value::Dictionary(data_stream)]),
    );
    let rtsp_setup2_bytes = write_binary_plist(rtsp_setup2);
    let info2 = airplay
        .rtsp_setup(&rtsp_setup2_bytes)
        .expect("rtsp setup streams");
    match info2 {
        Some(MediaStreamInfo::Video(v)) => {
            assert_eq!(v.stream_connection_id, (stream_connection_id as u64).to_string());
        }
        other => panic!("expected video stream info, got {other:?}"),
    }

    // Decrypt payload: construct FairPlayVideoDecryptor with fixed shared secret (Java test).
    let shared_secret = j(&[
        -5, -67, -104, 31, 49, 40, -76, 40, -116, 105, 45, -47, 125, -94, 117, -104, -54, -47, -50,
        6, 122, 1, -38, -114, -88, -85, -128, 2, 116, -119, -90, 123,
    ]);
    assert_eq!(shared_secret.len(), 32);

    let aes_key = airplay.get_fairplay_aes_key().expect("get_fairplay_aes_key");
    let conn_id_str = (stream_connection_id as u64).to_string();
    // Java: Long.toUnsignedString(-3907568444900622110L) == "14539175628808929506"
    assert_eq!(conn_id_str, "14539175628808929506");

    let mut decryptor = FairPlayVideoDecryptor::new(&aes_key, &shared_secret, &conn_id_str)
        .expect("construct video decryptor");

    let mut payload = fs::read(fixture_path("encrypted_payload")).expect("read encrypted_payload");
    assert!(!payload.is_empty());

    decryptor.decrypt(&mut payload).expect("decrypt video");

    // nc_len from first 4 bytes big-endian
    let nc_len = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    assert_eq!(
        nc_len,
        payload.len() - 4,
        "Decrypted payload is corrupted! nc_len={nc_len} expected={}",
        payload.len() - 4
    );
}

#[test]
fn rtsp_audio_stream_setup() {
    let mut airplay = AirPlay::new();

    let mut data_stream = Dictionary::new();
    data_stream.insert("type".into(), Value::Integer(96.into()));
    data_stream.insert("ct".into(), Value::Integer(2.into())); // ALAC
    data_stream.insert("audioFormat".into(), Value::Integer(0x100000i64.into())); // ALAC_48000_16_2
    data_stream.insert("spf".into(), Value::Integer(352.into()));

    let mut setup = Dictionary::new();
    setup.insert(
        "streams".into(),
        Value::Array(vec![Value::Dictionary(data_stream)]),
    );
    let bytes = write_binary_plist(setup);
    let info = airplay.rtsp_setup(&bytes).expect("audio setup");
    match info {
        Some(MediaStreamInfo::Audio(a)) => {
            assert_eq!(
                a.compression_type,
                Some(airplay_lib::CompressionType::Alac)
            );
            assert_eq!(
                a.audio_format,
                Some(airplay_lib::AudioFormat::Alac48000_16_2)
            );
            assert_eq!(a.samples_per_frame, Some(352));
        }
        other => panic!("expected audio stream info, got {other:?}"),
    }
}

#[test]
fn video_decryptor_not_ready_without_pairing() {
    let airplay = AirPlay::new();
    assert!(!airplay.is_fairplay_video_decryptor_ready());
    assert!(!airplay.is_fairplay_audio_decryptor_ready());
}
