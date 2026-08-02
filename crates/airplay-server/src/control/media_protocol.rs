//! Pure parsing and classification for AirPlay HTTP media control.
//!
//! Supports any sender that posts a standard `/play` request with a playable
//! location. `clientProcName` is informational only — never an allowlist.

use plist::{Dictionary, Value};
use thiserror::Error;

/// Parsed `POST /play` body fields.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayRequest {
    /// Informational only (e.g. "YouTube", "MobileSafari", "Music").
    pub client_proc_name: Option<String>,
    pub content_location: String,
    /// Optional 0.0..=1.0 start fraction when present.
    pub start_position: Option<f64>,
    pub uuid: Option<String>,
    pub stream_type: Option<i64>,
    /// Optional initial playback rate from the play plist.
    pub rate: Option<f64>,
    /// Optional initial volume in AirPlay dB from the play plist.
    pub volume_db: Option<f64>,
}

/// Parsed `POST /action` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaAction {
    UnhandledUrlResponse { url: String, data: Vec<u8> },
    PlaylistRemove { uuid: Option<String> },
    Unsupported(String),
}

/// Whether this receiver can attempt playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSupport {
    Supported,
    UnsupportedCodec,
    ProtectedContent,
    InvalidRequest,
}

/// High-level media kind inferred from the URL / path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    SenderLocalHls,
    RemoteHls,
    ProgressiveVideo,
    ProgressiveAudio,
    Unknown,
}

/// Compatibility class for diagnostics (not an allowlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCompatibilityClass {
    /// Ordinary remote HTTP(S) media.
    StandardAirPlay,
    /// Sender-local resource requiring reverse-channel proxy (`mlhls://`).
    SenderLocalHls,
    /// YouTube-style managed live HLS (still uses the generic proxy).
    YouTubeMlhls,
    /// FairPlay / SKD / encrypted content we cannot decrypt.
    ProtectedMedia,
    Unknown,
}

/// Result of classifying a `/play` request for the receiver.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedPlay {
    pub support: MediaSupport,
    pub kind: MediaKind,
    pub class: AppCompatibilityClass,
    /// URL the player should open (may be a local proxy URL for mlhls).
    pub playback_uri: String,
    /// Original Content-Location (redact before logging query).
    pub original_location: String,
    pub client_proc_name: Option<String>,
    pub start_position: Option<f64>,
    pub rate: Option<f64>,
    pub volume_db: Option<f64>,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Error)]
pub enum MediaProtocolError {
    #[error("invalid property list: {0}")]
    Plist(#[from] plist::Error),
    #[error("missing or invalid field {0}")]
    Field(&'static str),
}

/// Parse a binary or XML `/play` plist body.
///
/// `clientProcName` is optional. `Content-Location` is required.
pub fn parse_play_request(body: &[u8]) -> Result<PlayRequest, MediaProtocolError> {
    let value = Value::from_reader(std::io::Cursor::new(body))?;
    let dict = value
        .as_dictionary()
        .ok_or(MediaProtocolError::Field("root dictionary"))?;

    let client_proc_name = dict_string(dict, "clientProcName")?.map(str::to_string);
    let content_location = dict_string(dict, "Content-Location")?
        .or_else(|| dict_string(dict, "content-location").ok().flatten())
        .or_else(|| dict_string(dict, "location").ok().flatten())
        .ok_or(MediaProtocolError::Field("Content-Location"))?
        .to_string();

    if content_location.trim().is_empty() {
        return Err(MediaProtocolError::Field("Content-Location"));
    }

    let start_position = dict
        .get("Start-Position")
        .or_else(|| dict.get("start-position"))
        .and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64)));
    let uuid = dict_string(dict, "uuid")?.map(str::to_string);
    let stream_type = dict.get("streamType").and_then(|v| v.as_signed_integer());
    let rate = dict
        .get("rate")
        .and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64)))
        .filter(|r| r.is_finite());
    let volume_db = dict
        .get("volume")
        .and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64)))
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(-144.0, 0.0));

    Ok(PlayRequest {
        client_proc_name,
        content_location,
        start_position,
        uuid,
        stream_type,
        rate,
        volume_db,
    })
}

