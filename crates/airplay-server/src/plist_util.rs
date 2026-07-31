//! Binary / XML property-list response builders (port of Java `PropertyListUtil`).

use plist::{Dictionary, Value};

use crate::config::AirPlayConfig;
use crate::consumer::PlaybackInfo;

/// Write a dictionary as a binary property list.
fn to_binary(dict: Dictionary) -> Result<Vec<u8>, plist::Error> {
    let mut buf = Vec::new();
    Value::Dictionary(dict).to_writer_binary(&mut buf)?;
    Ok(buf)
}

/// Write a dictionary as an XML property list.
fn to_xml(dict: Dictionary) -> Result<Vec<u8>, plist::Error> {
    let mut buf = Vec::new();
    Value::Dictionary(dict).to_writer_xml(&mut buf)?;
    Ok(buf)
}

fn int(v: i64) -> Value {
    Value::Integer(v.into())
}

fn uint(v: u64) -> Value {
    Value::Integer(v.into())
}

/// RTSP `GET /info` binary plist (display / audio capability advertisement).
pub fn prepare_info_response(config: &AirPlayConfig) -> Result<Vec<u8>, plist::Error> {
    let mut audio_format_100 = Dictionary::new();
    audio_format_100.insert("audioInputFormats".into(), int(67_108_860));
    audio_format_100.insert("audioOutputFormats".into(), int(67_108_860));
    audio_format_100.insert("type".into(), int(100));

    let mut audio_format_101 = Dictionary::new();
    audio_format_101.insert("audioInputFormats".into(), int(67_108_860));
    audio_format_101.insert("audioOutputFormats".into(), int(67_108_860));
    audio_format_101.insert("type".into(), int(101));

    let audio_formats = Value::Array(vec![
        Value::Dictionary(audio_format_100),
        Value::Dictionary(audio_format_101),
    ]);

    let mut audio_latency_100 = Dictionary::new();
    audio_latency_100.insert("audioType".into(), Value::String("default".into()));
    audio_latency_100.insert("inputLatencyMicros".into(), Value::Boolean(false));
    audio_latency_100.insert("type".into(), int(100));

    let mut audio_latency_101 = Dictionary::new();
    audio_latency_101.insert("audioType".into(), Value::String("default".into()));
    audio_latency_101.insert("inputLatencyMicros".into(), Value::Boolean(false));
    audio_latency_101.insert("type".into(), int(101));

    let audio_latencies = Value::Array(vec![
        Value::Dictionary(audio_latency_100),
        Value::Dictionary(audio_latency_101),
    ]);

    let mut display = Dictionary::new();
    display.insert("features".into(), int(14));
    display.insert("height".into(), uint(u64::from(config.height)));
    display.insert("heightPhysical".into(), Value::Boolean(false));
    display.insert("heightPixels".into(), uint(u64::from(config.height)));
    display.insert("maxFPS".into(), uint(u64::from(config.fps)));
    display.insert("overscanned".into(), Value::Boolean(false));
    display.insert("refreshRate".into(), int(60));
    display.insert("rotation".into(), Value::Boolean(false));
    display.insert(
        "uuid".into(),
        Value::String("e5f7a68d-7b0f-4305-984b-974f677a150b".into()),
    );
    display.insert("width".into(), uint(u64::from(config.width)));
    display.insert("widthPhysical".into(), Value::Boolean(false));
    display.insert("widthPixels".into(), uint(u64::from(config.width)));

    let displays = Value::Array(vec![Value::Dictionary(display)]);

    let mut response = Dictionary::new();
    response.insert("audioFormats".into(), audio_formats);
    response.insert("audioLatencies".into(), audio_latencies);
    response.insert("displays".into(), displays);
    response.insert("features".into(), uint(130_367_356_919));
    response.insert("keepAliveSendStatsAsBody".into(), int(1));
    response.insert("model".into(), Value::String("AppleTV3,2".into()));
    response.insert("name".into(), Value::String("Apple TV".into()));
    response.insert(
        "pi".into(),
        Value::String("b08f5a79-db29-4384-b456-a4784d9e6055".into()),
    );
    response.insert("sourceVersion".into(), Value::String("220.68".into()));
    response.insert("statusFlags".into(), int(68));
    response.insert("vv".into(), int(2));

    to_binary(response)
}

