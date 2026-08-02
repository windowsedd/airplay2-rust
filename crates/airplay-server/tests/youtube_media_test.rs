//! Integration tests for YouTube AirPlay media control (reverse + playlist proxy).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::{
    parse_play_request, prepare_event_request, rewrite_playlist, AirPlayConfig, AirPlayConsumer,
    AirPlayServer, StreamGeneration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Default)]
struct RecordingMediaConsumer {
    playlists: Mutex<Vec<String>>,
    paused: Mutex<u32>,
    resumed: Mutex<u32>,
    removed: Mutex<u32>,
    seeks: Mutex<Vec<f64>>,
}

impl AirPlayConsumer for RecordingMediaConsumer {
    fn on_video_format(&self, _: &VideoStreamInfo, _: StreamGeneration) {}
    fn on_video(&self, _: &[u8]) {}
    fn on_video_src_disconnect(&self, _: StreamGeneration) {}
    fn on_audio_format(&self, _: &AudioStreamInfo, _: StreamGeneration) {}
    fn on_audio(&self, _: &[u8]) {}
    fn on_audio_src_disconnect(&self, _: StreamGeneration) {}

    fn on_media_playlist(&self, uri: &str) {
        self.playlists.lock().unwrap().push(uri.to_string());
    }
    fn on_media_playlist_remove(&self) {
        *self.removed.lock().unwrap() += 1;
    }
    fn on_media_playlist_pause(&self) {
        *self.paused.lock().unwrap() += 1;
    }
    fn on_media_playlist_resume(&self) {
        *self.resumed.lock().unwrap() += 1;
    }
    fn on_media_playlist_seek(&self, position_seconds: f64) {
        self.seeks.lock().unwrap().push(position_seconds);
    }
}

