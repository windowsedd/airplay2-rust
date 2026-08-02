//! Control request dispatch (RTSP + HTTP paths used by AirPlay receivers).

use std::sync::Arc;
use std::time::Duration;

use airplay_lib::MediaStreamInfo;
use tracing::{debug, error, info, warn};

use crate::config::AirPlayConfig;
use crate::consumer::AirPlayConsumer;
use crate::control::codec::{ControlRequest, ControlResponse, OutboundRequest};
use crate::control::media_protocol::{
    classify_play_request, parse_action, parse_play_request, parse_rate_value,
    parse_scrub_position, playlist_looks_protected, prepare_event_request, redact_media_url,
    MediaAction, MediaSupport,
};
use crate::control::playlist::{local_playlist_url, remote_playlist_url, rewrite_playlist};
use crate::media;
use crate::plist_util;
use crate::session::SessionManager;

/// What the connection layer should do after writing the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionDirective {
    Continue,
    UpgradeReverse {
        session_id: String,
        purpose: String,
    },
}

/// Response plus optional connection-level directive.
pub struct HandlerResult {
    pub response: ControlResponse,
    pub directive: ConnectionDirective,
}

impl HandlerResult {
    fn cont(response: ControlResponse) -> Self {
        Self {
            response,
            directive: ConnectionDirective::Continue,
        }
    }
}

/// Handles a single parsed control request.
pub struct ControlHandler {
    pub sessions: Arc<SessionManager>,
    pub config: AirPlayConfig,
    pub consumer: Arc<dyn AirPlayConsumer>,
    /// Control server listen port (used as eventPort in video SETUP + playlist proxy).
    pub control_port: u16,
}

impl ControlHandler {
    pub fn new(
        sessions: Arc<SessionManager>,
        config: AirPlayConfig,
        consumer: Arc<dyn AirPlayConsumer>,
        control_port: u16,
    ) -> Self {
        Self {
            sessions,
            config,
            consumer,
            control_port,
        }
    }

    /// Dispatch request → response + connection directive. Never panics on protocol errors.
    pub async fn handle(&self, request: &ControlRequest) -> HandlerResult {
        let method = request.method.to_ascii_uppercase();
        let path = request.path_only();

        log_control_request(request);

        if request.is_rtsp() {
            return HandlerResult::cont(self.handle_rtsp(&method, path, request).await);
        }
        if request.is_http() {
            return self.handle_http(&method, path, request).await;
        }

        warn!(version = %request.version, "unknown protocol version");
        HandlerResult::cont(ControlResponse::not_found(&request.version).with_cseq_from(request))
    }

    async fn handle_rtsp(
        &self,
        method: &str,
        path: &str,
        request: &ControlRequest,
    ) -> ControlResponse {
        match (method, path) {
            ("GET", "/info") => self.handle_get_info(request),
            ("POST", "/pair-setup") => self.handle_pair_setup(request),
            ("POST", "/pair-verify") => self.handle_pair_verify(request),
            ("POST", "/fp-setup") => self.handle_fp_setup(request),
            ("SETUP", _) => self.handle_setup(request).await,
            ("TEARDOWN", _) => self.handle_teardown(request),
            ("RECORD", _) => self.handle_record(request),
            ("GET_PARAMETER", _) => self.handle_get_parameter(request),
            ("SET_PARAMETER", _) => self.handle_set_parameter(request),
            ("FLUSH", _) => self.handle_flush(request),
            ("POST", "/feedback") => ok_rtsp(request),
            ("POST", "/audioMode") => ok_rtsp(request),
            _ => {
                error!(
                    method = %request.method,
                    path = %request.path,
                    "unknown RTSP control request"
                );
                ControlResponse::not_found(&request.version).with_cseq_from(request)
            }
        }
    }

    async fn handle_http(
        &self,
        method: &str,
        path: &str,
        request: &ControlRequest,
    ) -> HandlerResult {
        match (method, path) {
            ("GET", "/server-info") => HandlerResult::cont(self.handle_server_info(request)),
            ("POST", "/reverse") => self.handle_reverse(request),
            ("POST", "/play") => HandlerResult::cont(self.handle_play(request)),
            ("POST", "/rate") => HandlerResult::cont(self.handle_rate(request)),
            ("POST", "/action") => HandlerResult::cont(self.handle_action(request)),
            ("POST", "/stop") => HandlerResult::cont(self.handle_stop(request)),
            ("POST", "/scrub") => HandlerResult::cont(self.handle_scrub(request)),
            ("GET", "/playback-info") => HandlerResult::cont(self.handle_playback_info(request)),
            ("PUT", "/setProperty") => HandlerResult::cont(self.handle_set_property(request)),
            ("POST", "/getProperty") => {
                debug!(path, "getProperty (acknowledged, no-op)");
                HandlerResult::cont(ControlResponse::ok_http())
            }
            ("GET", p) if p.starts_with("/playlist") => {
                HandlerResult::cont(self.handle_get_playlist(request).await)
            }
            _ => {
                error!(
                    method = %request.method,
                    path = %request.path,
                    "unknown HTTP control request"
                );
                HandlerResult::cont(ControlResponse::not_found(&request.version))
            }
        }
    }

