//! GStreamer live player for AirPlay screen-mirror video (and optional audio).
//!
//! Requires system GStreamer 1.x and Cargo feature `gstreamer`.
//! Pipeline (H.264 annex-B / byte-stream):
//! `appsrc ! queue ! h264parse ! avdec_h264 ! videoconvert ! autovideosink`
//!
//! A GLib main loop runs on a background thread so Windows/Linux video sinks
//! can create and update the playback window.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use airplay_lib::{AudioStreamInfo, CompressionType, VideoStreamInfo};
use airplay_server::{AirPlayConsumer, PlaybackInfo};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

/// Live GStreamer-backed consumer: video window + ALAC / AAC-ELD audio.
pub struct GStreamerPlayer {
    h264_pipeline: Mutex<gst::Pipeline>,
    h264_src: Mutex<gst_app::AppSrc>,
    alac_pipeline: Mutex<gst::Pipeline>,
    alac_src: Mutex<gst_app::AppSrc>,
    aac_eld_pipeline: Mutex<gst::Pipeline>,
    aac_eld_src: Mutex<gst_app::AppSrc>,
    audio_compression: Mutex<Option<CompressionType>>,
    hls_pipeline: Mutex<Option<gst::Pipeline>>,
    /// Keep main-loop thread alive for the lifetime of the player.
    _main_loop_thread: Option<JoinHandle<()>>,
    main_loop_quit: Arc<AtomicBool>,
}

