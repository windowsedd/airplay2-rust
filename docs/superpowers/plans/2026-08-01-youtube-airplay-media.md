# YouTube AirPlay Media Playback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace display-quality presets with explicit geometry fields and implement the Java-faithful YouTube AirPlay reverse-channel, playlist-proxy, and GStreamer media-playback path.

**Architecture:** Keep protocol parsing and playlist rewriting in pure `airplay-server` modules, store bounded reverse-channel and pending-playlist state per session, and teach each Tokio control connection to transition into a reverse-response transport after HTTP upgrade. The existing `AirPlayConsumer` media hooks drive a separately managed GStreamer `playbin3`; screen mirroring and media playback never share codec state.

**Tech Stack:** Rust 2021, Tokio channels and timeouts, `plist`, `tracing`, GStreamer 1.20+ / Rust bindings 0.23, TOML/serde, and the bundled Java implementation as protocol oracle.

## Global Constraints

- Keep the educational/research disclaimer and do not present this as a commercial AirPlay implementation.
- Do not change pairing, FairPlay, OmgHax, HandGarble, SapHash, or Java-style overflow behavior.
- Keep `overflow-checks = false` in workspace development and test profiles.
- Preserve the crate graph: `lib <- server <- player/app`; `client -> lib` only.
- Do not delete, build, or clean the `java-airplay-2open` reference tree.
- GStreamer remains optional; `cargo test --workspace` and the `h264-dump` feature must not require a GStreamer runtime.
- Do not add a fake video-bitrate option or video re-encoding.
- Generated playlist URLs use `127.0.0.1`, and logs must not expose signed URLs, playlist bodies, device content, pairing secrets, or FCUP data.
- Preserve unrelated changes in the existing dirty checkout. Stage only the files named by each task.

---

## File Structure

- `crates/airplay-app/src/main.rs`: explicit display configuration and legacy-field warning.
- `config.toml`, `crates/airplay-app/config.example.toml`: explicit configuration examples.
- `crates/airplay-server/src/control/media_protocol.rs`: pure `/play`, `/action`, FCUP plist, and URL conversion logic.
- `crates/airplay-server/src/control/playlist.rs`: line-preserving master/media playlist rewriting.
- `crates/airplay-server/src/control/codec.rs`: outbound HTTP request and inbound HTTP response framing for reverse connections.
- `crates/airplay-server/src/control/handler.rs`: HTTP media endpoint behavior and connection directives.
- `crates/airplay-server/src/server.rs`: normal-to-reverse connection transition and writer loop.
- `crates/airplay-server/src/session.rs`: bounded reverse writers and pending playlist response ownership.
- `crates/airplay-player/src/gstreamer_player.rs`: safe `playbin3` lifecycle and media bus diagnostics.
- `crates/airplay-server/tests/youtube_media_test.rs`: socket-level media control integration.
- `crates/airplay-server/tests/fixtures/`: sanitized playlists containing no live tokens or private media.
- `README.md`, `docs/superpowers/plans/acceptance-checklist.md`: configuration, behavior, and live-test boundary.

---

### Task 1: Replace Quality Presets with Explicit Display Fields

**Files:**
- Modify: `crates/airplay-app/src/main.rs`
- Modify: `config.toml`
- Modify: `crates/airplay-app/config.example.toml`
- Modify: `README.md`

**Interfaces:**
- Produces: `AirplaySection { width, height, fps, refresh_rate, legacy_quality }`
- Produces: `resolve_display_config(&AirplaySection) -> (u32, u32, u32, u32)`
- Consumed by: existing `AirPlayConfig` construction in `main`.

- [ ] **Step 1: Write failing configuration tests**

Replace the preset-focused tests in `crates/airplay-app/src/main.rs` with tests that exercise explicit fields and legacy compatibility:

```rust
#[test]
fn explicit_display_fields_are_used_verbatim() {
    let cfg: AppConfig = toml::from_str(
        r#"
        [airplay]
        width = 1280
        height = 720
        fps = 60
        refresh_rate = 60
        [player]
        implementation = "gstreamer"
        "#,
    )
    .unwrap();
    assert_eq!(resolve_display_config(&cfg.airplay), (1280, 720, 60, 60));
}

#[test]
fn legacy_quality_does_not_override_explicit_or_default_geometry() {
    let cfg: AppConfig = toml::from_str(
        r#"
        [airplay]
        quality = "ultra"
        width = 1280
        height = 720
        "#,
    )
    .unwrap();
    assert_eq!(resolve_display_config(&cfg.airplay), (1280, 720, 60, 60));
    assert_eq!(cfg.airplay.legacy_quality.as_deref(), Some("ultra"));
}
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```powershell
cargo test -p airplay-app explicit_display_fields_are_used_verbatim
cargo test -p airplay-app legacy_quality_does_not_override_explicit_or_default_geometry
```

Expected: compilation failure because `resolve_display_config` and `legacy_quality` do not exist.

- [ ] **Step 3: Implement explicit configuration**

Use this shape in `crates/airplay-app/src/main.rs`:

```rust
#[derive(Debug, Deserialize)]
struct AirplaySection {
    #[serde(default = "default_server_name")]
    server_name: String,
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "default_fps", alias = "max_fps", alias = "maxFPS")]
    fps: u32,
    #[serde(default = "default_refresh_rate", alias = "refreshRate")]
    refresh_rate: u32,
    #[serde(default, rename = "quality")]
    legacy_quality: Option<String>,
}