    fn handle_get_info(&self, request: &ControlRequest) -> ControlResponse {
        match plist_util::prepare_info_response(&self.config) {
            Ok(body) => ControlResponse::ok_rtsp()
                .with_cseq_from(request)
                .with_body(body),
            Err(e) => {
                error!("prepare_info_response failed: {e}");
                ControlResponse::bad_request(&request.version).with_cseq_from(request)
            }
        }
    }

    fn handle_pair_setup(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let pk = self.sessions.with_session(&sid, |s| {
            s.airplay.lock().expect("airplay lock").pair_setup()
        });
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .with_body(pk.to_vec())
    }

    fn handle_pair_verify(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let result = self.sessions.with_session(&sid, |s| {
            s.airplay
                .lock()
                .expect("airplay lock")
                .pair_verify(&request.body)
        });
        match result {
            Ok(body) => ControlResponse::ok_rtsp()
                .with_cseq_from(request)
                .with_body(body),
            Err(e) => {
                warn!(session = %sid, "pair-verify failed: {e}");
                ControlResponse::bad_request(&request.version).with_cseq_from(request)
            }
        }
    }

    fn handle_fp_setup(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let result = self.sessions.with_session(&sid, |s| {
            s.airplay
                .lock()
                .expect("airplay lock")
                .fair_play_setup(&request.body)
        });
        match result {
            Ok(body) => ControlResponse::ok_rtsp()
                .with_cseq_from(request)
                .with_body(body),
            Err(e) => {
                warn!(session = %sid, "fp-setup failed: {e}");
                ControlResponse::bad_request(&request.version).with_cseq_from(request)
            }
        }
    }