impl GStreamerPlayer {
    /// Initialize GStreamer and build the live mirror pipelines.
    pub fn new() -> Result<Self, String> {
        gst::init().map_err(|e| format!("gstreamer init failed: {e}"))?;

        // GLib main loop is required for autovideosink window creation on Windows.
        let main_loop_quit = Arc::new(AtomicBool::new(false));
        let quit_flag = Arc::clone(&main_loop_quit);
        let main_loop_thread = thread::Builder::new()
            .name("gstreamer-main-loop".into())
            .spawn(move || {
                let main_context = glib::MainContext::default();
                let _guard = main_context.acquire().expect("acquire GLib main context");
                while !quit_flag.load(Ordering::Relaxed) {
                    // Pump pending events; short timeout so we can exit promptly.
                    let _ = main_context.iteration(false);
                    thread::sleep(std::time::Duration::from_millis(10));
                }
            })
            .map_err(|e| format!("spawn gstreamer main loop: {e}"))?;

        // queue isolates appsrc push thread from the decode/display chain.
        let h264_pipeline = gst::parse::launch(
            "appsrc name=video_src is-live=true format=time do-timestamp=true \
             ! queue max-size-buffers=0 max-size-time=0 max-size-bytes=0 \
             ! h264parse \
             ! avdec_h264 \
             ! videoconvert \
             ! autovideosink name=videosink sync=false",
        )
        .map_err(|e| format!("parse H.264 pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "H.264 launch result is not a Pipeline".to_string())?;

        // Prefer a titled window when the sink supports it.
        if let Some(sink) = h264_pipeline.by_name("videosink") {
            if sink.find_property("title").is_some() {
                sink.set_property_from_str("title", "airplay2-rust");
            }
        }

        let h264_src = h264_pipeline
            .by_name("video_src")
            .ok_or_else(|| "missing appsrc video_src".to_string())?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "video_src is not an AppSrc".to_string())?;

        h264_src.set_caps(Some(
            &gst::Caps::from_str(
                "video/x-h264,stream-format=(string)byte-stream,alignment=(string)au",
            )
            .map_err(|e| format!("H.264 caps: {e}"))?,
        ));
        h264_src.set_format(gst::Format::Time);
        h264_src.set_is_live(true);
        h264_src.set_stream_type(gst_app::AppStreamType::Stream);
        // Avoid blocking the media thread if the sink is slow.
        h264_src.set_property("block", false);
        h264_src.set_max_bytes(8 * 1024 * 1024);

        let alac_pipeline = gst::parse::launch(
            "appsrc name=alac_src is-live=true format=time do-timestamp=true \
             ! queue ! avdec_alac ! audioconvert ! audioresample ! autoaudiosink sync=false",
        )
        .map_err(|e| format!("parse ALAC pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "ALAC launch result is not a Pipeline".to_string())?;

        let alac_src = alac_pipeline
            .by_name("alac_src")
            .ok_or_else(|| "missing appsrc alac_src".to_string())?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "alac_src is not an AppSrc".to_string())?;

        // codec_data matches the Java GstPlayer ALAC 44.1 kHz stereo setup.
        alac_src.set_caps(Some(
            &gst::Caps::from_str(
                "audio/x-alac,mpegversion=(int)4,channels=(int)2,rate=(int)44100,\
                 stream-format=raw,codec_data=(buffer)00000024616c616300000000000001600010280a0e0200ff00000000000000000000ac44",
            )
            .map_err(|e| format!("ALAC caps: {e}"))?,
        ));
        alac_src.set_format(gst::Format::Time);
        alac_src.set_is_live(true);
        alac_src.set_stream_type(gst_app::AppStreamType::Stream);

        let aac_eld_pipeline = gst::parse::launch(
            "appsrc name=aac_eld_src is-live=true format=time do-timestamp=true \
             ! queue ! avdec_aac ! audioconvert ! audioresample ! autoaudiosink sync=false",
        )
        .map_err(|e| format!("parse AAC-ELD pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "AAC-ELD launch result is not a Pipeline".to_string())?;

        let aac_eld_src = aac_eld_pipeline
            .by_name("aac_eld_src")
            .ok_or_else(|| "missing appsrc aac_eld_src".to_string())?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "aac_eld_src is not an AppSrc".to_string())?;

        aac_eld_src.set_caps(Some(
            &gst::Caps::from_str(
                "audio/mpeg,mpegversion=(int)4,channels=(int)2,rate=(int)44100,\
                 stream-format=raw,codec_data=(buffer)f8e85000",
            )
            .map_err(|e| format!("AAC-ELD caps: {e}"))?,
        ));
        aac_eld_src.set_format(gst::Format::Time);
        aac_eld_src.set_is_live(true);
        aac_eld_src.set_stream_type(gst_app::AppStreamType::Stream);

        // Enable bus signal watches; the GLib pump thread will deliver sink UI events.
        for pipeline in [&h264_pipeline, &alac_pipeline, &aac_eld_pipeline] {
            if let Some(bus) = pipeline.bus() {
                bus.set_sync_handler(|_bus, msg| {
                    use gst::MessageView;
                    match msg.view() {
                        MessageView::Error(err) => {
                            tracing::error!(
                                error = %err.error(),
                                debug = ?err.debug(),
                                "GStreamer error"
                            );
                        }
                        MessageView::Warning(w) => {
                            tracing::warn!(
                                error = %w.error(),
                                debug = ?w.debug(),
                                "GStreamer warning"
                            );
                        }
                        _ => {}
                    }
                    gst::BusSyncReply::Pass
                });
            }
        }

        tracing::info!("GStreamer player ready (H.264 window + ALAC/AAC-ELD audio)");

        Ok(Self {
            h264_pipeline: Mutex::new(h264_pipeline),
            h264_src: Mutex::new(h264_src),
            alac_pipeline: Mutex::new(alac_pipeline),
            alac_src: Mutex::new(alac_src),
            aac_eld_pipeline: Mutex::new(aac_eld_pipeline),
            aac_eld_src: Mutex::new(aac_eld_src),
            audio_compression: Mutex::new(None),
            hls_pipeline: Mutex::new(None),
            _main_loop_thread: Some(main_loop_thread),
            main_loop_quit,
        })
    }

    fn push_to_appsrc(src: &gst_app::AppSrc, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let Ok(mut buffer) = gst::Buffer::with_size(data.len()) else {
            tracing::warn!("gstreamer: failed to allocate buffer of size {}", data.len());
            return;
        };
        {
            let buffer_ref = buffer.make_mut();
            match buffer_ref.map_writable() {
                Ok(mut map) => map.copy_from_slice(data),
                Err(e) => {
                    tracing::warn!(error = %e, "gstreamer: map buffer writable failed");
                    return;
                }
            }
        }
        if let Err(e) = src.push_buffer(buffer) {
            tracing::warn!(error = %e, "gstreamer: push_buffer failed");
        }
    }
}

impl AirPlayConsumer for GStreamerPlayer {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            "video format; starting GStreamer H.264 pipeline"
        );
        match self.h264_pipeline.lock() {
            Ok(p) => {
                if let Err(e) = p.set_state(gst::State::Playing) {
                    tracing::error!(error = %e, "failed to play H.264 pipeline");
                }
            }
            Err(e) => tracing::error!(error = %e, "H.264 pipeline mutex poisoned"),
        }
    }

    fn on_video(&self, data: &[u8]) {
        match self.h264_src.lock() {
            Ok(src) => Self::push_to_appsrc(&src, data),
            Err(e) => tracing::error!(error = %e, "H.264 appsrc mutex poisoned"),
        }
    }

    fn on_video_src_disconnect(&self) {
        tracing::info!("video source disconnected; stopping H.264 pipeline");
        match self.h264_pipeline.lock() {
            Ok(p) => {
                let _ = p.set_state(gst::State::Null);
            }
            Err(e) => tracing::error!(error = %e, "H.264 pipeline mutex poisoned on disconnect"),
        }
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        tracing::info!(?info, "audio format; starting GStreamer audio pipelines");
        if let Ok(mut ct) = self.audio_compression.lock() {
            *ct = info.compression_type;
        }
        for (label, pipe) in [
            ("ALAC", &self.alac_pipeline),
            ("AAC-ELD", &self.aac_eld_pipeline),
        ] {
            match pipe.lock() {
                Ok(p) => {
                    if let Err(e) = p.set_state(gst::State::Playing) {
                        tracing::warn!(pipeline = label, error = %e, "failed to play audio pipeline");
                    }
                }
                Err(e) => tracing::error!(pipeline = label, error = %e, "audio pipeline mutex poisoned"),
            }
        }
    }

    fn on_audio(&self, data: &[u8]) {
        let compression = self
            .audio_compression
            .lock()
            .ok()
            .and_then(|g| *g);
        match compression {
            Some(CompressionType::Alac) => match self.alac_src.lock() {
                Ok(src) => Self::push_to_appsrc(&src, data),
                Err(e) => tracing::error!(error = %e, "ALAC appsrc mutex poisoned"),
            },
            Some(CompressionType::AacEld) | Some(CompressionType::Aac) => {
                match self.aac_eld_src.lock() {
                    Ok(src) => Self::push_to_appsrc(&src, data),
                    Err(e) => tracing::error!(error = %e, "AAC-ELD appsrc mutex poisoned"),
                }
            }
            other => {
                tracing::trace!(?other, "ignoring audio (unsupported or unknown compression)");
            }
        }
    }

    fn on_audio_src_disconnect(&self) {
        tracing::debug!("audio source disconnected; stopping audio pipelines");
        for pipe in [&self.alac_pipeline, &self.aac_eld_pipeline] {
            if let Ok(p) = pipe.lock() {
                let _ = p.set_state(gst::State::Null);
            }
        }
        if let Ok(mut ct) = self.audio_compression.lock() {
            *ct = None;
        }
    }

    fn on_media_playlist(&self, playlist_uri: &str) {
        tracing::info!(%playlist_uri, "starting GStreamer HLS playbin");
        match gst::parse::launch(&format!("playbin3 uri={playlist_uri}")) {
            Ok(elem) => match elem.downcast::<gst::Pipeline>() {
                Ok(pipeline) => {
                    if let Err(e) = pipeline.set_state(gst::State::Playing) {
                        tracing::error!(error = %e, "HLS pipeline play failed");
                    }
                    if let Ok(mut slot) = self.hls_pipeline.lock() {
                        if let Some(old) = slot.take() {
                            let _ = old.set_state(gst::State::Null);
                        }
                        *slot = Some(pipeline);
                    }
                }
                Err(_) => tracing::error!("playbin3 launch is not a Pipeline"),
            },
            Err(e) => tracing::error!(error = %e, "failed to parse HLS pipeline"),
        }
    }

    fn on_media_playlist_remove(&self) {
        if let Ok(mut slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.take() {
                let _ = p.set_state(gst::State::Null);
            }
        }
    }

    fn on_media_playlist_pause(&self) {
        if let Ok(slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.as_ref() {
                let _ = p.set_state(gst::State::Paused);
            }
        }
    }

    fn on_media_playlist_resume(&self) {
        if let Ok(slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.as_ref() {
                let _ = p.set_state(gst::State::Playing);
            }
        }
    }

    fn playback_info(&self) -> PlaybackInfo {
        if let Ok(slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.as_ref() {
                let duration = p
                    .query_duration::<gst::ClockTime>()
                    .map(|t| t.seconds() as f64)
                    .unwrap_or(0.0);
                let position = p
                    .query_position::<gst::ClockTime>()
                    .map(|t| t.seconds() as f64)
                    .unwrap_or(0.0);
                return PlaybackInfo {
                    duration,
                    position,
                };
            }
        }
        PlaybackInfo {
            duration: 0.0,
            position: 0.0,
        }
    }
}

impl Drop for GStreamerPlayer {
    fn drop(&mut self) {
        for pipe in [
            &self.h264_pipeline,
            &self.alac_pipeline,
            &self.aac_eld_pipeline,
        ] {
            if let Ok(p) = pipe.lock() {
                let _ = p.set_state(gst::State::Null);
            }
        }
        if let Ok(mut slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.take() {
                let _ = p.set_state(gst::State::Null);
            }
        }
        self.main_loop_quit.store(true, Ordering::Relaxed);
        if let Some(handle) = self._main_loop_thread.take() {
            let _ = handle.join();
        }
    }
}