fn resolve_display_config(section: &AirplaySection) -> (u32, u32, u32, u32) {
    (
        section.width.max(1),
        section.height.max(1),
        section.fps.clamp(1, 120),
        section.refresh_rate.clamp(1, 240),
    )
}
```

Log one warning when `legacy_quality.is_some()` stating that the field is ignored and that the receiver does not own the sender's H.264 bitrate. Remove `default_quality`, `resolve_display_quality`, preset labels, and preset-oriented starter-config text.

- [ ] **Step 4: Update configuration examples and README**

Use this exact display example in both TOML files and the main README example:

```toml
[airplay]
server_name = "airplay2-rust"
width = 1280
height = 720
fps = 60
refresh_rate = 60
```

Document that these are advertised display limits, not an OBS-style bitrate control. Keep `[player] preview_mode` documentation separate.

- [ ] **Step 5: Verify GREEN**

Run:

```powershell
cargo test -p airplay-app
cargo check -p airplay-app --no-default-features --features h264-dump
```

Expected: all app tests pass and the dump-only feature compiles.

- [ ] **Step 6: Commit only Task 1 files**

```powershell
git add crates/airplay-app/src/main.rs config.toml crates/airplay-app/config.example.toml README.md
git commit -m "fix(app): use explicit AirPlay display settings"
```

---

### Task 2: Parse YouTube Media Plists and Build FCUP Events

**Files:**
- Create: `crates/airplay-server/src/control/media_protocol.rs`
- Modify: `crates/airplay-server/src/control/mod.rs`

**Interfaces:**
- Produces: `PlayRequest`, `MediaAction`, and `MediaProtocolError`.
- Produces: `parse_play_request(&[u8]) -> Result<PlayRequest, MediaProtocolError>`.
- Produces: `parse_action(&[u8]) -> Result<MediaAction, MediaProtocolError>`.
- Produces: `prepare_event_request(session_id: &str, remote_url: &str) -> Result<Vec<u8>, plist::Error>`.
- Consumed by: Task 7 HTTP media endpoints.

- [ ] **Step 1: Write failing unit tests**

Add tests within `media_protocol.rs` using programmatically generated sanitized plists:

```rust
fn plist_bytes(dict: plist::Dictionary) -> Vec<u8> {
    let mut bytes = Vec::new();
    Value::Dictionary(dict).to_writer_binary(&mut bytes).unwrap();
    bytes
}

#[test]
fn parses_youtube_play_request() {
    let mut dict = plist::Dictionary::new();
    dict.insert("clientProcName".into(), Value::String("YouTube".into()));
    dict.insert("clientBundleID".into(), Value::String("com.google.ios.youtube".into()));
    dict.insert("Content-Location".into(), Value::String("mlhls://localhost/master.m3u8".into()));
    let body = plist_bytes(dict);
    let play = parse_play_request(&body).unwrap();
    assert_eq!(play.client_proc_name, "YouTube");
    assert_eq!(play.content_location, "mlhls://localhost/master.m3u8");
}

#[test]
fn parses_unhandled_url_response_data_without_logging_it() {
    let mut params = plist::Dictionary::new();
    params.insert("FCUP_Response_URL".into(), Value::String("mlhls://localhost/master.m3u8".into()));
    params.insert("FCUP_Response_Data".into(), Value::Data(b"#EXTM3U\n".to_vec()));
    let mut dict = plist::Dictionary::new();
    dict.insert("type".into(), Value::String("unhandledURLResponse".into()));
    dict.insert("params".into(), Value::Dictionary(params));
    let body = plist_bytes(dict);
    assert_eq!(
        parse_action(&body).unwrap(),
        MediaAction::UnhandledUrlResponse {
            url: "mlhls://localhost/master.m3u8".into(),
            data: b"#EXTM3U\n".to_vec(),
        }
    );
}