    async fn handle_setup(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let setup_result = self.sessions.with_session(&sid, |s| {
            s.airplay
                .lock()
                .expect("airplay lock")
                .rtsp_setup(&request.body)
        });

        let media_info = match setup_result {
            Ok(m) => m,
            Err(e) => {
                warn!(session = %sid, "rtsp_setup failed: {e}");
                return ControlResponse::bad_request(&request.version).with_cseq_from(request);
            }
        };

        match media_info {
            None => ok_rtsp(request),
            Some(MediaStreamInfo::Video(info)) => {
                let generation = self.sessions.next_generation();
                let previous_video_gen = self.sessions.with_session(&sid, |s| {
                    s.video_generation.replace(generation)
                });
                // Stop media playback if still active — mirror takes over.
                self.consumer.on_media_playlist_remove();
                self.sessions.cancel_media(&sid);
                self.consumer.on_video_format(&info, generation);
                if let Some(old_gen) = previous_video_gen {
                    self.consumer.on_video_src_disconnect(old_gen);
                }
                match media::video::bind().await {
                    Ok((listener, port)) => {
                        let (airplay, consumer) = (
                            self.sessions
                                .with_session(&sid, |s| Arc::clone(&s.airplay)),
                            Arc::clone(&self.consumer),
                        );
                        let handle =
                            media::video::run_accept(listener, airplay, consumer, generation);
                        self.sessions.with_session(&sid, |s| {
                            if let Some(old) = s.video_task.take() {
                                old.abort();
                            }
                            s.video_port = Some(port);
                            s.video_generation = Some(generation);
                            s.video_task = Some(handle);
                        });
                        info!(
                            session = %sid,
                            port,
                            generation,
                            "video SETUP bound media port"
                        );
                        match plist_util::prepare_setup_video_response(
                            port,
                            self.control_port,
                            0,
                        ) {
                            Ok(body) => ControlResponse::ok_rtsp()
                                .with_cseq_from(request)
                                .with_body(body),
                            Err(e) => {
                                error!("prepare_setup_video_response: {e}");
                                ControlResponse::bad_request(&request.version)
                                    .with_cseq_from(request)
                            }
                        }
                    }
                    Err(e) => {
                        error!("bind video port failed: {e}");
                        ControlResponse::bad_request(&request.version).with_cseq_from(request)
                    }
                }
            }
            Some(MediaStreamInfo::Audio(info)) => {
                let generation = self.sessions.next_generation();
                let previous_audio_gen = self.sessions.with_session(&sid, |s| {
                    s.audio_generation.replace(generation)
                });
                self.consumer.on_audio_format(&info, generation);
                if let Some(old_gen) = previous_audio_gen {
                    self.consumer.on_audio_src_disconnect(old_gen);
                }
                let data = media::audio::bind().await;
                let control = media::audio_control::bind().await;
                match (data, control) {
                    (Ok((data_sock, data_port)), Ok((ctrl_sock, ctrl_port))) => {
                        let airplay = self
                            .sessions
                            .with_session(&sid, |s| Arc::clone(&s.airplay));
                        let consumer = Arc::clone(&self.consumer);
                        let data_handle = media::audio::run_recv(data_sock, airplay, consumer);
                        let ctrl_handle = media::audio_control::run_recv(ctrl_sock);
                        self.sessions.with_session(&sid, |s| {
                            if let Some(old) = s.audio_task.take() {
                                old.abort();
                            }
                            if let Some(old) = s.audio_control_task.take() {
                                old.abort();
                            }
                            s.audio_port = Some(data_port);
                            s.audio_control_port = Some(ctrl_port);
                            s.audio_generation = Some(generation);
                            s.audio_task = Some(data_handle);
                            s.audio_control_task = Some(ctrl_handle);
                        });
                        info!(
                            session = %sid,
                            data_port,
                            ctrl_port,
                            generation,
                            "audio SETUP bound media ports"
                        );
                        match plist_util::prepare_setup_audio_response(data_port, ctrl_port) {
                            Ok(body) => ControlResponse::ok_rtsp()
                                .with_cseq_from(request)
                                .with_body(body),
                            Err(e) => {
                                error!("prepare_setup_audio_response: {e}");
                                ControlResponse::bad_request(&request.version)
                                    .with_cseq_from(request)
                            }
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        error!("bind audio ports failed: {e}");
                        ControlResponse::bad_request(&request.version).with_cseq_from(request)
                    }
                }
            }
        }
    }

    fn handle_teardown(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let media_info = self.sessions.with_session(&sid, |s| {
            s.airplay
                .lock()
                .expect("airplay lock")
                .rtsp_teardown(&request.body)
        });

        match media_info {
            Ok(Some(MediaStreamInfo::Audio(_))) => {
                let gen = self.sessions.with_session(&sid, |s| {
                    if let Some(h) = s.audio_task.take() {
                        h.abort();
                    }
                    if let Some(h) = s.audio_control_task.take() {
                        h.abort();
                    }
                    s.audio_port = None;
                    s.audio_control_port = None;
                    s.audio_generation.take()
                });
                if let Some(generation) = gen {
                    self.consumer.on_audio_src_disconnect(generation);
                }
            }
            Ok(Some(MediaStreamInfo::Video(_))) => {
                let gen = self.sessions.with_session(&sid, |s| {
                    if let Some(h) = s.video_task.take() {
                        h.abort();
                    }
                    s.video_port = None;
                    s.video_generation.take()
                });
                if let Some(generation) = gen {
                    self.consumer.on_video_src_disconnect(generation);
                }
            }
            Ok(None) => {
                let (video_gen, audio_gen) =
                    self.sessions.with_session(&sid, |s| s.stop_media());
                if let Some(generation) = audio_gen {
                    self.consumer.on_audio_src_disconnect(generation);
                }
                if let Some(generation) = video_gen {
                    self.consumer.on_video_src_disconnect(generation);
                }
            }
            Err(e) => {
                debug!(session = %sid, "rtsp_teardown parse note: {e}");
                let (video_gen, audio_gen) =
                    self.sessions.with_session(&sid, |s| s.stop_media());
                if let Some(generation) = audio_gen {
                    self.consumer.on_audio_src_disconnect(generation);
                }
                if let Some(generation) = video_gen {
                    self.consumer.on_video_src_disconnect(generation);
                }
            }
        }

        ok_rtsp(request)
    }

    fn handle_record(&self, request: &ControlRequest) -> ControlResponse {
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .header("Audio-Latency", "11025")
            .header("Audio-Jack-Status", "connected; type=analog")
    }

    fn handle_get_parameter(&self, request: &ControlRequest) -> ControlResponse {
        let volume_db = self.consumer.volume().unwrap_or(0.0).clamp(-144.0, 0.0);
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .with_body(format!("volume: {volume_db:.6}\r\n").into_bytes())
    }

    fn handle_set_parameter(&self, request: &ControlRequest) -> ControlResponse {
        if request.header("Content-Type").is_some_and(|content_type| {
            content_type
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/parameters"))
        }) {
            if let Some(volume_db) = parse_volume_parameter(&request.body) {
                info!(
                    db = volume_db,
                    source = "rtsp",
                    "AirPlay volume update"
                );
                self.consumer.on_volume(volume_db);
            }
        }
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .header("Audio-Jack-Status", "connected; type=analog")
    }

    fn handle_set_property(&self, request: &ControlRequest) -> ControlResponse {
        // Query may contain property name: /setProperty?volume or path-only with plist body.
        let query = request.path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let path_prop = query
            .split('&')
            .next()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && !s.contains('='));

        match parse_set_property_volume(&request.body, path_prop) {
            SetPropertyVolume::VolumeDb(db) => {
                info!(db, source = "setProperty", "AirPlay volume update");
                self.consumer.on_volume(db);
            }
            SetPropertyVolume::Muted(muted) => {
                info!(muted, source = "setProperty", "AirPlay mute update");
                self.consumer.on_mute(muted);
            }
            SetPropertyVolume::Both { db, muted } => {
                info!(db, source = "setProperty", "AirPlay volume update");
                self.consumer.on_volume(db);
                info!(muted, source = "setProperty", "AirPlay mute update");
                self.consumer.on_mute(muted);
            }
            SetPropertyVolume::None => {
                debug!(
                    path = %request.path_only(),
                    query = %query,
                    "setProperty without volume/mute keys (acknowledged)"
                );
            }
        }
        ControlResponse::ok_http()
    }

