//! RTSP SETUP / TEARDOWN binary plist parsing and session state.
//!
//! Port of Java `com.github.serezhka.airplay.lib.internal.RTSP`.

use plist::{Dictionary, Value};
use tracing::{debug, error, info, warn};

use crate::error::{AirPlayError, Result};
use crate::stream_info::{
    AudioFormat, AudioStreamInfo, CompressionType, MediaStreamInfo, VideoStreamInfo,
};

/// RTSP SETUP state: encrypted AES key material and stream connection id.
#[derive(Debug, Default)]
pub struct Rtsp {
    ekey: Option<Vec<u8>>,
    eiv: Option<Vec<u8>>,
    stream_connection_id: Option<String>,
}

impl Rtsp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse RTSP SETUP binary plist body.
    ///
    /// - If `ekey` / `eiv` present: store them, return `None`.
    /// - If `streams` present: parse first stream, update stream id for type 110, return info.
    pub fn setup(&mut self, plist_bytes: &[u8]) -> Result<Option<MediaStreamInfo>> {
        let dict = parse_dict(plist_bytes)?;

        if dict.contains_key("ekey") || dict.contains_key("eiv") {
            if let Some(v) = dict.get("ekey") {
                self.ekey = Some(value_as_data(v, "ekey")?);
            }
            if let Some(v) = dict.get("eiv") {
                self.eiv = Some(value_as_data(v, "eiv")?);
            }
            info!(
                "Encrypted AES key: {}, iv: {}",
                hex_encode(self.ekey.as_deref().unwrap_or(&[])),
                hex_encode(self.eiv.as_deref().unwrap_or(&[]))
            );
            return Ok(None);
        }

        if dict.contains_key("streams") {
            debug!("RTSP SETUP streams present");
            return self.get_media_stream_info(&dict);
        }

        error!("Unknown RTSP setup content (no ekey/eiv/streams)");
        Ok(None)
    }

    /// Parse RTSP TEARDOWN binary plist body.
    pub fn teardown(&mut self, plist_bytes: &[u8]) -> Result<Option<MediaStreamInfo>> {
        let dict = parse_dict(plist_bytes)?;
        debug!("RTSP TEARDOWN streams");
        if dict.contains_key("streams") {
            return self.get_media_stream_info(&dict);
        }
        Ok(None)
    }

    pub fn stream_connection_id(&self) -> Option<&str> {
        self.stream_connection_id.as_deref()
    }

    pub fn ekey(&self) -> Option<&[u8]> {
        self.ekey.as_deref()
    }

    pub fn eiv(&self) -> Option<&[u8]> {
        self.eiv.as_deref()
    }

    fn get_media_stream_info(&mut self, request: &Dictionary) -> Result<Option<MediaStreamInfo>> {
        let streams = request
            .get("streams")
            .ok_or_else(|| AirPlayError::Rtsp("missing streams".into()))?;

        let arr = match streams {
            Value::Array(a) => a,
            _ => {
                return Err(AirPlayError::Rtsp(
                    "streams is not an array".into(),
                ))
            }
        };

        if arr.is_empty() {
            return Err(AirPlayError::Rtsp("streams array is empty".into()));
        }
        if arr.len() > 1 {
            warn!("Request contains more than one stream info");
        }

        let stream = match &arr[0] {
            Value::Dictionary(d) => d,
            _ => {
                return Err(AirPlayError::Rtsp(
                    "stream entry is not a dictionary".into(),
                ))
            }
        };

        let type_code = stream
            .get("type")
            .and_then(value_as_i64)
            .ok_or_else(|| AirPlayError::Rtsp("stream missing type".into()))?;

        match type_code {
            // video stream
            110 => {
                if let Some(v) = stream.get("streamConnectionID") {
                    let id = value_as_i64(v).ok_or_else(|| {
                        AirPlayError::Rtsp("streamConnectionID is not an integer".into())
                    })?;
                    // Java: Long.toUnsignedString((long) streamConnectionID)
                    self.stream_connection_id = Some((id as u64).to_string());
                }
                let conn_id = self
                    .stream_connection_id
                    .clone()
                    .unwrap_or_default();
                Ok(Some(MediaStreamInfo::Video(VideoStreamInfo::new(conn_id))))
            }
            // audio stream
            96 => {
                let mut info = AudioStreamInfo::default();
                if let Some(v) = stream.get("ct") {
                    let code = value_as_i64(v).ok_or_else(|| {
                        AirPlayError::Rtsp("ct is not an integer".into())
                    })? as u64;
                    info.compression_type = Some(CompressionType::from_code(code)?);
                }
                if let Some(v) = stream.get("audioFormat") {
                    let code = value_as_i64(v).ok_or_else(|| {
                        AirPlayError::Rtsp("audioFormat is not an integer".into())
                    })? as u64;
                    info.audio_format = Some(AudioFormat::from_code(code)?);
                }
                if let Some(v) = stream.get("spf") {
                    let spf = value_as_i64(v).ok_or_else(|| {
                        AirPlayError::Rtsp("spf is not an integer".into())
                    })?;
                    info.samples_per_frame = Some(spf as i32);
                }
                Ok(Some(MediaStreamInfo::Audio(info)))
            }
            other => {
                error!("Unknown stream type: {other}");
                Ok(None)
            }
        }
    }
}

fn parse_dict(plist_bytes: &[u8]) -> Result<Dictionary> {
    let value = plist::from_bytes::<Value>(plist_bytes)
        .map_err(|e| AirPlayError::Rtsp(format!("plist parse error: {e}")))?;
    match value {
        Value::Dictionary(d) => Ok(d),
        _ => Err(AirPlayError::Rtsp(
            "RTSP plist root is not a dictionary".into(),
        )),
    }
}

fn value_as_data(v: &Value, key: &str) -> Result<Vec<u8>> {
    match v {
        Value::Data(d) => Ok(d.clone()),
        _ => Err(AirPlayError::Rtsp(format!(
            "{key} is not binary data"
        ))),
    }
}

fn value_as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Integer(i) => i
            .as_signed()
            .or_else(|| i.as_unsigned().map(|u| u as i64)),
        _ => None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