async fn read_http_response(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = stream.read(&mut tmp).await.expect("read");
        assert!(n > 0, "eof before response");
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

fn binary_play_plist() -> Vec<u8> {
    binary_play_plist_with("YouTube", "mlhls://localhost/master.m3u8")
}

fn binary_play_plist_with(client: &str, location: &str) -> Vec<u8> {
    use plist::{Dictionary, Value};
    let mut dict = Dictionary::new();
    if !client.is_empty() {
        dict.insert("clientProcName".into(), Value::String(client.into()));
    }
    dict.insert(
        "Content-Location".into(),
        Value::String(location.into()),
    );
    let mut bytes = Vec::new();
    Value::Dictionary(dict)
        .to_writer_binary(&mut bytes)
        .unwrap();
    bytes
}

fn binary_action_response(url: &str, data: &[u8]) -> Vec<u8> {
    use plist::{Dictionary, Value};
    let mut params = Dictionary::new();
    params.insert("FCUP_Response_URL".into(), Value::String(url.into()));
    params.insert("FCUP_Response_Data".into(), Value::Data(data.to_vec()));
    let mut dict = Dictionary::new();
    dict.insert(
        "type".into(),
        Value::String("unhandledURLResponse".into()),
    );
    dict.insert("params".into(), Value::Dictionary(params));
    let mut bytes = Vec::new();
    Value::Dictionary(dict)
        .to_writer_binary(&mut bytes)
        .unwrap();
    bytes
}

#[tokio::test]
async fn reverse_upgrade_returns_101() {
    let consumer = Arc::new(RecordingMediaConsumer::default());
    let mut server = AirPlayServer::new(AirPlayConfig::default(), consumer);
    server.start().await.expect("start");
    let port = server.port();

    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = b"POST /reverse HTTP/1.1\r\n\
Host: 127.0.0.1\r\n\
X-Apple-Session-ID: session-1\r\n\
X-Apple-Purpose: event\r\n\
Upgrade: PTTH/1.0\r\n\
Connection: Upgrade\r\n\
Content-Length: 0\r\n\
\r\n";
    stream.write_all(req).await.unwrap();
    let resp = timeout(Duration::from_secs(2), read_http_response(&mut stream))
        .await
        .expect("timeout")
        .clone();
    let text = String::from_utf8_lossy(&resp);
    assert!(
        text.starts_with("HTTP/1.1 101 Switching Protocols"),
        "got: {text}"
    );
    assert!(text.contains("Upgrade: PTTH/1.0"));
    assert!(text.contains("Connection: Upgrade"));

    server.stop().await;
}

#[tokio::test]
async fn youtube_play_and_playlist_proxy_round_trip() {
    let consumer = Arc::new(RecordingMediaConsumer::default());
    let mut server = AirPlayServer::new(AirPlayConfig::default(), Arc::clone(&consumer) as _);
    server.start().await.expect("start");
    let port = server.port();

    // 1) Reverse event channel.
    let mut reverse = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    reverse
        .write_all(
            b"POST /reverse HTTP/1.1\r\n\
Host: 127.0.0.1\r\n\
X-Apple-Session-ID: session-1\r\n\
X-Apple-Purpose: event\r\n\
Upgrade: PTTH/1.0\r\n\
Connection: Upgrade\r\n\
Content-Length: 0\r\n\
\r\n",
        )
        .await
        .unwrap();
    let upgrade = timeout(Duration::from_secs(2), read_http_response(&mut reverse))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&upgrade).contains("101"));

    // 2) POST /play
    let body = binary_play_plist();
    let mut play_conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let play_req = format!(
        "POST /play HTTP/1.1\r\n\
Host: 127.0.0.1\r\n\
X-Apple-Session-ID: session-1\r\n\
Content-Type: application/x-apple-binary-plist\r\n\
Content-Length: {}\r\n\
\r\n",
        body.len()
    );
    play_conn.write_all(play_req.as_bytes()).await.unwrap();
    play_conn.write_all(&body).await.unwrap();
    let play_resp = timeout(Duration::from_secs(2), read_http_response(&mut play_conn))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&play_resp).contains("200 OK"));

    let expected_uri = format!("http://127.0.0.1:{port}/playlist/master.m3u8?session=session-1");
    assert_eq!(
        consumer.playlists.lock().unwrap().as_slice(),
        &[expected_uri.clone()]
    );

    // 3) GStreamer-style GET of the proxy playlist (async while we answer FCUP).
    let proxy_path = format!("/playlist/master.m3u8?session=session-1");
    let proxy_task = tokio::spawn(async move {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let req = format!(
            "GET {proxy_path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n"
        );
        s.write_all(req.as_bytes()).await.unwrap();
        read_http_response(&mut s).await
    });

    // 4) Read POST /event on reverse connection.
    let event_bytes = timeout(Duration::from_secs(2), async {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = reverse.read(&mut tmp).await.unwrap();
            assert!(n > 0);
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                // Ensure body present.
                if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&buf[..end]).unwrap();
                    let cl = headers
                        .lines()
                        .find_map(|l| {
                            let (n, v) = l.split_once(':')?;
                            n.eq_ignore_ascii_case("Content-Length")
                                .then(|| v.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + cl {
                        return buf;
                    }
                }
            }
        }
    })
    .await
    .expect("event timeout");

    let event_text = String::from_utf8_lossy(&event_bytes);
    assert!(event_text.starts_with("POST /event HTTP/1.1"));
    assert!(event_text.contains("X-Apple-Session-ID: session-1"));
    assert!(event_text.contains("unhandledURLRequest"));
    assert!(event_text.contains("mlhls://localhost/master.m3u8"));

    // 5) POST /action with synthetic master playlist.
    let master = b"\
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1000
mlhls://localhost/video.m3u8
";
    let action_body = binary_action_response("mlhls://localhost/master.m3u8", master);
    let mut action_conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let action_req = format!(
        "POST /action HTTP/1.1\r\n\
Host: 127.0.0.1\r\n\
X-Apple-Session-ID: session-1\r\n\
Content-Type: application/x-apple-binary-plist\r\n\
Content-Length: {}\r\n\
\r\n",
        action_body.len()
    );
    action_conn.write_all(action_req.as_bytes()).await.unwrap();
    action_conn.write_all(&action_body).await.unwrap();
    let action_resp = timeout(Duration::from_secs(2), read_http_response(&mut action_conn))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&action_resp).contains("200 OK"));

    // 6) Proxy response should be rewritten.
    let proxy_resp = timeout(Duration::from_secs(3), proxy_task)
        .await
        .expect("proxy join timeout")
        .expect("proxy task");
    let proxy_text = String::from_utf8_lossy(&proxy_resp);
    assert!(proxy_text.contains("200 OK"), "{proxy_text}");
    assert!(proxy_text.contains("application/vnd.apple.mpegurl"));
    assert!(proxy_text.contains("http://127.0.0.1:"));
    assert!(!proxy_text.contains("mlhls://localhost"));

    // 7) rate pause/resume
    let mut rate = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    rate.write_all(b"POST /rate?value=0 HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: session-1\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let _ = read_http_response(&mut rate).await;
    rate.write_all(b"POST /rate?value=1 HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: session-1\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let _ = read_http_response(&mut rate).await;
    assert_eq!(*consumer.paused.lock().unwrap(), 1);
    assert_eq!(*consumer.resumed.lock().unwrap(), 1);

    // 8) stop
    let mut stop = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stop.write_all(b"POST /stop HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: session-1\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let _ = read_http_response(&mut stop).await;
    assert!(*consumer.removed.lock().unwrap() >= 1);

    server.stop().await;
}

#[test]
fn play_request_unit_parse() {
    let body = binary_play_plist();
    let play = parse_play_request(&body).unwrap();
    assert_eq!(play.client_proc_name.as_deref(), Some("YouTube"));
    assert!(play.content_location.contains("mlhls://localhost"));
}

#[tokio::test]
async fn generic_app_http_play_is_accepted() {
    let consumer = Arc::new(RecordingMediaConsumer::default());
    let mut server = AirPlayServer::new(AirPlayConfig::default(), Arc::clone(&consumer) as _);
    server.start().await.expect("start");
    let port = server.port();

    // Safari-style direct HTTPS HLS — no reverse channel required for /play accept.
    let body = binary_play_plist_with(
        "MobileSafari",
        "https://devstreaming-cdn.example/bipbop.m3u8",
    );
    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "POST /play HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: safari-1\r\nContent-Type: application/x-apple-binary-plist\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    conn.write_all(req.as_bytes()).await.unwrap();
    conn.write_all(&body).await.unwrap();
    let resp = timeout(Duration::from_secs(2), read_http_response(&mut conn))
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&resp).contains("200 OK"),
        "generic apps must not be rejected: {}",
        String::from_utf8_lossy(&resp)
    );
    let playlists = consumer.playlists.lock().unwrap().clone();
    assert_eq!(
        playlists.as_slice(),
        &["https://devstreaming-cdn.example/bipbop.m3u8".to_string()]
    );

    // Unknown client with MP4
    let body = binary_play_plist_with("SomePodcastApp", "https://cdn.example/ep.m4a");
    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "POST /play HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: pod-1\r\nContent-Type: application/x-apple-binary-plist\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    conn.write_all(req.as_bytes()).await.unwrap();
    conn.write_all(&body).await.unwrap();
    let resp = timeout(Duration::from_secs(2), read_http_response(&mut conn))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&resp).contains("200 OK"));

    // Protected SKD URL → 501, server stays up
    let body = binary_play_plist_with("TVApp", "skd://keys.example/asset");
    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "POST /play HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: drm-1\r\nContent-Type: application/x-apple-binary-plist\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    conn.write_all(req.as_bytes()).await.unwrap();
    conn.write_all(&body).await.unwrap();
    let resp = timeout(Duration::from_secs(2), read_http_response(&mut conn))
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&resp).contains("501"),
        "protected content should be 501"
    );

    // Control still alive
    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    conn.write_all(b"GET /server-info HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let resp = timeout(Duration::from_secs(2), read_http_response(&mut conn))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&resp).contains("200"));

    server.stop().await;
}