    fn handle_flush(&self, request: &ControlRequest) -> ControlResponse {
        ok_rtsp(request)
    }

    fn handle_server_info(&self, _request: &ControlRequest) -> ControlResponse {
        match plist_util::prepare_server_info_response() {
            Ok(body) => ControlResponse::ok_http()
                .header("Content-Type", "text/x-apple-plist+xml")
                .with_body(body),
            Err(e) => {
                error!("prepare_server_info_response failed: {e}");
                ControlResponse::bad_request("HTTP/1.1")
            }
        }
    }

    fn handle_reverse(&self, request: &ControlRequest) -> HandlerResult {
        let session_id = request
            .header("X-Apple-Session-ID")
            .or_else(|| request.header("Active-Remote"))
            .unwrap_or("")
            .to_string();
        let purpose = request
            .header("X-Apple-Purpose")
            .unwrap_or("")
            .to_string();
        let upgrade = request.header("Upgrade").unwrap_or("PTTH/1.0").to_string();

        if session_id.is_empty() || purpose.is_empty() {
            warn!("POST /reverse missing session id or purpose");
            return HandlerResult::cont(ControlResponse::bad_request("HTTP/1.1"));
        }

        // Ensure session exists before upgrade.
        self.sessions.with_session(&session_id, |_| {});

        info!(
            session = %session_id,
            purpose = %purpose,
            upgrade = %upgrade,
            "HTTP reverse upgrade"
        );

        HandlerResult {
            response: ControlResponse::switching_protocols(&upgrade),
            directive: ConnectionDirective::UpgradeReverse {
                session_id,
                purpose,
            },
        }
    }

