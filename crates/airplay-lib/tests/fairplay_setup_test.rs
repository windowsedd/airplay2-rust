//! Integration tests ported from Java `AirPlayFairPlayTest` (fp-setup only).

use airplay_lib::{AirPlay, FairPlay};

/// Java signed-byte → u8 (two's complement).
fn j(bytes: &[i8]) -> Vec<u8> {
    bytes.iter().map(|&b| b as u8).collect()
}

#[test]
fn fairplay_setup_phase1_matches_java() {
    let mut fp = FairPlay::new();

    // /fp-setup 1 request (mode 0)
    let req = j(&[70, 80, 76, 89, 3, 1, 1, 0, 0, 0, 0, 4, 2, 0, 0, -69]);
    assert_eq!(req.len(), 16);
    assert_eq!(req[14], 0); // mode 0

    let resp = fp.fair_play_setup(&req).expect("fp-setup phase 1");

    let expected = j(&[
        70, 80, 76, 89, 3, 1, 2, 0, 0, 0, 0, -126, 2, 0, 15, -97, 63, -98, 10, 37, 33, -37, -33, 49,
        42, -78, -65, -78, -98, -115, 35, 43, 99, 118, -88, -56, 24, 112, 29, 34, -82, -109, -40, 39,
        55, -2, -81, -99, -76, -3, -12, 28, 45, -70, -99, 31, 73, -54, -86, -65, 101, -111, -84, 31,
        123, -58, -9, -32, 102, 61, 33, -81, -32, 21, 101, -107, 62, -85, -127, -12, 24, -50, -19, 9,
        90, -37, 124, 61, 14, 37, 73, 9, -89, -104, 49, -44, -100, 57, -126, -105, 52, 52, -6, -53,
        66, -58, 58, 28, -39, 17, -90, -2, -108, 26, -118, 109, 74, 116, 59, 70, -61, -89, 100, -98,
        68, -57, -119, 85, -28, -99, -127, 85, 0, -107, 73, -60, -30, -9, -93, -10, -43, -70,
    ]);
    assert_eq!(resp, expected);
    assert_eq!(resp.len(), 142);
    assert!(fp.key_msg().is_none());
}

#[test]
fn fairplay_setup_phase2_matches_java() {
    let mut fp = FairPlay::new();

    // /fp-setup 2 request (164 bytes)
    let req = j(&[
        70, 80, 76, 89, 3, 1, 3, 0, 0, 0, 0, -104, 0, -113, 26, -100, -40, -92, -10, 52, 109, 20,
        120, 6, -62, -67, -118, 75, -47, -71, -109, -45, -61, 106, -95, 1, 36, -104, -7, 78, -1, -13,
        70, 123, -49, 27, 49, -104, 98, 92, -94, 69, -114, 62, -48, 30, -35, 53, -25, 41, 53, 125, -7,
        75, -128, -51, 10, -50, 35, 84, -42, -116, -29, 127, 94, 24, -16, -49, -46, 109, 65, 103, 21,
        63, -64, -76, 54, 35, 22, 111, 8, -58, 111, -45, 1, 56, 14, -80, -98, -97, -115, -24, 59, -46,
        -82, -57, -92, 1, -15, -5, -67, -13, 46, 10, -43, 81, -24, 121, 63, -25, -63, 25, 35, 51,
        -103, -91, 53, 76, -59, 67, 7, 30, -68, -50, -32, -84, -123, 34, -82, 27, -85, 51, -44, 65,
        -60, 120, -11, 99, -50, -3, 66, 117, -5, 85, 90, 58, -29, 58, -40, -71, -7, -108, -7, -75,
    ]);
    assert_eq!(req.len(), 164);

    let resp = fp.fair_play_setup(&req).expect("fp-setup phase 2");

    let expected = j(&[
        70, 80, 76, 89, 3, 1, 4, 0, 0, 0, 0, 20, -60, 120, -11, 99, -50, -3, 66, 117, -5, 85, 90,
        58, -29, 58, -40, -71, -7, -108, -7, -75,
    ]);
    assert_eq!(resp, expected);
    assert_eq!(resp.len(), 32);

    // key_msg stored; last 20 of response == last 20 of request
    let key_msg = fp.key_msg().expect("key_msg after phase 2");
    assert_eq!(key_msg.as_slice(), req.as_slice());
    assert_eq!(&resp[12..], &req[144..164]);
}

#[test]
fn fairplay_setup_via_airplay_facade() {
    let mut airplay = AirPlay::new();

    let req1 = j(&[70, 80, 76, 89, 3, 1, 1, 0, 0, 0, 0, 4, 2, 0, 0, -69]);
    let resp1 = airplay
        .fair_play_setup(&req1)
        .expect("AirPlay fp-setup phase 1");
    assert_eq!(resp1.len(), 142);

    let req2 = j(&[
        70, 80, 76, 89, 3, 1, 3, 0, 0, 0, 0, -104, 0, -113, 26, -100, -40, -92, -10, 52, 109, 20,
        120, 6, -62, -67, -118, 75, -47, -71, -109, -45, -61, 106, -95, 1, 36, -104, -7, 78, -1, -13,
        70, 123, -49, 27, 49, -104, 98, 92, -94, 69, -114, 62, -48, 30, -35, 53, -25, 41, 53, 125, -7,
        75, -128, -51, 10, -50, 35, 84, -42, -116, -29, 127, 94, 24, -16, -49, -46, 109, 65, 103, 21,
        63, -64, -76, 54, 35, 22, 111, 8, -58, 111, -45, 1, 56, 14, -80, -98, -97, -115, -24, 59, -46,
        -82, -57, -92, 1, -15, -5, -67, -13, 46, 10, -43, 81, -24, 121, 63, -25, -63, 25, 35, 51,
        -103, -91, 53, 76, -59, 67, 7, 30, -68, -50, -32, -84, -123, 34, -82, 27, -85, 51, -44, 65,
        -60, 120, -11, 99, -50, -3, 66, 117, -5, 85, 90, 58, -29, 58, -40, -71, -7, -108, -7, -75,
    ]);
    let resp2 = airplay
        .fair_play_setup(&req2)
        .expect("AirPlay fp-setup phase 2");
    assert_eq!(resp2.len(), 32);
    assert_eq!(
        resp2,
        j(&[
            70, 80, 76, 89, 3, 1, 4, 0, 0, 0, 0, 20, -60, 120, -11, 99, -50, -3, 66, 117, -5, 85,
            90, 58, -29, 58, -40, -71, -7, -108, -7, -75,
        ])
    );
}

#[test]
fn fairplay_unsupported_version_errors() {
    let mut fp = FairPlay::new();
    let mut req = [0u8; 16];
    req[4] = 2; // not version 3
    let err = fp.fair_play_setup(&req).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not supported") || msg.contains("version"),
        "unexpected error: {msg}"
    );
}

#[test]
fn decrypt_aes_key_stub_not_implemented() {
    let fp = FairPlay::new();
    let err = fp.decrypt_aes_key(&[0u8; 72]).unwrap_err();
    assert!(err.to_string().contains("not implemented"));
}