/// Classify a play request and produce a player URI.
///
/// `local_proxy` converts `mlhls://localhost/...` → `http://127.0.0.1:port/playlist/...`.
pub fn classify_play_request(
    play: &PlayRequest,
    session_id: &str,
    control_port: u16,
    local_proxy: impl Fn(u16, &str, &str) -> Result<String, String>,
) -> ClassifiedPlay {
    let original = play.content_location.clone();
    let client = play.client_proc_name.clone();
    let is_youtube = client
        .as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case("YouTube"));

    // Protected / DRM schemes first.
    if looks_protected(&original) {
        return ClassifiedPlay {
            support: MediaSupport::ProtectedContent,
            kind: MediaKind::Unknown,
            class: AppCompatibilityClass::ProtectedMedia,
            playback_uri: String::new(),
            original_location: original,
            client_proc_name: client,
            start_position: play.start_position,
            rate: play.rate,
            volume_db: play.volume_db,
            reason: Some(
                "This application is using protected content that this receiver cannot play.",
            ),
        };
    }

    if original.starts_with("mlhls://localhost") {
        match local_proxy(control_port, session_id, &original) {
            Ok(uri) => {
                let class = if is_youtube {
                    AppCompatibilityClass::YouTubeMlhls
                } else {
                    AppCompatibilityClass::SenderLocalHls
                };
                return ClassifiedPlay {
                    support: MediaSupport::Supported,
                    kind: MediaKind::SenderLocalHls,
                    class,
                    playback_uri: uri,
                    original_location: original,
                    client_proc_name: client,
                    start_position: play.start_position,
                    rate: play.rate,
                    volume_db: play.volume_db,
                    reason: None,
                };
            }
            Err(_) => {
                return ClassifiedPlay {
                    support: MediaSupport::InvalidRequest,
                    kind: MediaKind::SenderLocalHls,
                    class: AppCompatibilityClass::SenderLocalHls,
                    playback_uri: String::new(),
                    original_location: original,
                    client_proc_name: client,
                    start_position: play.start_position,
                    rate: play.rate,
                    volume_db: play.volume_db,
                    reason: Some("invalid sender-local media URL"),
                };
            }
        }
    }

    if original.starts_with("http://") || original.starts_with("https://") {
        let kind = infer_remote_kind(&original);
        return ClassifiedPlay {
            support: MediaSupport::Supported,
            kind,
            class: AppCompatibilityClass::StandardAirPlay,
            playback_uri: original.clone(),
            original_location: original,
            client_proc_name: client,
            start_position: play.start_position,
            rate: play.rate,
            volume_db: play.volume_db,
            reason: None,
        };
    }

    // Relative playlist paths sometimes appear; treat as invalid without a base.
    if original.starts_with('/') || original.contains(".m3u8") {
        return ClassifiedPlay {
            support: MediaSupport::InvalidRequest,
            kind: MediaKind::Unknown,
            class: AppCompatibilityClass::Unknown,
            playback_uri: String::new(),
            original_location: original,
            client_proc_name: client,
            start_position: play.start_position,
            rate: play.rate,
            volume_db: play.volume_db,
            reason: Some("relative media URL without base is not supported"),
        };
    }

    ClassifiedPlay {
        support: MediaSupport::UnsupportedCodec,
        kind: MediaKind::Unknown,
        class: AppCompatibilityClass::Unknown,
        playback_uri: String::new(),
        original_location: original,
        client_proc_name: client,
        start_position: play.start_position,
        rate: play.rate,
        volume_db: play.volume_db,
        reason: Some("unsupported media URL scheme"),
    }
}

/// Detect DRM / FairPlay markers in a URL or playlist body snippet.
pub fn looks_protected(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("skd://")
        || lower.contains("skd:")
        || lower.contains("fairplay")
        || lower.contains("com.apple.streamingkeydelivery")
        || lower.contains("urn:uuid:edef8ba9") // Widevine PSSH often paired with DRM
        || lower.contains("#ext-x-key:method=sample-aes")
        || lower.contains("#ext-x-key:method=sample-aes-ctr")
        || lower.contains("method=sample-aes")
}

/// Scan playlist body for protection after FCUP fetch (generic proxy path).
pub fn playlist_looks_protected(body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    looks_protected(text)
}