    fn handle_play(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let play = match parse_play_request(&request.body) {
            Ok(p) => p,
            Err(e) => {
                warn!(session = %sid, error = %e, "POST /play parse failed");
                return ControlResponse::bad_request("HTTP/1.1");
            }
        };

        let classified = classify_play_request(&play, &sid, self.control_port, |port, session, remote| {
            local_playlist_url(port, session, remote).map_err(|e| e.to_string())
        });

        info!(
            session = %sid,
            client = classified.client_proc_name.as_deref().unwrap_or("-"),
            location_scheme = %url_scheme(&classified.original_location),
            location = %redact_media_url(&classified.original_location),
            ?classified.kind,
            ?classified.class,
            ?classified.support,
            start_position = ?classified.start_position,
            "POST /play"
        );

        match classified.support {
            MediaSupport::InvalidRequest => {
                warn!(
                    session = %sid,
                    reason = classified.reason.unwrap_or("invalid"),
                    "POST /play rejected"
                );
                return ControlResponse::bad_request("HTTP/1.1");
            }
            MediaSupport::ProtectedContent => {
                let msg = classified.reason.unwrap_or(
                    "This application is using protected content that this receiver cannot play.",
                );
                warn!(session = %sid, %msg, "POST /play protected content");
                // Keep server alive; honest failure (no DRM bypass).
                self.consumer.on_media_error(msg);
                return ControlResponse::not_implemented_http();
            }
            MediaSupport::UnsupportedCodec => {
                let msg = classified
                    .reason
                    .unwrap_or("unsupported media URL or codec");
                warn!(session = %sid, %msg, "POST /play unsupported");
                self.consumer.on_media_error(msg);
                return ControlResponse::not_implemented_http();
            }
            MediaSupport::Supported => {}
        }

        // Stop mirror streams for this session so media takes the window.
        // Mirroring continues only when the app never sends /play.
        let (video_gen, audio_gen) = self.sessions.with_session(&sid, |s| s.stop_media());
        if let Some(g) = audio_gen {
            self.consumer.on_audio_src_disconnect(g);
        }
        if let Some(g) = video_gen {
            self.consumer.on_video_src_disconnect(g);
        }

        // Optional initial volume from play plist (software path).
        if let Some(db) = classified.volume_db {
            self.consumer.on_volume(db);
        }

        // Start media player with local proxy / direct URL (non-blocking).
        info!(
            session = %sid,
            kind = ?classified.kind,
            class = ?classified.class,
            playback = %redact_media_url(&classified.playback_uri),
            "starting media playback"
        );
        self.consumer.on_media_playlist(&classified.playback_uri);

        if let Some(start) = classified.start_position {
            // Fractional start (0.0..=1.0) — apply after duration is known when possible.
            debug!(session = %sid, start, "play start-position (fraction)");
            if start > 0.0 && start <= 1.0 {
                // Best-effort: consumer may no-op until duration is available.
                self.consumer.on_media_playlist_seek_fraction(start);
            } else if start > 1.0 {
                // Some senders use absolute seconds.
                self.consumer.on_media_playlist_seek(start);
            }
        }

        if let Some(rate) = classified.rate {
            if rate == 0.0 {
                self.consumer.on_media_playlist_pause();
            } else if rate > 0.0 {
                self.consumer.on_media_playlist_resume();
            }
        }

        ControlResponse::ok_http()
    }

    fn handle_rate(&self, request: &ControlRequest) -> ControlResponse {
        let query = request.path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let Some(rate) = parse_rate_value(query) else {
            return ControlResponse::bad_request("HTTP/1.1");
        };
        if rate == 0.0 {
            self.consumer.on_media_playlist_pause();
        } else {
            self.consumer.on_media_playlist_resume();
        }
        info!(rate, "POST /rate");
        ControlResponse::ok_http()
    }

    fn handle_scrub(&self, request: &ControlRequest) -> ControlResponse {
        let query = request.path.split_once('?').map(|(_, q)| q).unwrap_or("");
        if let Some(position) = parse_scrub_position(query) {
            info!(position, "POST /scrub");
            self.consumer.on_media_playlist_seek(position);
        } else {
            debug!(path = %request.path, "POST /scrub without parseable position");
        }
        ControlResponse::ok_http()
    }

    fn handle_stop(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        info!(session = %sid, "POST /stop");
        self.sessions.cancel_media(&sid);
        self.consumer.on_media_playlist_remove();
        ControlResponse::ok_http()
    }

    fn handle_action(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let action = match parse_action(&request.body) {
            Ok(a) => a,
            Err(e) => {
                warn!(session = %sid, error = %e, "POST /action parse failed");
                return ControlResponse::bad_request("HTTP/1.1");
            }
        };

        match action {
            MediaAction::UnhandledUrlResponse { url, data } => {
                let fulfilled = self.sessions.fulfill_playlist(&sid, &url, data);
                if fulfilled {
                    debug!(
                        session = %sid,
                        url_path = %redact_url_path(&url),
                        "FCUP playlist response fulfilled"
                    );
                } else {
                    debug!(
                        session = %sid,
                        url_path = %redact_url_path(&url),
                        "FCUP playlist response had no waiter (stale/late)"
                    );
                }
            }
            MediaAction::PlaylistRemove { uuid } => {
                info!(session = %sid, ?uuid, "playlistRemove");
                self.sessions.cancel_media(&sid);
                self.consumer.on_media_playlist_remove();
            }
            MediaAction::Unsupported(kind) => {
                debug!(session = %sid, kind = %kind, "unsupported /action type");
            }
        }

        ControlResponse::ok_http()
    }

