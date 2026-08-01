//! Integration tests for the AirPlay control server (GET /info, pair-setup).

use std::sync::{Arc, Mutex};

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::{AirPlayConfig, AirPlayConsumer, AirPlayServer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct NoopConsumer;

impl AirPlayConsumer for NoopConsumer {
    fn on_video_format(&self, _info: &VideoStreamInfo) {}
    fn on_video(&self, _data: &[u8]) {}
    fn on_video_src_disconnect(&self) {}
    fn on_audio_format(&self, _info: &AudioStreamInfo) {}
    fn on_audio(&self, _data: &[u8]) {}
    fn on_audio_src_disconnect(&self) {}
}

#[derive(Default)]
struct RecordingVolumeConsumer {
    volume_db: Mutex<Option<f64>>,
}

impl AirPlayConsumer for RecordingVolumeConsumer {
    fn on_video_format(&self, _info: &VideoStreamInfo) {}
    fn on_video(&self, _data: &[u8]) {}
    fn on_video_src_disconnect(&self) {}
    fn on_audio_format(&self, _info: &AudioStreamInfo) {}
    fn on_audio(&self, _data: &[u8]) {}
    fn on_audio_src_disconnect(&self) {}

    fn on_volume(&self, volume_db: f64) {
        *self.volume_db.lock().expect("volume lock") = Some(volume_db);
    }

    fn volume(&self) -> Option<f64> {
        *self.volume_db.lock().expect("volume lock")
    }
}

async fn read_http_like_response(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    // Read until we have headers + full body (or timeout via test runtime).
    loop {
        let n = stream
            .read(&mut tmp)
            .await
            .expect("read response chunk");
        assert!(n > 0, "eof before complete response");
        buf.extend_from_slice(&tmp[..n]);
        if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&buf[..header_end]).unwrap_or("");
            let content_length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("Content-Length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            });
            let body_start = header_end + 4;
            let body_len = content_length.unwrap_or(0);
            if buf.len() >= body_start + body_len {
                return buf;
            }
        }
    }
}

#[tokio::test]
async fn get_info_returns_rtsp_200_with_body() {
    let mut server = AirPlayServer::new(AirPlayConfig::default(), Arc::new(NoopConsumer));
    server.start().await.expect("start server");
    let port = server.port();
    assert_ne!(port, 0);

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");

    let req = b"GET /info RTSP/1.0\r\n\
CSeq: 1\r\n\
Active-Remote: test-session\r\n\
\r\n";
    stream.write_all(req).await.expect("write request");
    stream.flush().await.expect("flush");

    let resp = read_http_like_response(&mut stream).await;
    let text = String::from_utf8_lossy(&resp);
    assert!(
        text.starts_with("RTSP/1.0 200"),
        "response start: {:?}",
        &text[..text.len().min(80)]
    );
    assert!(text.contains("CSeq: 1"));

    // Body should be non-empty binary plist.
    let header_end = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let body = &resp[header_end + 4..];
    assert!(!body.is_empty(), "info body must be non-empty");
    assert_eq!(&body[..6.min(body.len())], b"bplist");

    server.stop().await;
}

#[tokio::test]
async fn pair_setup_returns_32_byte_body() {
    let mut server = AirPlayServer::new(AirPlayConfig::default(), Arc::new(NoopConsumer));
    server.start().await.expect("start server");
    let port = server.port();

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");

    let req = b"POST /pair-setup RTSP/1.0\r\n\
CSeq: 2\r\n\
Active-Remote: test-session\r\n\
Content-Length: 0\r\n\
\r\n";
    stream.write_all(req).await.expect("write");
    stream.flush().await.expect("flush");

    let resp = read_http_like_response(&mut stream).await;
    let text = String::from_utf8_lossy(&resp);
    assert!(
        text.starts_with("RTSP/1.0 200"),
        "response start: {:?}",
        &text[..text.len().min(80)]
    );

    let header_end = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let body = &resp[header_end + 4..];
    assert_eq!(body.len(), 32, "pair-setup must return 32-byte public key");

    server.stop().await;
}

#[tokio::test]
async fn unknown_path_returns_404() {
    let mut server = AirPlayServer::new(AirPlayConfig::default(), Arc::new(NoopConsumer));
    server.start().await.expect("start server");
    let port = server.port();

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");

    let req = b"GET /nope RTSP/1.0\r\nCSeq: 9\r\n\r\n";
    stream.write_all(req).await.expect("write");
    let resp = read_http_like_response(&mut stream).await;
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("RTSP/1.0 404"), "got: {text}");

    server.stop().await;
}

#[tokio::test]
async fn volume_set_parameter_updates_consumer_and_get_reports_it() {
    let consumer = Arc::new(RecordingVolumeConsumer::default());
    let mut server = AirPlayServer::new(AirPlayConfig::default(), consumer.clone());
    server.start().await.expect("start server");

    let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
        .await
        .expect("connect");
    let body = "volume: -20.000000\r\n";
    let request = format!(
        "SET_PARAMETER /stream RTSP/1.0\r\n\
CSeq: 10\r\n\
Content-Type: text/parameters\r\n\
Content-Length: {}\r\n\
\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write SET_PARAMETER");
    let response = read_http_like_response(&mut stream).await;
    assert!(String::from_utf8_lossy(&response).starts_with("RTSP/1.0 200"));
    assert_eq!(
        *consumer.volume_db.lock().expect("volume lock"),
        Some(-20.0)
    );

    stream
        .write_all(
            b"GET_PARAMETER /stream RTSP/1.0\r\n\
CSeq: 11\r\n\
Content-Type: text/parameters\r\n\
Content-Length: 8\r\n\
\r\n\
volume\r\n",
        )
        .await
        .expect("write GET_PARAMETER");
    let response = read_http_like_response(&mut stream).await;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response headers");
    assert_eq!(&response[header_end + 4..], b"volume: -20.000000\r\n");

    server.stop().await;
}

#[tokio::test]
async fn volume_set_parameter_ignores_unrelated_and_invalid_values() {
    let consumer = Arc::new(RecordingVolumeConsumer::default());
    let mut server = AirPlayServer::new(AirPlayConfig::default(), consumer.clone());
    server.start().await.expect("start server");

    let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
        .await
        .expect("connect");
    for (cseq, body) in [
        (20, "progress: 1/2/3\r\n"),
        (21, "volume: not-a-number\r\n"),
        (22, "volume: NaN\r\n"),
    ] {
        let request = format!(
            "SET_PARAMETER /stream RTSP/1.0\r\nCSeq: {cseq}\r\nContent-Type: text/parameters\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write SET_PARAMETER");
        let response = read_http_like_response(&mut stream).await;
        assert!(String::from_utf8_lossy(&response).starts_with("RTSP/1.0 200"));
    }

    assert_eq!(*consumer.volume_db.lock().expect("volume lock"), None);
    server.stop().await;
}
