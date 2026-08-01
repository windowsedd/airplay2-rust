# YouTube AirPlay Media Playback Design

**Date:** 2026-08-01

**Status:** Approved for implementation planning

**Scope:** Explicit display configuration and Java-faithful YouTube AirPlay media playback

## 1. Goals

1. Replace misleading display-quality presets with explicit receiver display fields:
   `width`, `height`, `fps`, and `refresh_rate`.
2. Support the mode switch made by the iOS YouTube app from screen mirroring to
   AirPlay media playback.
3. Port the reverse-channel and managed-live-HLS behavior from the bundled
   `java-airplay-2open` protocol reference without changing pairing, FairPlay,
   OmgHax, HandGarble, SapHash, or other cryptographic behavior.

The receiver remains an educational and research project. This work does not
claim general compatibility with every AirPlay media application.

## 2. Current Behavior and Root Cause

Screen mirroring uses the RTSP video stream and the direct GStreamer H.264
pipeline. When the user presses AirPlay inside YouTube, the sender deliberately
tears down that video stream and remains connected in media-playback mode.

The Rust HTTP handler currently returns success for `/reverse`, `/play`,
`/action`, `/rate`, `/stop`, playlist routes, and `/playback-info` without
implementing their protocol behavior. Consequently, YouTube reports that it is
connected, the mirroring window disappears as expected, but no replacement
media pipeline receives a playable URL.

The Java reference implements the missing path by keeping an upgraded reverse
HTTP connection, requesting `mlhls://localhost` playlists from the sender,
rewriting them through a receiver-local proxy, and passing that proxy URL to the
player.

## 3. Explicit Display Configuration

The supported configuration becomes:

```toml
[airplay]
server_name = "airplay2-rust"
width = 1280
height = 720
fps = 60
refresh_rate = 60
```

The `quality` field and its `low`, `medium`, `high`, and `ultra` mappings are
removed. Missing explicit fields retain the existing safe defaults so older
configuration files still deserialize. If a legacy file contains `quality`, it
is ignored with a structured warning explaining that the receiver cannot set
the iPhone encoder bitrate.

No `bitrate` setting is added. OBS can control bitrate because it owns its
encoder; this receiver does not. AirPlay negotiation in the Java reference
advertises display geometry, `maxFPS`, and `refreshRate`, but no receiver-owned
H.264 bitrate.

The player `preview_mode` remains separate because it controls receiver-side
buffering and sink synchronization, not sender encoding quality.

## 4. Media-Control Architecture

### 4.1 Connection roles

The control server must support both ordinary request/response connections and
upgraded reverse connections. A successful `POST /reverse` returns HTTP 101 and
registers the connection by AirPlay session and `X-Apple-Purpose` (notably
`event`).

Each connection owns its Tokio write half. Outbound reverse requests are sent
through a bounded per-connection channel so no session mutex is held across
network I/O. Closing a socket removes only the matching registered writer.
Replacing a reverse connection atomically retires the older writer.

### 4.2 Session media state

Each session stores:

- reverse-channel senders keyed by purpose;
- pending local playlist responses keyed by the corresponding remote
  `mlhls://localhost` URL;
- current media-playback identity and cancellation state.

Pending requests use bounded queues and timeouts. Session removal, `/stop`,
playlist removal, or connection loss cancels pending responses and stops the
media consumer. Stale responses cannot satisfy a newer session request.

### 4.3 `/play`

`POST /play` parses the binary or XML plist body with the existing `plist`
dependency. For the supported YouTube path it validates `clientProcName` and
`Content-Location`.

An `mlhls://localhost/...` location is mapped to:

```text
http://127.0.0.1:<control-port>/playlist/...?...session=<encoded-session-id>
```

The loopback URL is passed to `AirPlayConsumer::on_media_playlist`. Direct
`http://` or `https://` media URLs may be passed through only when explicitly
present in a valid play request. Unsupported schemes, missing fields, and
unsupported clients receive an honest 400 or 501 response and structured logs.

### 4.4 Playlist proxy and reverse events

When GStreamer requests `/playlist/...`, the handler:

1. resolves and validates the session query parameter;
2. converts the local path back to the corresponding
   `mlhls://localhost/...` URL;
3. registers a one-shot pending response;
4. sends the Java-compatible FCUP `/event` plist over the session's `event`
   reverse connection;
5. waits for a bounded timeout without blocking unrelated control requests.

`POST /action` handles `unhandledURLResponse`. It validates
`FCUP_Response_URL` and `FCUP_Response_Data`, locates the matching pending
request, rewrites the returned playlist, and fulfills the waiting local HTTP
response. Unknown or late response URLs are logged and discarded.

### 4.5 Playlist rewriting

Master playlists rewrite variant and alternative-rendition URIs from
`mlhls://localhost` to session-scoped loopback proxy URLs.

Media playlists preserve ordering, tags, durations, and unknown comments. When
the Java-specific `YT-EXT-CONDENSED-URL` comment is present, segment URLs are
expanded using its `BASE-URI`, `PREFIX`, and `PARAMS` attributes exactly as the
Java reference does. Malformed attributes fail that playlist request instead of
constructing a guessed URL.

Playlist parsing and rewriting live in a pure server module so they can be
tested without sockets, GStreamer, a GPU, or a YouTube account.

## 5. Playback Controls and Player Lifecycle

- `/rate?value=0` pauses the media consumer; nonzero values resume it.
- `/stop` and `playlistRemove` stop media playback and cancel pending playlist
  requests.
- `/playback-info` returns the existing XML plist populated from the consumer's
  actual duration and position.
- Unsupported property operations keep narrowly scoped responses and logs; they
  do not claim to change state they do not implement.

The GStreamer player constructs `playbin3` through `ElementFactory`, sets its
`uri` property directly, and installs bus logging before requesting `PLAYING`.
This avoids pipeline-string injection and exposes network, demux, decode, and
sink errors. The HLS pipeline has its own lifecycle and does not reuse the
screen-mirroring H.264 codec gate.

The expected transition is:

```text
screen mirror TEARDOWN -> mirror pipeline NULL
YouTube /play -> local playlist proxy -> playbin3 PLAYING
/stop or playlistRemove -> playbin3 NULL
new screen mirror SETUP -> direct H.264 pipeline PLAYING
```

## 6. Error Handling and Security

- Limit control bodies, playlist bodies, pending requests, and reverse-channel
  queues.
- Bind the playlist proxy to the existing receiver control listener but expose
  generated playback URLs only on `127.0.0.1`.
- Percent-encode session identifiers and reject missing or unknown sessions.
- Do not log playlist tokens, complete signed URLs, private media data, pairing
  secrets, or FCUP response bodies. Logs use redacted URL scheme/path metadata.
- Time out pending playlist requests and return an HTTP gateway error rather
  than hanging GStreamer indefinitely.
- Do not add telemetry, remote updates, downloading tools, or video
  re-encoding.

## 7. Testing

Automated tests cover:

- explicit display configuration and legacy `quality` handling;
- reverse-upgrade response and connection registration lifecycle;
- `/play` plist parsing and supported/unsupported URL handling;
- local/remote playlist URL conversion and session encoding;
- FCUP event plist construction;
- master-playlist rewriting;
- media-playlist condensed-URL expansion;
- malformed and oversized playlist rejection;
- pending-request success, timeout, cancellation, and stale-response behavior;
- `/rate`, `/stop`, `playlistRemove`, and `/playback-info` dispatch;
- GStreamer media element construction where runtime plugins are available.

Fixtures are copied or translated from the bundled Java reference where
possible and sanitized so no signed YouTube URLs, device captures, or private
content are committed.

## 8. Acceptance Boundary

Unit and integration tests can prove protocol framing, playlist rewriting,
session lifecycle, and GStreamer construction. Completion also requires a live
Windows test with an iPhone YouTube session verifying:

1. the mirroring window closes when YouTube switches modes;
2. a GStreamer media window opens;
3. video and audio play;
4. pause/resume and stop work;
5. reconnecting to screen mirroring restores the direct preview;
6. no signed URLs or pairing secrets appear in logs.

Until that live test passes, YouTube playback must be described as implemented
but awaiting device validation.