    fn handle_playback_info(&self, _request: &ControlRequest) -> ControlResponse {
        let info = self.consumer.playback_info();
        match plist_util::prepare_playback_info_response(&info) {
            Ok(body) => ControlResponse::ok_http()
                .header("Content-Type", "text/x-apple-plist+xml")
                .with_body(body),
            Err(e) => {
                error!("prepare_playback_info_response failed: {e}");
                ControlResponse::bad_request("HTTP/1.1")
            }
        }
    }

    async fn handle_get_playlist(&self, request: &ControlRequest) -> ControlResponse {
        let (session_id, remote_url) = match remote_playlist_url(&request.path) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, path = %request.path_only(), "invalid playlist proxy path");
                return ControlResponse::bad_request("HTTP/1.1");
            }
        };

        let Some(pending_rx) = self
            .sessions
            .register_playlist(&session_id, &remote_url)
        else {
            warn!(
                session = %session_id,
                "duplicate pending playlist request"
            );
            return ControlResponse::bad_gateway();
        };

        let event_body = match prepare_event_request(&session_id, &remote_url) {
            Ok(b) => b,
            Err(e) => {
                self.sessions
                    .remove_pending_playlist(&session_id, &remote_url);
                error!(error = %e, "prepare_event_request failed");
                return ControlResponse::bad_gateway();
            }
        };

        let Some(tx) = self.sessions.reverse_sender(&session_id, "event") else {
            self.sessions
                .remove_pending_playlist(&session_id, &remote_url);
            warn!(
                session = %session_id,
                "no reverse event channel for playlist fetch"
            );
            return ControlResponse::bad_gateway();
        };

        let outbound = OutboundRequest::post_event(&session_id, event_body);
        if tx.try_send(outbound).is_err() {
            self.sessions
                .remove_pending_playlist(&session_id, &remote_url);
            warn!(session = %session_id, "reverse event channel full or closed");
            return ControlResponse::bad_gateway();
        }

        debug!(
            session = %session_id,
            remote_path = %redact_url_path(&remote_url),
            "awaiting FCUP playlist response"
        );

        match tokio::time::timeout(Duration::from_secs(8), pending_rx).await {
            Ok(Ok(body)) => {
                // Generic proxy: detect DRM in fetched playlists (any app, not only YouTube).
                if playlist_looks_protected(&body) {
                    warn!(
                        session = %session_id,
                        remote_path = %redact_url_path(&remote_url),
                        "proxied playlist appears protected (FairPlay/SAMPLE-AES); refusing"
                    );
                    self.consumer.on_media_error(
                        "This application is using protected content that this receiver cannot play.",
                    );
                    self.consumer.on_media_playlist_remove();
                    return ControlResponse::not_implemented_http();
                }

                let content_type = sniff_playlist_content_type(&body);
                match rewrite_playlist(&body, self.control_port, &session_id) {
                    Ok(rewritten) => {
                        debug!(
                            session = %session_id,
                            content_type,
                            bytes = rewritten.len(),
                            "playlist proxy response"
                        );
                        ControlResponse::ok_http()
                            .header("Content-Type", content_type)
                            .with_body(rewritten)
                    }
                    Err(error) => {
                        // Non-playlist binary (segment/key) may fail rewrite — pass through.
                        if body.starts_with(b"#EXTM3U") || body.starts_with(b"#extm3u") {
                            warn!(
                                session = %session_id,
                                %error,
                                "playlist rewrite rejected"
                            );
                            ControlResponse::bad_gateway()
                        } else {
                            debug!(
                                session = %session_id,
                                bytes = body.len(),
                                "proxy passthrough non-playlist resource"
                            );
                            ControlResponse::ok_http()
                                .header("Content-Type", content_type)
                                .with_body(body)
                        }
                    }
                }
            }
            Ok(Err(_)) => {
                warn!(session = %session_id, "playlist waiter cancelled");
                ControlResponse::bad_gateway()
            }
            Err(_) => {
                self.sessions
                    .remove_pending_playlist(&session_id, &remote_url);
                warn!(session = %session_id, "playlist FCUP timeout");
                ControlResponse::gateway_timeout()
            }
        }
    }
}