#[test]
fn event_request_matches_java_field_shape() {
    let bytes = prepare_event_request("session-1", "mlhls://localhost/master.m3u8").unwrap();
    let value = Value::from_reader_xml(bytes.as_slice()).unwrap();
    let request = value.as_dictionary().unwrap()["request"].as_dictionary().unwrap();
    assert_eq!(request["FCUP_Response_URL"].as_string(), Some("mlhls://localhost/master.m3u8"));
    assert_eq!(request["sessionID"].as_signed_integer(), Some(1));
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
cargo test -p airplay-server media_protocol
```

Expected: compilation failure because the new types and functions are absent.

- [ ] **Step 3: Implement strict plist parsing**

Implement these public types:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayRequest {
    pub client_proc_name: String,
    pub content_location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaAction {
    UnhandledUrlResponse { url: String, data: Vec<u8> },
    PlaylistRemove,
    Unsupported(String),
}

#[derive(Debug, thiserror::Error)]
pub enum MediaProtocolError {
    #[error("invalid property list: {0}")]
    Plist(#[from] plist::Error),
    #[error("missing or invalid field {0}")]
    Field(&'static str),
}
```

Parse with `Value::from_reader(std::io::Cursor::new(body))`, require dictionary/string/data types exactly, and never include values or bodies in error text. Build the FCUP wrapper with the exact Java fields `FCUP_Response_ClientInfo`, `FCUP_Response_ClientRef`, `FCUP_Response_Headers`, `FCUP_Response_RequestID`, `FCUP_Response_URL`, `sessionID`, and `type=unhandledURLRequest`.

- [ ] **Step 4: Verify GREEN and commit**

Run:

```powershell
cargo test -p airplay-server media_protocol
```

Expected: all new parsing and event-shape tests pass.

```powershell
git add crates/airplay-server/src/control/media_protocol.rs crates/airplay-server/src/control/mod.rs
git commit -m "feat(server): parse AirPlay media control plists"
```

---

### Task 3: Implement Session-Scoped Playlist URL and Rewriting Logic

**Files:**
- Create: `crates/airplay-server/src/control/playlist.rs`
- Create: `crates/airplay-server/tests/fixtures/youtube_master.m3u8`
- Create: `crates/airplay-server/tests/fixtures/youtube_media_condensed.m3u8`
- Modify: `crates/airplay-server/src/control/mod.rs`

**Interfaces:**
- Produces: `local_playlist_url(control_port: u16, session_id: &str, remote_url: &str) -> Result<String, PlaylistError>`.
- Produces: `remote_playlist_url(local_path_and_query: &str) -> Result<(String, String), PlaylistError>` returning `(session_id, remote_mlhls_url)`.
- Produces: `rewrite_playlist(body: &[u8], control_port: u16, session_id: &str) -> Result<Vec<u8>, PlaylistError>`.
- Consumed by: Task 7 playlist proxy and `/action` fulfillment.

- [ ] **Step 1: Add sanitized fixtures and failing tests**

Use a fixture with relative managed-live-HLS URIs:

```m3u8
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",URI="mlhls://localhost/audio.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2500000,AUDIO="audio"
mlhls://localhost/video.m3u8
```

Use a media fixture with synthetic condensed data:

```m3u8
#EXTM3U
#YT-EXT-CONDENSED-URL:BASE-URI="https://example.invalid/videoplayback",PREFIX="seg/",PARAMS="id,range"
#EXTINF:2.0,
seg/demo/0-999
```

Add tests:

```rust
#[test]
fn local_and_remote_playlist_urls_round_trip() {
    let local = local_playlist_url(7000, "session-1", "mlhls://localhost/master.m3u8").unwrap();
    assert_eq!(local, "http://127.0.0.1:7000/playlist/master.m3u8?session=session-1");
    assert_eq!(
        remote_playlist_url("/playlist/master.m3u8?session=session-1").unwrap(),
        ("session-1".into(), "mlhls://localhost/master.m3u8".into())
    );
}

#[test]
fn rewrites_master_uris_and_preserves_tags() {
    let out = rewrite_playlist(include_bytes!("../../tests/fixtures/youtube_master.m3u8"), 7000, "session-1").unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("URI=\"http://127.0.0.1:7000/playlist/audio.m3u8?session=session-1\""));
    assert!(text.contains("http://127.0.0.1:7000/playlist/video.m3u8?session=session-1"));
    assert!(text.contains("#EXT-X-STREAM-INF:BANDWIDTH=2500000"));
}

#[test]
fn expands_condensed_segment_url() {
    let out = rewrite_playlist(include_bytes!("../../tests/fixtures/youtube_media_condensed.m3u8"), 7000, "session-1").unwrap();
    assert!(String::from_utf8(out).unwrap().contains(
        "https://example.invalid/videoplayback/id/demo/range/0-999"
    ));
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
cargo test -p airplay-server playlist
```

Expected: compilation failure because the playlist functions do not exist.

- [ ] **Step 3: Implement URL validation and line-preserving rewriting**

Define:

```rust
const MAX_PLAYLIST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    #[error("playlist exceeds 2097152 bytes")]
    TooLarge,
    #[error("playlist is not UTF-8")]
    Utf8,
    #[error("invalid playlist URL")]
    Url,
    #[error("invalid condensed URL metadata")]
    Condensed,
}
```

Implement local `percent_encode_component` and strict `percent_decode_component` helpers over UTF-8 bytes: leave RFC 3986 unreserved bytes (`A-Z`, `a-z`, `0-9`, `-`, `.`, `_`, `~`) unchanged and encode every other byte as uppercase `%HH`; reject malformed escapes, control characters, NUL, empty identifiers, and decoded identifiers longer than 256 bytes. This avoids a network-fetched dependency while satisfying session encoding. Accept remote URLs only when they begin with `mlhls://localhost/`. Rewrite:

- non-comment URI lines beginning `mlhls://localhost/`;
- quoted `URI="mlhls://localhost/..."` attributes without changing other attributes;
- condensed segment paths only when `BASE-URI`, `PREFIX`, and `PARAMS` are all valid and the number of segment values equals the number of parameter names.

Preserve all unmodified lines and choose the input's final-newline behavior. Do not normalize or reorder tags.

- [ ] **Step 4: Add rejection tests and verify GREEN**

Add assertions for unknown session characters, non-local `mlhls` hosts, oversized bodies, missing condensed attributes, and mismatched parameter counts.

Run:

```powershell
cargo test -p airplay-server playlist
```

Expected: all URL, rewrite, preservation, and rejection tests pass.

- [ ] **Step 5: Commit**

```powershell
git add crates/airplay-server/src/control/playlist.rs crates/airplay-server/src/control/mod.rs crates/airplay-server/tests/fixtures/youtube_master.m3u8 crates/airplay-server/tests/fixtures/youtube_media_condensed.m3u8
git commit -m "feat(server): rewrite managed live HLS playlists"
```

---

### Task 4: Add Reverse and Pending-Playlist Session State

**Files:**
- Modify: `crates/airplay-server/src/session.rs`
- Modify: `crates/airplay-server/src/control/codec.rs`
- Modify: `crates/airplay-server/src/control/mod.rs`

**Interfaces:**
- Produces: `OutboundRequest { method, path, headers, body }` for reverse writers; Task 5 adds its wire methods.
- Produces: `register_reverse`, `reverse_sender`, `remove_reverse_if_generation`, `register_playlist`, `fulfill_playlist`, `remove_pending_playlist`, and `cancel_media` on `SessionManager`.
- Consumed by: Tasks 6 and 7.

- [ ] **Step 1: Write failing async lifecycle tests**

Add Tokio tests using bounded `mpsc` and `oneshot` channels:

```rust
#[tokio::test]
async fn replacing_reverse_writer_does_not_let_old_disconnect_remove_new_writer() {
    let sessions = SessionManager::new();
    let (old_tx, _) = tokio::sync::mpsc::channel(2);
    let old_generation = sessions.register_reverse("s1", "event", old_tx);
    let (new_tx, _) = tokio::sync::mpsc::channel(2);
    let new_generation = sessions.register_reverse("s1", "event", new_tx.clone());
    sessions.remove_reverse_if_generation("s1", "event", old_generation);
    assert!(sessions.reverse_sender("s1", "event").is_some());
    sessions.remove_reverse_if_generation("s1", "event", new_generation);
    assert!(sessions.reverse_sender("s1", "event").is_none());
}

#[tokio::test]
async fn pending_playlist_is_one_shot_and_cancelled_with_media() {
    let sessions = SessionManager::new();
    let rx = sessions.register_playlist("s1", "mlhls://localhost/master.m3u8").unwrap();
    assert!(sessions.fulfill_playlist("s1", "mlhls://localhost/master.m3u8", b"ok".to_vec()));
    assert_eq!(rx.await.unwrap(), b"ok");
    assert!(!sessions.fulfill_playlist("s1", "mlhls://localhost/master.m3u8", b"late".to_vec()));
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
cargo test -p airplay-server session::tests
```

Expected: compilation failure for the absent session APIs.

- [ ] **Step 3: Implement bounded session state**

First define the message type in `control/codec.rs` and re-export it from `control/mod.rs`:

```rust
#[derive(Debug, Clone)]
pub struct OutboundRequest {
    pub method: &'static str,
    pub path: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
```

Add to `Session`:

```rust
pub struct ReverseWriter {
    pub generation: u64,
    pub tx: tokio::sync::mpsc::Sender<crate::control::OutboundRequest>,
}

pub reverse_writers: HashMap<String, ReverseWriter>,
pub pending_playlists: HashMap<String, tokio::sync::oneshot::Sender<Vec<u8>>>,
pub next_reverse_generation: u64,
```

Use channel capacity `8` in the connection layer and reject a duplicate pending remote URL instead of silently replacing its waiter. `cancel_media` drains pending senders, causing their receivers to close, but does not stop RTSP audio/video tasks. Session removal still calls both media cancellation and existing `stop_media`.

- [ ] **Step 4: Verify GREEN and commit**

Run:

```powershell
cargo test -p airplay-server session::tests
```

Expected: all existing and new session tests pass.

```powershell
git add crates/airplay-server/src/session.rs crates/airplay-server/src/control/codec.rs crates/airplay-server/src/control/mod.rs
git commit -m "feat(server): track AirPlay reverse media state"
```

---

### Task 5: Add Reverse-Connection Wire Framing

**Files:**
- Modify: `crates/airplay-server/src/control/codec.rs`
- Modify: `crates/airplay-server/src/control/mod.rs`

**Interfaces:**
- Consumes: `OutboundRequest` defined by Task 4.
- Produces: `OutboundRequest::post_event(session_id, body)` and `OutboundRequest::to_bytes()`.
- Produces: `read_response<R: AsyncRead + Unpin>(&mut R, &mut Vec<u8>) -> io::Result<Option<ControlResponseHead>>`.
- Consumed by: Task 6 reverse connection loop.

- [ ] **Step 1: Write failing framing tests**

```rust
#[test]
fn outbound_event_request_has_required_headers() {
    let bytes = OutboundRequest::post_event("s1", b"<plist/>".to_vec()).to_bytes();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("POST /event HTTP/1.1\r\n"));
    assert!(text.contains("Content-Type: text/x-apple-plist+xml\r\n"));
    assert!(text.contains("X-Apple-Session-ID: s1\r\n"));
    assert!(text.contains("Content-Length: 8\r\n\r\n<plist/>"));
}

#[tokio::test]
async fn parses_reverse_http_response_and_preserves_next_frame() {
    let wire = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nokHTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n";
    let mut reader = tokio::io::BufReader::new(wire.as_slice());
    let mut pending = Vec::new();
    assert_eq!(read_response(&mut reader, &mut pending).await.unwrap().unwrap().status, 200);
    assert_eq!(read_response(&mut reader, &mut pending).await.unwrap().unwrap().status, 204);
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
cargo test -p airplay-server control::codec::tests
```

Expected: compilation failure because outbound and response framing are missing.

- [ ] **Step 3: Implement framing with shared limits**

Add the `OutboundRequest` methods and define the response type:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlResponseHead {
    pub status: u16,
    pub reason: String,
    pub body_len: usize,
}
```

Reuse `MAX_HEADER_BYTES` and `MAX_BODY_BYTES`. `read_response` must consume the declared response body without retaining or logging it, preserve pipelined bytes, and reject malformed status lines or excessive lengths.

- [ ] **Step 4: Verify GREEN and commit**

Run:

```powershell
cargo test -p airplay-server control::codec::tests
```

Expected: all request, response, pipelining, and size-limit tests pass.

```powershell
git add crates/airplay-server/src/control/codec.rs crates/airplay-server/src/control/mod.rs
git commit -m "feat(server): frame AirPlay reverse HTTP traffic"
```

---

### Task 6: Upgrade `/reverse` Connections and Run the Writer Loop

**Files:**
- Modify: `crates/airplay-server/src/control/handler.rs`
- Modify: `crates/airplay-server/src/server.rs`
- Modify: `crates/airplay-server/src/control/codec.rs`
- Test: `crates/airplay-server/tests/control_info_test.rs`

**Interfaces:**
- Consumes: Task 4 reverse registration and Task 5 framing.
- Produces: `HandlerResult { response: ControlResponse, directive: ConnectionDirective }`.
- Produces: `ConnectionDirective::UpgradeReverse { session_id, purpose }`.
- Consumed by: Task 7 FCUP event dispatch.

- [ ] **Step 1: Write a failing in-process reverse-upgrade test**

Extend the existing server integration test to send:

```text
POST /reverse HTTP/1.1
X-Apple-Session-ID: session-1
X-Apple-Purpose: event
Upgrade: PTTH/1.0
Connection: Upgrade
Content-Length: 0


```

Assert the response begins with `HTTP/1.1 101 Switching Protocols`, includes `Upgrade: PTTH/1.0` and `Connection: Upgrade`, and that the socket remains open for an outbound event request.

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
cargo test -p airplay-server --test control_info_test reverse
```

Expected: FAIL because the current handler returns HTTP 200 and the connection remains in the normal request loop.

- [ ] **Step 3: Add connection directives**

Define in `handler.rs`:

```rust
pub struct HandlerResult {
    pub response: ControlResponse,
    pub directive: ConnectionDirective,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionDirective {
    Continue,
    UpgradeReverse { session_id: String, purpose: String },
}
```

All existing routes return `Continue`. `/reverse` requires `X-Apple-Session-ID`, `X-Apple-Purpose`, and `Upgrade`, returns 400 when absent, and otherwise returns status 101 with the upgrade directive. Add `ControlResponse::switching_protocols(upgrade: &str)` instead of misusing `ok_http`.

- [ ] **Step 4: Implement the reverse transport loop**

After writing the 101 response in `server.rs`:

1. create `mpsc::channel::<OutboundRequest>(8)`;
2. register its sender and remember the generation;
3. enter `run_reverse_connection(reader, writer, receiver)`;
4. `tokio::select!` between outbound requests and `read_response`;
5. log only status, purpose, and session—not bodies or URLs;
6. remove the registration only if its generation still matches.

Use this loop shape:

```rust
loop {
    tokio::select! {
        outbound = rx.recv() => match outbound {
            Some(request) => writer.write_all(&request.to_bytes()).await?,
            None => break,
        },
        response = read_response(&mut reader, &mut pending) => match response? {
            Some(head) => tracing::debug!(status = head.status, "reverse response"),
            None => break,
        },
    }
}
```

- [ ] **Step 5: Verify GREEN and regression tests**

Run:

```powershell
cargo test -p airplay-server --test control_info_test
cargo test -p airplay-server
```

Expected: reverse upgrade passes and all existing control/session tests remain green.

- [ ] **Step 6: Commit**

```powershell
git add crates/airplay-server/src/control/handler.rs crates/airplay-server/src/server.rs crates/airplay-server/src/control/codec.rs crates/airplay-server/tests/control_info_test.rs
git commit -m "feat(server): support AirPlay reverse connections"
```

---

### Task 7: Implement YouTube Media HTTP Endpoints and Playlist Proxy

**Files:**
- Modify: `crates/airplay-server/src/control/handler.rs`
- Modify: `crates/airplay-server/src/plist_util.rs`
- Create: `crates/airplay-server/tests/youtube_media_test.rs`

**Interfaces:**
- Consumes: Tasks 2-6 protocol, playlist, session, and reverse APIs.
- Produces: functional `/play`, `/playlist/*`, `/action`, `/rate`, `/stop`, and `/playback-info` routes.
- Calls: existing `AirPlayConsumer` media methods.

- [ ] **Step 1: Write a failing end-to-end media-control test**

Create a test consumer recording playlist, pause, resume, and remove calls. Start the in-process server, open an event reverse connection, then:

1. send a binary `/play` plist containing YouTube and `mlhls://localhost/master.m3u8`;
2. assert `on_media_playlist` receives a `127.0.0.1` proxy URL;
3. request that proxy path on a second connection;
4. assert the reverse connection receives a Java-shaped `POST /event`;
5. send `/action` with matching URL and synthetic playlist data;
6. assert the proxy request receives the rewritten playlist.

The core assertion should be:

```rust
assert_eq!(consumer.playlists.lock().unwrap().as_slice(), &[
    format!("http://127.0.0.1:{port}/playlist/master.m3u8?session=session-1")
]);
assert!(proxy_body.contains("http://127.0.0.1:"));
assert!(!proxy_body.contains("mlhls://localhost"));
```

- [ ] **Step 2: Run the integration test and verify RED**

Run:

```powershell
cargo test -p airplay-server --test youtube_media_test -- --nocapture
```

Expected: FAIL because `/play` and `/action` are stubs and `/playlist/*` returns 404.

- [ ] **Step 3: Implement `/play`**

Use `X-Apple-Session-ID` as the media session key. Parse with `parse_play_request`; accept `clientProcName == "YouTube"`; support `mlhls://localhost/...` through `local_playlist_url`; allow direct `http`/`https` URLs only after scheme validation. Call `consumer.on_media_playlist` only after validation succeeds. Return 400 for malformed input and 501 for unsupported clients/schemes.

- [ ] **Step 4: Implement `/playlist/*` with timeout**

Resolve `(session_id, remote_url)`, register the pending one-shot, build the FCUP body, and `try_send` an `OutboundRequest::post_event` through the session's `event` writer. Await the response with:

```rust
match tokio::time::timeout(Duration::from_secs(8), pending_rx).await {
    Ok(Ok(body)) => match rewrite_playlist(&body, self.control_port, &session_id) {
        Ok(rewritten) => ControlResponse::ok_http()
            .header("Content-Type", "application/vnd.apple.mpegurl")
            .with_body(rewritten),
        Err(error) => {
            tracing::warn!(session = %session_id, %error, "playlist rewrite rejected");
            ControlResponse::new("HTTP/1.1", 502, "Bad Gateway")
        }
    },
    Ok(Err(_)) => ControlResponse::new("HTTP/1.1", 502, "Bad Gateway"),
    Err(_) => ControlResponse::new("HTTP/1.1", 504, "Gateway Timeout"),
}
```

Remove the pending entry on send failure or timeout. Never hold a session lock during the timeout.

- [ ] **Step 5: Implement `/action`, controls, and playback info**

- `UnhandledUrlResponse`: fulfill the exact session/URL pending sender.
- `PlaylistRemove`: call `on_media_playlist_remove` and `cancel_media`.
- `/rate?value=0`: pause; any finite nonzero value: resume; malformed values: 400.
- `/stop`: remove media playlist and cancel pending requests.
- `/playback-info`: call `prepare_playback_info_response(&consumer.playback_info())`, set XML content type, and return the body.

Add query parsing limited to ASCII keys/values needed by these routes; reject duplicate `session` or `value` parameters.

- [ ] **Step 6: Add failure and lifecycle cases**

Test missing reverse connection (502), pending timeout (504 with paused Tokio time), malformed plist (400), unsupported client (501), unmatched late action (200 but discarded), pause/resume calls, stop cancellation, and XML playback-info fields.

- [ ] **Step 7: Verify GREEN and commit**

Run:

```powershell
cargo test -p airplay-server --test youtube_media_test -- --nocapture
cargo test -p airplay-server
```

Expected: all media integration and server regression tests pass.

```powershell
git add crates/airplay-server/src/control/handler.rs crates/airplay-server/src/plist_util.rs crates/airplay-server/tests/youtube_media_test.rs
git commit -m "feat(server): proxy YouTube AirPlay playlists"
```

---

### Task 8: Make GStreamer Media Playback Safe and Observable

**Files:**
- Modify: `crates/airplay-player/src/gstreamer_player.rs`

**Interfaces:**
- Consumes: `AirPlayConsumer::on_media_playlist` and related controls.
- Produces: one separately managed `playbin3` media element with bus diagnostics and clean replacement/removal.

- [ ] **Step 1: Write a failing media element construction test**

Extract a helper and test it behind the GStreamer feature:

```rust
#[test]
#[ignore = "requires installed GStreamer playback plugins"]
fn media_playbin_sets_uri_as_property() {
    gst::init().unwrap();
    let playbin = build_media_playbin("http://127.0.0.1:7000/playlist/master.m3u8").unwrap();
    assert_eq!(
        playbin.property::<String>("uri"),
        "http://127.0.0.1:7000/playlist/master.m3u8"
    );
    playbin.set_state(gst::State::Null).unwrap();
}
```

- [ ] **Step 2: Run the test and verify RED**

Run with the repository's absolute runner override:

```powershell
cargo --config "target.x86_64-pc-windows-msvc.runner=['cmd','/C','F:\airplay2-rust\scripts\gst-runner.cmd']" test -p airplay-player --features gstreamer media_playbin_sets_uri_as_property -- --ignored
```

Expected: compilation failure because `build_media_playbin` does not exist.

- [ ] **Step 3: Build `playbin3` without pipeline-string interpolation**

Implement:

```rust
fn build_media_playbin(uri: &str) -> Result<gst::Element, String> {
    if !(uri.starts_with("http://") || uri.starts_with("https://")) {
        return Err("unsupported media URI scheme".into());
    }
    let playbin = gst::ElementFactory::make("playbin3")
        .name("airplay-media")
        .property("uri", uri)
        .build()
        .map_err(|error| format!("create playbin3: {error}"))?;
    Ok(playbin)
}
```

Change `hls_pipeline` to `Mutex<Option<gst::Element>>`. Install a bus sync handler before `PLAYING` that logs source, error category, redacted URI kind (`loopback-playlist` or `https`), state transitions, buffering percent, and EOS. Do not log the URI itself.

- [ ] **Step 4: Implement replacement and cleanup semantics**

Construct and prepare the new element before locking the slot. Stop the previous element before replacement. On construction or state failure, leave no half-installed element. `/stop`, playlist removal, player drop, and a new screen-mirror format set the media element to `NULL` and clear the slot.

Apply window title and force-aspect-ratio to media sinks through the existing deep-element hook where supported.

- [ ] **Step 5: Verify GREEN and feature isolation**

Run:

```powershell
cargo --config "target.x86_64-pc-windows-msvc.runner=['cmd','/C','F:\airplay2-rust\scripts\gst-runner.cmd']" test -p airplay-player --features gstreamer
cargo check -p airplay-app --no-default-features --features h264-dump
```

Expected: player tests pass and dump-only compilation remains GStreamer-free.

- [ ] **Step 6: Commit**

```powershell
git add crates/airplay-player/src/gstreamer_player.rs
git commit -m "feat(player): play AirPlay media playlists safely"
```

---

### Task 9: Documentation, Full Verification, and Windows Device Validation

**Files:**
- Modify: `crates/airplay-lib/src/pairing.rs`
- Modify: `README.md`
- Modify: `docs/superpowers/plans/acceptance-checklist.md`

**Interfaces:**
- Documents: explicit display settings, YouTube mode switching, required plugins, diagnostics, and acceptance boundary.

- [ ] **Step 1: Update user documentation**

Document:

- explicit `1280x720@60` configuration and absence of bitrate control;
- `implementation = "gstreamer"` and `preview_mode = "low-latency"` for live use;
- YouTube's transition from mirror window to media window;
- required `playbin3`, HLS, HTTPS, demux, decode, audio, and video-sink plugins;
- `gst-inspect-1.0 playbin3`, `gst-inspect-1.0 hlsdemux2`, and existing D3D11 inspection commands;
- redacted troubleshooting messages for missing reverse channel, playlist timeout, GStreamer error, and unsupported client;
- the statement that receiver quality cannot exceed the stream chosen by the sender.

Also remove the existing `tracing::info!` field that prints the ECDH shared secret in `crates/airplay-lib/src/pairing.rs`. Replace it with `tracing::debug!("pairing shared secret established")`; do not change secret calculation, storage, or verification.

Before changing the log, run this regression guard and verify it fails by printing the offending line:

```powershell
rg -n "secret\s*=\s*%hex_encode" crates/airplay-lib/src/pairing.rs
```

After the change, rerun it and expect no matches, then run `cargo test -p airplay-lib pairing_test` to prove pairing behavior remains unchanged.

- [ ] **Step 2: Run formatting and scoped hygiene checks**

Run:

```powershell
rustfmt --edition 2021 --check crates/airplay-app/src/main.rs crates/airplay-lib/src/pairing.rs crates/airplay-server/src/control/media_protocol.rs crates/airplay-server/src/control/playlist.rs crates/airplay-server/src/control/codec.rs crates/airplay-server/src/control/handler.rs crates/airplay-server/src/server.rs crates/airplay-server/src/session.rs crates/airplay-player/src/gstreamer_player.rs
git diff --check -- crates/airplay-app/src/main.rs crates/airplay-lib/src/pairing.rs config.toml crates/airplay-app/config.example.toml crates/airplay-server crates/airplay-player/src/gstreamer_player.rs README.md docs/superpowers/plans/acceptance-checklist.md
cargo fmt --all -- --check
```

Expected: focused Rust files and scoped diff pass. If the workspace-wide formatter still reports unrelated pre-existing crypto/client drift, record that exact limitation and do not reformat line-faithful crypto.

- [ ] **Step 3: Run complete automated verification**

Run:

```powershell
cargo check --workspace
cargo --config "target.x86_64-pc-windows-msvc.runner=['cmd','/C','F:\airplay2-rust\scripts\gst-runner.cmd']" test --workspace
cargo check -p airplay-app --no-default-features --features h264-dump
cargo check -p airplay-app --features gstreamer
cargo --config "target.x86_64-pc-windows-msvc.runner=['cmd','/C','F:\airplay2-rust\scripts\gst-runner.cmd']" test -p airplay-player --features gstreamer media_playbin_sets_uri_as_property -- --ignored
gst-inspect-1.0 playbin3
gst-inspect-1.0 hlsdemux2
gst-inspect-1.0 d3d11h264dec
gst-inspect-1.0 d3d11videosink
```

Expected: checks/tests pass; runtime element inspection exits zero. Ignored mDNS tests remain explicitly identified.

- [ ] **Step 4: Perform the live Windows/iPhone YouTube test**

Start the receiver with the root `config.toml`, connect Screen Mirroring, then press AirPlay inside YouTube. Verify in order:

1. direct preview runs with selected D3D11 decoder/sink;
2. mirror video TEARDOWN closes the mirror window;
3. `/play` logs supported client and redacted `mlhls` path metadata;
4. GStreamer opens a media window and reaches `PLAYING`;
5. video and audio play;
6. YouTube pause/resume changes the media state;
7. YouTube disconnect removes the media window;
8. reconnecting Screen Mirroring restores the direct preview;
9. logs contain no pairing secrets, signed URLs, playlist bodies, or FCUP response data.

If any live step fails, preserve only redacted method/path/status logs and add a focused regression test before changing code.

- [ ] **Step 5: Update acceptance checklist honestly**

Mark automated items only from captured command output. Mark YouTube live playback complete only if all nine live steps pass; otherwise state “implemented, awaiting device validation” with the failing boundary.

- [ ] **Step 6: Commit documentation**

```powershell
git add crates/airplay-lib/src/pairing.rs README.md docs/superpowers/plans/acceptance-checklist.md
git commit -m "docs: document YouTube AirPlay media playback"
```

---

## Final Review Gate

Before reporting completion:

1. use `superpowers:requesting-code-review` on the entire task-scoped diff;
2. address Critical and Important findings with failing regression tests;
3. rerun Task 9 verification after the final code change;
4. use `superpowers:verification-before-completion` before any success claim;
5. distinguish automated protocol/runtime verification from the live iPhone YouTube result.
