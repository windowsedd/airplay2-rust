//! Control request dispatch (RTSP + HTTP paths used by AirPlay receivers).

use std::sync::Arc;

use airplay_lib::MediaStreamInfo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

use crate::config::AirPlayConfig;
use crate::consumer::AirPlayConsumer;
use crate::control::codec::{ControlRequest, ControlResponse};
use crate::plist_util;
use crate::session::SessionManager;

/// Handles a single parsed control request.
pub struct ControlHandler {
    pub sessions: Arc<SessionManager>,
    pub config: AirPlayConfig,
    pub consumer: Arc<dyn AirPlayConsumer>,
    /// Control server listen port (used as eventPort in video SETUP).
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

    /// Dispatch request → response. Never panics on protocol errors.
    pub async fn handle(&self, request: &ControlRequest) -> ControlResponse {
        let method = request.method.to_ascii_uppercase();
        let path = request.path_only();

        debug!(
            method = %request.method,
            path = %request.path,
            version = %request.version,
            "control request"
        );

        if request.is_rtsp() {
            return self.handle_rtsp(&method, path, request).await;
        }
        if request.is_http() {
            return self.handle_http(&method, path, request).await;
        }

        warn!(version = %request.version, "unknown protocol version");
        ControlResponse::not_found(&request.version).with_cseq_from(request)
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
    ) -> ControlResponse {
        match (method, path) {
            ("GET", "/server-info") => self.handle_server_info(request),
            // Stubs for reverse/play and related HTTP media paths.
            ("POST", "/reverse") => ControlResponse::ok_http()
                .header("Upgrade", request.header("Upgrade").unwrap_or("PTTH/1.0"))
                .header("Connection", "Upgrade")
                .with_body(Vec::new()),
            ("POST", "/play")
            | ("POST", "/rate")
            | ("POST", "/action")
            | ("POST", "/getProperty")
            | ("POST", "/stop")
            | ("POST", "/scrub")
            | ("PUT", "/setProperty")
            | ("GET", "/playback-info") => ControlResponse::ok_http(),
            _ => {
                error!(
                    method = %request.method,
                    path = %request.path,
                    "unknown HTTP control request"
                );
                ControlResponse::not_found(&request.version)
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
        let pk = self
            .sessions
            .with_session(&sid, |s| s.airplay.pair_setup());
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .with_body(pk.to_vec())
    }

    fn handle_pair_verify(&self, request: &ControlRequest) -> ControlResponse {
        let sid = request.session_id().to_string();
        let result = self
            .sessions
            .with_session(&sid, |s| s.airplay.pair_verify(&request.body));
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
        let result = self
            .sessions
            .with_session(&sid, |s| s.airplay.fair_play_setup(&request.body));
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
        let setup_result = self
            .sessions
            .with_session(&sid, |s| s.airplay.rtsp_setup(&request.body));

        let media = match setup_result {
            Ok(m) => m,
            Err(e) => {
                warn!(session = %sid, "rtsp_setup failed: {e}");
                return ControlResponse::bad_request(&request.version).with_cseq_from(request);
            }
        };

        match media {
            None => {
                // ekey/eiv only — empty body OK.
                ok_rtsp(request)
            }
            Some(MediaStreamInfo::Video(info)) => {
                self.consumer.on_video_format(&info);
                match bind_placeholder_tcp().await {
                    Ok((listener, port)) => {
                        let handle = spawn_tcp_discard(listener);
                        self.sessions.with_session(&sid, |s| {
                            if let Some(old) = s.video_task.take() {
                                old.abort();
                            }
                            s.video_port = Some(port);
                            s.video_task = Some(handle);
                        });
                        info!(session = %sid, port, "video SETUP bound placeholder port");
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
                self.consumer.on_audio_format(&info);
                let data = bind_placeholder_tcp().await;
                let control = bind_placeholder_tcp().await;
                match (data, control) {
                    (Ok((data_l, data_port)), Ok((ctrl_l, ctrl_port))) => {
                        let data_handle = spawn_tcp_discard(data_l);
                        let ctrl_handle = spawn_tcp_discard(ctrl_l);
                        self.sessions.with_session(&sid, |s| {
                            if let Some(old) = s.audio_task.take() {
                                old.abort();
                            }
                            if let Some(old) = s.audio_control_task.take() {
                                old.abort();
                            }
                            s.audio_port = Some(data_port);
                            s.audio_control_port = Some(ctrl_port);
                            s.audio_task = Some(data_handle);
                            s.audio_control_task = Some(ctrl_handle);
                        });
                        info!(
                            session = %sid,
                            data_port,
                            ctrl_port,
                            "audio SETUP bound placeholder ports"
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
        let media = self
            .sessions
            .with_session(&sid, |s| s.airplay.rtsp_teardown(&request.body));

        match media {
            Ok(Some(MediaStreamInfo::Audio(_))) => {
                self.consumer.on_audio_src_disconnect();
                self.sessions.with_session(&sid, |s| {
                    if let Some(h) = s.audio_task.take() {
                        h.abort();
                    }
                    if let Some(h) = s.audio_control_task.take() {
                        h.abort();
                    }
                    s.audio_port = None;
                    s.audio_control_port = None;
                });
            }
            Ok(Some(MediaStreamInfo::Video(_))) => {
                self.consumer.on_video_src_disconnect();
                self.sessions.with_session(&sid, |s| {
                    if let Some(h) = s.video_task.take() {
                        h.abort();
                    }
                    s.video_port = None;
                });
            }
            Ok(None) => {
                self.consumer.on_audio_src_disconnect();
                self.consumer.on_video_src_disconnect();
                self.sessions.with_session(&sid, |s| s.stop_media());
            }
            Err(e) => {
                // Empty body TEARDOWN is common; still disconnect everything.
                debug!(session = %sid, "rtsp_teardown parse note: {e}");
                self.consumer.on_audio_src_disconnect();
                self.consumer.on_video_src_disconnect();
                self.sessions.with_session(&sid, |s| s.stop_media());
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
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .with_body(b"volume: 0.000000\r\n".to_vec())
    }

    fn handle_set_parameter(&self, request: &ControlRequest) -> ControlResponse {
        ControlResponse::ok_rtsp()
            .with_cseq_from(request)
            .header("Audio-Jack-Status", "connected; type=analog")
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
}

fn ok_rtsp(request: &ControlRequest) -> ControlResponse {
    ControlResponse::ok_rtsp().with_cseq_from(request)
}

async fn bind_placeholder_tcp() -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Accept connections and discard all data until the listener is dropped/aborted.
fn spawn_tcp_discard(listener: TcpListener) -> tokio::task::AbortHandle {
    let join = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((mut stream, peer)) => {
                    debug!(%peer, "placeholder media accept (discard)");
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        loop {
                            match stream.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(_) => {
                                    // discard until Task 10
                                }
                            }
                        }
                        let _ = stream.shutdown().await;
                    });
                }
                Err(_) => break,
            }
        }
    });
    join.abort_handle()
}