fn infer_remote_kind(url: &str) -> MediaKind {
    let path = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".m3u8") || path.contains(".m3u8") || path.contains("/playlist") {
        return MediaKind::RemoteHls;
    }
    if path.ends_with(".m4a")
        || path.ends_with(".aac")
        || path.ends_with(".mp3")
        || path.ends_with(".wav")
        || path.ends_with(".flac")
    {
        return MediaKind::ProgressiveAudio;
    }
    if path.ends_with(".mp4")
        || path.ends_with(".m4v")
        || path.ends_with(".mov")
        || path.ends_with(".webm")
        || path.ends_with(".ts")
    {
        return MediaKind::ProgressiveVideo;
    }
    MediaKind::Unknown
}

/// Parse a binary or XML `/action` plist body.
pub fn parse_action(body: &[u8]) -> Result<MediaAction, MediaProtocolError> {
    let value = Value::from_reader(std::io::Cursor::new(body))?;
    let dict = value
        .as_dictionary()
        .ok_or(MediaProtocolError::Field("root dictionary"))?;
    let action_type = dict_string(dict, "type")?
        .ok_or(MediaProtocolError::Field("type"))?
        .to_string();

    match action_type.as_str() {
        "unhandledURLResponse" => {
            let params = dict
                .get("params")
                .and_then(|v| v.as_dictionary())
                .ok_or(MediaProtocolError::Field("params"))?;
            let url = dict_string(params, "FCUP_Response_URL")?
                .ok_or(MediaProtocolError::Field("FCUP_Response_URL"))?
                .to_string();
            let data = params
                .get("FCUP_Response_Data")
                .and_then(|v| v.as_data())
                .ok_or(MediaProtocolError::Field("FCUP_Response_Data"))?
                .to_vec();
            Ok(MediaAction::UnhandledUrlResponse { url, data })
        }
        "playlistRemove" => {
            let uuid = dict
                .get("params")
                .and_then(|v| v.as_dictionary())
                .and_then(|p| p.get("item"))
                .and_then(|v| v.as_dictionary())
                .and_then(|item| dict_string(item, "uuid").ok().flatten())
                .map(str::to_string);
            Ok(MediaAction::PlaylistRemove { uuid })
        }
        other => Ok(MediaAction::Unsupported(other.to_string())),
    }
}

/// Build the Java-compatible FCUP `/event` XML plist body.
pub fn prepare_event_request(session_id: &str, list_uri: &str) -> Result<Vec<u8>, plist::Error> {
    let mut headers = Dictionary::new();
    headers.insert(
        "X-Playback-Session-Id".into(),
        Value::String(session_id.to_string()),
    );

    let mut request = Dictionary::new();
    request.insert("FCUP_Response_ClientInfo".into(), Value::Integer(0.into()));
    request.insert("FCUP_Response_ClientRef".into(), Value::Integer(0.into()));
    request.insert(
        "FCUP_Response_Headers".into(),
        Value::Dictionary(headers),
    );
    request.insert("FCUP_Response_RequestID".into(), Value::Integer(0.into()));
    request.insert(
        "FCUP_Response_URL".into(),
        Value::String(list_uri.to_string()),
    );
    request.insert("sessionID".into(), Value::Integer(1.into()));

    let mut wrapper = Dictionary::new();
    wrapper.insert("request".into(), Value::Dictionary(request));
    wrapper.insert("sessionID".into(), Value::Integer(1.into()));
    wrapper.insert(
        "type".into(),
        Value::String("unhandledURLRequest".into()),
    );

    let mut buf = Vec::new();
    Value::Dictionary(wrapper).to_writer_xml(&mut buf)?;
    Ok(buf)
}

fn dict_string<'a>(
    dict: &'a Dictionary,
    key: &str,
) -> Result<Option<&'a str>, MediaProtocolError> {
    match dict.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(MediaProtocolError::Field("field type")),
    }
}

/// Parse `/rate?value=` query parameter (floating point).
pub fn parse_rate_value(query: &str) -> Option<f64> {
    for part in query.split('&') {
        let mut kv = part.splitn(2, '=');
        let key = kv.next()?;
        let value = kv.next().unwrap_or("");
        if key == "value" {
            let rate: f64 = value.parse().ok()?;
            if rate.is_finite() {
                return Some(rate);
            }
        }
    }
    None
}