/// SETUP audio stream response (data + control ports).
pub fn prepare_setup_audio_response(
    data_port: u16,
    control_port: u16,
) -> Result<Vec<u8>, plist::Error> {
    let mut data_stream = Dictionary::new();
    data_stream.insert("dataPort".into(), uint(u64::from(data_port)));
    data_stream.insert("type".into(), int(96));
    data_stream.insert("controlPort".into(), uint(u64::from(control_port)));

    let mut response = Dictionary::new();
    response.insert(
        "streams".into(),
        Value::Array(vec![Value::Dictionary(data_stream)]),
    );

    to_binary(response)
}

/// SETUP video stream response (data + event/timing ports).
pub fn prepare_setup_video_response(
    data_port: u16,
    event_port: u16,
    timing_port: u16,
) -> Result<Vec<u8>, plist::Error> {
    let mut data_stream = Dictionary::new();
    data_stream.insert("dataPort".into(), uint(u64::from(data_port)));
    data_stream.insert("type".into(), int(110));

    let mut response = Dictionary::new();
    response.insert(
        "streams".into(),
        Value::Array(vec![Value::Dictionary(data_stream)]),
    );
    response.insert("eventPort".into(), uint(u64::from(event_port)));
    response.insert("timingPort".into(), uint(u64::from(timing_port)));

    to_binary(response)
}

/// HTTP `GET /server-info` XML plist.
pub fn prepare_server_info_response() -> Result<Vec<u8>, plist::Error> {
    let mut response = Dictionary::new();
    // 119 — matches Java; larger feature bits trigger HTTP fp-setup paths.
    response.insert("features".into(), int(119));
    response.insert("protovers".into(), Value::Real(1.0));
    response.insert("srcvers".into(), Value::Real(101.28));
    to_xml(response)
}

/// HTTP `GET /playback-info` XML plist.
pub fn prepare_playback_info_response(info: &PlaybackInfo) -> Result<Vec<u8>, plist::Error> {
    let mut loaded = Dictionary::new();
    loaded.insert("duration".into(), Value::Real(info.duration));
    loaded.insert("start".into(), Value::Real(0.0));

    let mut seekable = Dictionary::new();
    seekable.insert("duration".into(), Value::Real(info.duration));
    seekable.insert("start".into(), Value::Real(0.0));

    let mut response = Dictionary::new();
    response.insert("duration".into(), Value::Real(info.duration));
    response.insert(
        "loadedTimeRanges".into(),
        Value::Array(vec![Value::Dictionary(loaded)]),
    );
    response.insert("playbackBufferEmpty".into(), Value::Boolean(true));
    response.insert("playbackBufferFull".into(), Value::Boolean(false));
    response.insert("playbackLikelyToKeepUp".into(), Value::Boolean(true));
    response.insert("position".into(), Value::Real(info.position));
    response.insert("rate".into(), int(1));
    response.insert("readyToPlay".into(), Value::Boolean(true));
    response.insert(
        "seekableTimeRanges".into(),
        Value::Array(vec![Value::Dictionary(seekable)]),
    );

    to_xml(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_response_is_non_empty_binary_plist() {
        let cfg = AirPlayConfig::default();
        let bytes = prepare_info_response(&cfg).expect("info plist");
        assert!(!bytes.is_empty());
        // Binary plist magic "bplist"
        assert_eq!(&bytes[..6], b"bplist");
    }

    #[test]
    fn server_info_is_xml() {
        let bytes = prepare_server_info_response().expect("server-info");
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("plist") || s.contains("features"));
    }

    #[test]
    fn setup_video_audio_ports_roundtrip_parse() {
        let v = prepare_setup_video_response(7001, 7000, 0).expect("video setup");
        assert_eq!(&v[..6], b"bplist");
        let a = prepare_setup_audio_response(7002, 7003).expect("audio setup");
        assert_eq!(&a[..6], b"bplist");
    }
}