#[tokio::test]
async fn play_without_client_proc_name_still_works() {
    let consumer = Arc::new(RecordingMediaConsumer::default());
    let mut server = AirPlayServer::new(AirPlayConfig::default(), Arc::clone(&consumer) as _);
    server.start().await.expect("start");
    let port = server.port();

    let body = binary_play_plist_with("", "https://cdn.example/clip.mp4");
    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "POST /play HTTP/1.1\r\nHost: x\r\nX-Apple-Session-ID: anon\r\nContent-Type: application/x-apple-binary-plist\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    conn.write_all(req.as_bytes()).await.unwrap();
    conn.write_all(&body).await.unwrap();
    let resp = timeout(Duration::from_secs(2), read_http_response(&mut conn))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&resp).contains("200 OK"));
    assert!(consumer
        .playlists
        .lock()
        .unwrap()
        .iter()
        .any(|u| u.ends_with("clip.mp4")));
    server.stop().await;
}

#[test]
fn event_request_shape() {
    let bytes = prepare_event_request("s1", "mlhls://localhost/master.m3u8").unwrap();
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("unhandledURLRequest"));
    assert!(s.contains("FCUP_Response_URL"));
}

#[test]
fn fixture_master_rewrite() {
    let raw = include_bytes!("fixtures/youtube_master.m3u8");
    let out = rewrite_playlist(raw, 7000, "session-1").unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("http://127.0.0.1:7000/playlist/"));
    assert!(!text.contains("mlhls://localhost"));
}