/// Parse `/scrub?position=` query (seconds).
pub fn parse_scrub_position(query: &str) -> Option<f64> {
    for part in query.split('&') {
        let mut kv = part.splitn(2, '=');
        let key = kv.next()?;
        let value = kv.next().unwrap_or("");
        if key == "position" {
            let pos: f64 = value.parse().ok()?;
            if pos.is_finite() && pos >= 0.0 {
                return Some(pos);
            }
        }
    }
    None
}

/// Redact URL for logs: scheme + host + path only (drop query).
pub fn redact_media_url(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or(url);
    no_query.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist_bytes(dict: Dictionary) -> Vec<u8> {
        let mut bytes = Vec::new();
        Value::Dictionary(dict)
            .to_writer_binary(&mut bytes)
            .unwrap();
        bytes
    }

    fn proxy(port: u16, sid: &str, remote: &str) -> Result<String, String> {
        if !remote.starts_with("mlhls://localhost") {
            return Err("bad".into());
        }
        let path = remote.trim_start_matches("mlhls://localhost");
        Ok(format!(
            "http://127.0.0.1:{port}/playlist{path}?session={sid}"
        ))
    }

    #[test]
    fn parses_youtube_play_request() {
        let mut dict = Dictionary::new();
        dict.insert("clientProcName".into(), Value::String("YouTube".into()));
        dict.insert(
            "Content-Location".into(),
            Value::String("mlhls://localhost/master.m3u8".into()),
        );
        dict.insert("Start-Position".into(), Value::Real(0.25));
        let play = parse_play_request(&plist_bytes(dict)).unwrap();
        assert_eq!(play.client_proc_name.as_deref(), Some("YouTube"));
        assert_eq!(play.content_location, "mlhls://localhost/master.m3u8");
        assert_eq!(play.start_position, Some(0.25));
    }

    #[test]
    fn parses_play_without_client_proc_name() {
        let mut dict = Dictionary::new();
        dict.insert(
            "Content-Location".into(),
            Value::String("https://cdn.example/video.m3u8".into()),
        );
        let play = parse_play_request(&plist_bytes(dict)).unwrap();
        assert!(play.client_proc_name.is_none());
        assert!(play.content_location.contains("example"));
    }

    #[test]
    fn accepts_safari_and_photos_style_clients() {
        for name in ["MobileSafari", "Photos", "Music", "Podcasts", "AVKit"] {
            let mut dict = Dictionary::new();
            dict.insert("clientProcName".into(), Value::String(name.into()));
            dict.insert(
                "Content-Location".into(),
                Value::String("https://example.invalid/a.mp4".into()),
            );
            let play = parse_play_request(&plist_bytes(dict)).unwrap();
            let c = classify_play_request(&play, "s1", 7000, proxy);
            assert_eq!(c.support, MediaSupport::Supported, "{name}");
            assert_eq!(c.class, AppCompatibilityClass::StandardAirPlay);
            assert!(c.playback_uri.starts_with("https://"));
        }
    }

    #[test]
    fn mlhls_is_sender_local_proxy() {
        let mut dict = Dictionary::new();
        dict.insert(
            "Content-Location".into(),
            Value::String("mlhls://localhost/master.m3u8".into()),
        );
        let play = parse_play_request(&plist_bytes(dict)).unwrap();
        let c = classify_play_request(&play, "session-1", 7000, proxy);
        assert_eq!(c.support, MediaSupport::Supported);
        assert_eq!(c.kind, MediaKind::SenderLocalHls);
        assert_eq!(c.class, AppCompatibilityClass::SenderLocalHls);
        assert!(c.playback_uri.contains("127.0.0.1:7000/playlist"));
    }

    #[test]
    fn youtube_mlhls_class_is_distinct_but_supported() {
        let mut dict = Dictionary::new();
        dict.insert("clientProcName".into(), Value::String("YouTube".into()));
        dict.insert(
            "Content-Location".into(),
            Value::String("mlhls://localhost/master.m3u8".into()),
        );
        let play = parse_play_request(&plist_bytes(dict)).unwrap();
        let c = classify_play_request(&play, "s", 1, proxy);
        assert_eq!(c.class, AppCompatibilityClass::YouTubeMlhls);
        assert_eq!(c.support, MediaSupport::Supported);
    }

    #[test]
    fn protected_skd_rejected() {
        let mut dict = Dictionary::new();
        dict.insert(
            "Content-Location".into(),
            Value::String("skd://fairplay-key-server/xyz".into()),
        );
        let play = parse_play_request(&plist_bytes(dict)).unwrap();
        let c = classify_play_request(&play, "s", 1, proxy);
        assert_eq!(c.support, MediaSupport::ProtectedContent);
        assert_eq!(c.class, AppCompatibilityClass::ProtectedMedia);
        assert!(c.reason.unwrap().contains("protected"));
    }

    #[test]
    fn sample_aes_playlist_detected() {
        let body = b"#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n";
        assert!(playlist_looks_protected(body));
    }

    #[test]
    fn remote_hls_and_mp4_kinds() {
        let hls = PlayRequest {
            client_proc_name: None,
            content_location: "https://cdn.example/master.m3u8".into(),
            start_position: None,
            uuid: None,
            stream_type: None,
            rate: None,
            volume_db: None,
        };
        assert_eq!(
            classify_play_request(&hls, "s", 1, proxy).kind,
            MediaKind::RemoteHls
        );
        let mp4 = PlayRequest {
            content_location: "https://cdn.example/clip.mp4".into(),
            ..hls.clone()
        };
        assert_eq!(
            classify_play_request(&mp4, "s", 1, proxy).kind,
            MediaKind::ProgressiveVideo
        );
        let audio = PlayRequest {
            content_location: "https://cdn.example/ep.m4a".into(),
            ..hls
        };
        assert_eq!(
            classify_play_request(&audio, "s", 1, proxy).kind,
            MediaKind::ProgressiveAudio
        );
    }

    #[test]
    fn parses_unhandled_url_response_data_without_logging_it() {
        let mut params = Dictionary::new();
        params.insert(
            "FCUP_Response_URL".into(),
            Value::String("mlhls://localhost/master.m3u8".into()),
        );
        params.insert(
            "FCUP_Response_Data".into(),
            Value::Data(b"#EXTM3U\n".to_vec()),
        );
        let mut dict = Dictionary::new();
        dict.insert(
            "type".into(),
            Value::String("unhandledURLResponse".into()),
        );
        dict.insert("params".into(), Value::Dictionary(params));
        assert_eq!(
            parse_action(&plist_bytes(dict)).unwrap(),
            MediaAction::UnhandledUrlResponse {
                url: "mlhls://localhost/master.m3u8".into(),
                data: b"#EXTM3U\n".to_vec(),
            }
        );
    }

    #[test]
    fn parses_playlist_remove() {
        let mut item = Dictionary::new();
        item.insert("uuid".into(), Value::String("abc".into()));
        let mut params = Dictionary::new();
        params.insert("item".into(), Value::Dictionary(item));
        let mut dict = Dictionary::new();
        dict.insert("type".into(), Value::String("playlistRemove".into()));
        dict.insert("params".into(), Value::Dictionary(params));
        assert_eq!(
            parse_action(&plist_bytes(dict)).unwrap(),
            MediaAction::PlaylistRemove {
                uuid: Some("abc".into())
            }
        );
    }

    #[test]
    fn event_request_matches_java_field_shape() {
        let bytes = prepare_event_request("session-1", "mlhls://localhost/master.m3u8").unwrap();
        let value = Value::from_reader_xml(bytes.as_slice()).unwrap();
        let root = value.as_dictionary().unwrap();
        let request = root["request"].as_dictionary().unwrap();
        assert_eq!(
            request["FCUP_Response_URL"].as_string(),
            Some("mlhls://localhost/master.m3u8")
        );
        assert_eq!(root["type"].as_string(), Some("unhandledURLRequest"));
    }

    #[test]
    fn rate_and_scrub_parse() {
        assert_eq!(parse_rate_value("value=0"), Some(0.0));
        assert_eq!(parse_scrub_position("position=12.5"), Some(12.5));
    }

    #[test]
    fn missing_content_location_errors() {
        let mut dict = Dictionary::new();
        dict.insert("clientProcName".into(), Value::String("YouTube".into()));
        assert!(matches!(
            parse_play_request(&plist_bytes(dict)),
            Err(MediaProtocolError::Field("Content-Location"))
        ));
    }

    #[test]
    fn redact_strips_query() {
        assert_eq!(
            redact_media_url("https://x/a.m3u8?token=secret"),
            "https://x/a.m3u8"
        );
    }
}