fn sniff_playlist_content_type(body: &[u8]) -> &'static str {
    if body.starts_with(b"#EXTM3U") || body.starts_with(b"#extm3u") {
        "application/vnd.apple.mpegurl"
    } else if body.len() >= 4 && &body[..4] == b"\0\0\0" {
        // ftyp box often at offset 4 — still return generic binary.
        "video/mp4"
    } else if body.starts_with(b"\xff\xf1") || body.starts_with(b"\xff\xf9") {
        "audio/aac"
    } else {
        "application/octet-stream"
    }
}

fn ok_rtsp(request: &ControlRequest) -> ControlResponse {
    ControlResponse::ok_rtsp().with_cseq_from(request)
}

fn parse_volume_parameter(body: &[u8]) -> Option<f64> {
    // Accept CRLF / LF / lone CR and extra parameters; never panic.
    let text = std::str::from_utf8(body).ok()?;
    for line in text.split(|c| c == '\n' || c == '\r') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name, value) = match line.split_once(':') {
            Some(pair) => pair,
            None => match line.split_once('=') {
                Some(pair) => pair,
                None => continue,
            },
        };
        if !name.trim().eq_ignore_ascii_case("volume") {
            continue;
        }
        let volume_db = value.trim().parse::<f64>().ok()?;
        if volume_db.is_finite() {
            // Clamp to AirPlay range; 0.0 remains maximum (not mute).
            return Some(volume_db.clamp(-144.0, 0.0));
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
enum SetPropertyVolume {
    None,
    VolumeDb(f64),
    Muted(bool),
    Both { db: f64, muted: bool },
}

/// Parse HTTP `/setProperty` binary or XML plist for volume / mute keys.
fn parse_set_property_volume(body: &[u8], path_prop: Option<&str>) -> SetPropertyVolume {
    if body.is_empty() && path_prop.is_none() {
        return SetPropertyVolume::None;
    }

    // Empty body with property name in query is uncommon; acknowledge only.
    let value = if body.is_empty() {
        None
    } else {
        plist::Value::from_reader(std::io::Cursor::new(body)).ok()
    };

    let mut volume_db: Option<f64> = None;
    let mut muted: Option<bool> = None;

    if let Some(v) = value.as_ref() {
        extract_volume_mute_from_plist(v, &mut volume_db, &mut muted);
    }

    // If the query names a single property and the body is a bare number/bool plist.
    if let (Some(prop), Some(v)) = (path_prop, value.as_ref()) {
        if prop.eq_ignore_ascii_case("volume") || prop.eq_ignore_ascii_case("outputVolume") {
            if volume_db.is_none() {
                volume_db = plist_as_f64(v);
            }
        } else if prop.eq_ignore_ascii_case("muted") || prop.eq_ignore_ascii_case("mute") {
            if muted.is_none() {
                muted = plist_as_bool(v);
            }
        }
    }

    match (volume_db, muted) {
        (Some(db), Some(m)) => SetPropertyVolume::Both { db, muted: m },
        (Some(db), None) => SetPropertyVolume::VolumeDb(db),
        (None, Some(m)) => SetPropertyVolume::Muted(m),
        (None, None) => SetPropertyVolume::None,
    }
}

fn extract_volume_mute_from_plist(
    value: &plist::Value,
    volume_db: &mut Option<f64>,
    muted: &mut Option<bool>,
) {
    match value {
        plist::Value::Dictionary(dict) => {
            for (key, val) in dict {
                let k = key.as_str();
                if k.eq_ignore_ascii_case("volume")
                    || k.eq_ignore_ascii_case("outputVolume")
                    || k.eq_ignore_ascii_case("volumeValue")
                {
                    if let Some(db) = plist_as_f64(val) {
                        *volume_db = Some(db.clamp(-144.0, 0.0));
                    }
                } else if k.eq_ignore_ascii_case("muted") || k.eq_ignore_ascii_case("mute") {
                    if let Some(m) = plist_as_bool(val) {
                        *muted = Some(m);
                    }
                } else if k.eq_ignore_ascii_case("value")
                    || k.eq_ignore_ascii_case("params")
                    || k.eq_ignore_ascii_case("property")
                {
                    // Nested shapes observed in some AirPlay property envelopes.
                    extract_volume_mute_from_plist(val, volume_db, muted);
                }
            }
        }
        plist::Value::Array(items) => {
            for item in items {
                extract_volume_mute_from_plist(item, volume_db, muted);
            }
        }
        _ => {}
    }
}

fn plist_as_f64(value: &plist::Value) -> Option<f64> {
    match value {
        plist::Value::Real(r) => r.is_finite().then_some(*r),
        plist::Value::Integer(i) => i.as_signed().map(|v| v as f64),
        plist::Value::String(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

fn plist_as_bool(value: &plist::Value) -> Option<bool> {
    match value {
        plist::Value::Boolean(b) => Some(*b),
        plist::Value::Integer(i) => i.as_signed().map(|v| v != 0),
        plist::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod volume_parse_tests {
    use super::*;
    use plist::{Dictionary, Value};

    #[test]
    fn zero_db_is_maximum_not_mute() {
        let db = parse_volume_parameter(b"volume: 0.000000\r\n").unwrap();
        assert_eq!(db, 0.0);
    }

    #[test]
    fn mute_sentinel_and_low_volume() {
        assert_eq!(
            parse_volume_parameter(b"volume: -144.000000\r\n"),
            Some(-144.0)
        );
        assert_eq!(parse_volume_parameter(b"volume: -30.0\n"), Some(-30.0));
    }

    #[test]
    fn extra_parameters_and_whitespace() {
        let body = b"progress: 1/2/3\r\n  volume : -18.5  \r\nother: x\r\n";
        assert_eq!(parse_volume_parameter(body), Some(-18.5));
    }

    #[test]
    fn malformed_volume_ignored() {
        assert!(parse_volume_parameter(b"volume: NaN\r\n").is_none());
        assert!(parse_volume_parameter(b"volume: abc\r\n").is_none());
        assert!(parse_volume_parameter(b"not-volume: 1\r\n").is_none());
    }

    #[test]
    fn set_property_plist_volume() {
        let mut dict = Dictionary::new();
        dict.insert("volume".into(), Value::Real(-20.0));
        let mut bytes = Vec::new();
        Value::Dictionary(dict)
            .to_writer_binary(&mut bytes)
            .unwrap();
        assert_eq!(
            parse_set_property_volume(&bytes, None),
            SetPropertyVolume::VolumeDb(-20.0)
        );
    }

    #[test]
    fn set_property_plist_mute() {
        let mut dict = Dictionary::new();
        dict.insert("muted".into(), Value::Boolean(true));
        let mut bytes = Vec::new();
        Value::Dictionary(dict)
            .to_writer_binary(&mut bytes)
            .unwrap();
        assert_eq!(
            parse_set_property_volume(&bytes, None),
            SetPropertyVolume::Muted(true)
        );
    }

    #[test]
    fn set_property_output_volume_key() {
        let mut dict = Dictionary::new();
        dict.insert("outputVolume".into(), Value::Real(0.0));
        let mut bytes = Vec::new();
        Value::Dictionary(dict)
            .to_writer_binary(&mut bytes)
            .unwrap();
        assert_eq!(
            parse_set_property_volume(&bytes, None),
            SetPropertyVolume::VolumeDb(0.0)
        );
    }
}

/// Safe structured diagnostics for every control request (no secrets / bodies).
fn log_control_request(request: &ControlRequest) {
    let protocol = if request.is_rtsp() {
        "RTSP"
    } else if request.is_http() {
        "HTTP"
    } else {
        "other"
    };
    let content_type = request.header("Content-Type").unwrap_or("-");
    let content_length = request
        .header("Content-Length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(request.body.len());
    let purpose = request.header("X-Apple-Purpose").unwrap_or("-");
    let session = request.session_id();
    let active_remote = request.header("Active-Remote").unwrap_or("-");
    let apple_session = request.header("X-Apple-Session-ID").unwrap_or("-");

    info!(
        protocol,
        method = %request.method,
        path = %request.path_only(),
        version = %request.version,
        session = %session,
        active_remote = %active_remote,
        apple_session = %apple_session,
        purpose = %purpose,
        content_type = %content_type,
        content_length,
        "control request"
    );

    // Decode plist keys only (never dump secret values).
    if content_type.contains("plist") || looks_like_plist(&request.body) {
        if let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(&request.body)) {
            if let Some(dict) = value.as_dictionary() {
                let keys: Vec<&str> = dict.keys().map(|k| k.as_str()).collect();
                debug!(?keys, "control plist keys");
            }
        }
    }
}

fn looks_like_plist(body: &[u8]) -> bool {
    body.starts_with(b"bplist") || body.starts_with(b"<?xml") || body.starts_with(b"<plist")
}

fn url_scheme(url: &str) -> &str {
    url.split("://").next().unwrap_or(url)
}

fn redact_url_path(url: &str) -> String {
    // Keep scheme + path only; drop query (may hold tokens).
    let no_query = url.split('?').next().unwrap_or(url);
    no_query.to_string()
}
