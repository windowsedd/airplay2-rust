//! GStreamer live player for AirPlay screen-mirror video (and optional audio).
//!
//! - Video window title: **airplay2-rust** (renames Windows D3D12 default title)
//! - PC-side volume (not phone): GStreamer `volume` element + console keys:
//!   `+` / `=` louder, `-` quieter, `m` mute/unmute, `0`–`9` set level
//!
//! Pipeline (H.264 annex-B):
//! `appsrc ! h264parse ! avdec_h264 ! videoconvert ! autovideosink`

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use airplay_lib::{AudioStreamInfo, CompressionType, VideoStreamInfo};
use airplay_server::{AirPlayConsumer, PlaybackInfo};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

const WINDOW_TITLE: &str = "airplay2-rust";

/// Live GStreamer-backed consumer: titled video window + PC volume control.
pub struct GStreamerPlayer {
    h264_pipeline: Mutex<gst::Pipeline>,
    h264_src: Mutex<gst_app::AppSrc>,
    alac_pipeline: Mutex<gst::Pipeline>,
    alac_src: Mutex<gst_app::AppSrc>,
    aac_eld_pipeline: Mutex<gst::Pipeline>,
    aac_eld_src: Mutex<gst_app::AppSrc>,
    audio_compression: Mutex<Option<CompressionType>>,
    hls_pipeline: Mutex<Option<gst::Pipeline>>,
    /// Shared linear volume (0.0–2.0), stored as milli-units (1000 = 1.0).
    volume_milli: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    volume_before_mute: Mutex<f64>,
    _main_loop_thread: Option<JoinHandle<()>>,
    _volume_keys_thread: Option<JoinHandle<()>>,
    main_loop_quit: Arc<AtomicBool>,
}

impl GStreamerPlayer {
    /// Initialize GStreamer, pipelines, window title fixer, and volume key thread.
    pub fn new() -> Result<Self, String> {
        Self::with_volume(1.0)
    }

    /// Create with initial PC volume in `0.0..=2.0` (1.0 = unity).
    pub fn with_volume(initial: f64) -> Result<Self, String> {
        gst::init().map_err(|e| format!("gstreamer init failed: {e}"))?;

        let volume_milli = Arc::new(AtomicU32::new(volume_to_milli(initial.clamp(0.0, 2.0))));
        let muted = Arc::new(AtomicBool::new(false));
        let main_loop_quit = Arc::new(AtomicBool::new(false));

        // Pump GLib so video sinks can create/update the HWND.
        let quit_flag = Arc::clone(&main_loop_quit);
        let main_loop_thread = thread::Builder::new()
            .name("gstreamer-main-loop".into())
            .spawn(move || {
                let main_context = glib::MainContext::default();
                let _guard = main_context.acquire().expect("acquire GLib main context");
                while !quit_flag.load(Ordering::Relaxed) {
                    let _ = main_context.iteration(false);
                    // Keep retitling Windows D3D sinks (they often reset the title).
                    #[cfg(windows)]
                    rename_d3d_windows(WINDOW_TITLE);
                    thread::sleep(Duration::from_millis(50));
                }
            })
            .map_err(|e| format!("spawn gstreamer main loop: {e}"))?;

        let h264_pipeline = gst::parse::launch(
            "appsrc name=h264-src is-live=true format=time do-timestamp=true \
             ! h264parse config-interval=-1 \
             ! avdec_h264 \
             ! videoconvert \
             ! autovideosink name=videosink sync=false",
        )
        .map_err(|e| format!("parse H.264 pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "H.264 launch result is not a Pipeline".to_string())?;

        apply_window_title(&h264_pipeline, WINDOW_TITLE);

        let h264_src = h264_pipeline
            .by_name("h264-src")
            .ok_or_else(|| "missing appsrc h264-src".to_string())?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "h264-src is not an AppSrc".to_string())?;

        h264_src.set_caps(Some(
            &gst::Caps::from_str(
                "video/x-h264,colorimetry=bt709,stream-format=(string)byte-stream,alignment=(string)au",
            )
            .map_err(|e| format!("H.264 caps: {e}"))?,
        ));
        h264_src.set_format(gst::Format::Time);
        h264_src.set_is_live(true);
        h264_src.set_stream_type(gst_app::AppStreamType::Stream);
        h264_src.set_property("block", true);
        h264_src.set_max_bytes(4 * 1024 * 1024);

        // volume element = PC-side gain (does not change phone volume).
        let alac_pipeline = gst::parse::launch(
            "appsrc name=alac_src is-live=true format=time do-timestamp=true \
             ! queue ! avdec_alac ! audioconvert ! audioresample \
             ! volume name=vol_alac volume=1.0 \
             ! autoaudiosink sync=false",
        )
        .map_err(|e| format!("parse ALAC pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "ALAC launch result is not a Pipeline".to_string())?;

        let alac_src = alac_pipeline
            .by_name("alac_src")
            .ok_or_else(|| "missing appsrc alac_src".to_string())?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "alac_src is not an AppSrc".to_string())?;

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
             ! queue ! avdec_aac ! audioconvert ! audioresample \
             ! volume name=vol_aac volume=1.0 \
             ! autoaudiosink sync=false",
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

        // Apply initial volume to named elements.
        let vol = milli_to_volume(volume_milli.load(Ordering::Relaxed));
        set_pipeline_volume(&alac_pipeline, "vol_alac", vol);
        set_pipeline_volume(&aac_eld_pipeline, "vol_aac", vol);

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
                        MessageView::StateChanged(sc) => {
                            // When sink goes PLAYING, retitle window.
                            if sc.current() == gst::State::Playing {
                                #[cfg(windows)]
                                rename_d3d_windows(WINDOW_TITLE);
                            }
                        }
                        _ => {}
                    }
                    gst::BusSyncReply::Pass
                });
            }
        }

        // Console volume keys (PC-side only).
        let vol_keys_quit = Arc::clone(&main_loop_quit);
        let vol_m = Arc::clone(&volume_milli);
        let muted_flag = Arc::clone(&muted);
        let alac_for_keys = alac_pipeline.clone();
        let aac_for_keys = aac_eld_pipeline.clone();
        let volume_keys_thread = thread::Builder::new()
            .name("pc-volume-keys".into())
            .spawn(move || {
                use std::io::Read;
                tracing::info!(
                    "PC volume keys (not phone): +/= louder, - quieter, m mute, 0-9 set level"
                );
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                let mut buf = [0u8; 1];
                let mut last_before_mute = 1.0_f64;
                while !vol_keys_quit.load(Ordering::Relaxed) {
                    // Non-blocking-ish: try read with short timeout via peeks aren't portable;
                    // use blocking read — user can still Ctrl+C the process.
                    // To avoid blocking forever on exit, we use a separate approach on Windows:
                    // try_read via raw mode is heavy; keep simple blocking with quit check after each key.
                    match lock.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {
                            let key = buf[0] as char;
                            let muted_now = muted_flag.load(Ordering::Relaxed);
                            let cur = milli_to_volume(vol_m.load(Ordering::Relaxed));
                            let (new_vol, new_muted) = match key {
                                '+' | '=' => ((cur + 0.1).min(2.0), false),
                                '-' | '_' => ((cur - 0.1).max(0.0), false),
                                'm' | 'M' => {
                                    if muted_now {
                                        (last_before_mute, false)
                                    } else {
                                        last_before_mute = if cur > 0.001 { cur } else { 1.0 };
                                        (0.0, true)
                                    }
                                }
                                '0'..='9' => {
                                    let digit = (key as u8 - b'0') as f64;
                                    (digit / 9.0, false) // 0→0%, 9→100%
                                }
                                _ => continue,
                            };
                            muted_flag.store(new_muted, Ordering::Relaxed);
                            vol_m.store(volume_to_milli(new_vol), Ordering::Relaxed);
                            set_pipeline_volume(&alac_for_keys, "vol_alac", new_vol);
                            set_pipeline_volume(&aac_for_keys, "vol_aac", new_vol);
                            let pct = (new_vol * 100.0).round() as i32;
                            if new_muted {
                                tracing::info!(percent = 0, "PC volume MUTED");
                            } else {
                                tracing::info!(percent = pct, linear = new_vol, "PC volume");
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .ok();

        tracing::info!(
            window = WINDOW_TITLE,
            volume = vol,
            "GStreamer ready — window title '{WINDOW_TITLE}'; PC volume keys: +/- m 0-9"
        );

        Ok(Self {
            h264_pipeline: Mutex::new(h264_pipeline),
            h264_src: Mutex::new(h264_src),
            alac_pipeline: Mutex::new(alac_pipeline),
            alac_src: Mutex::new(alac_src),
            aac_eld_pipeline: Mutex::new(aac_eld_pipeline),
            aac_eld_src: Mutex::new(aac_eld_src),
            audio_compression: Mutex::new(None),
            hls_pipeline: Mutex::new(None),
            volume_milli,
            muted,
            volume_before_mute: Mutex::new(1.0),
            _main_loop_thread: Some(main_loop_thread),
            _volume_keys_thread: volume_keys_thread,
            main_loop_quit,
        })
    }

    /// Set PC-side linear volume in `0.0..=2.0` (does not change the phone).
    pub fn set_volume(&self, volume: f64) {
        let v = volume.clamp(0.0, 2.0);
        self.muted.store(false, Ordering::Relaxed);
        self.volume_milli
            .store(volume_to_milli(v), Ordering::Relaxed);
        if let Ok(p) = self.alac_pipeline.lock() {
            set_pipeline_volume(&p, "vol_alac", v);
        }
        if let Ok(p) = self.aac_eld_pipeline.lock() {
            set_pipeline_volume(&p, "vol_aac", v);
        }
        tracing::info!(linear = v, percent = (v * 100.0).round() as i32, "PC volume set");
    }

    pub fn volume(&self) -> f64 {
        milli_to_volume(self.volume_milli.load(Ordering::Relaxed))
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

fn volume_to_milli(v: f64) -> u32 {
    (v.clamp(0.0, 2.0) * 1000.0).round() as u32
}

fn milli_to_volume(m: u32) -> f64 {
    m as f64 / 1000.0
}

fn set_pipeline_volume(pipeline: &gst::Pipeline, name: &str, volume: f64) {
    if let Some(elem) = pipeline.by_name(name) {
        if elem.find_property("volume").is_some() {
            elem.set_property("volume", volume);
        }
    }
}

fn apply_window_title(pipeline: &gst::Pipeline, title: &str) {
    // Try every element (autovideosink wraps d3d11/d3d12 sinks).
    let iter = pipeline.iterate_recurse();
    for item in iter {
        let Ok(elem) = item else { continue };
        for prop in ["title", "window-title", "display-name"] {
            if elem.find_property(prop).is_some() {
                elem.set_property_from_str(prop, title);
            }
        }
    }
    #[cfg(windows)]
    rename_d3d_windows(title);
}

/// Windows D3D sinks often ignore GStreamer title props and use "Direct3D12 Renderer".
#[cfg(windows)]
fn rename_d3d_windows(title: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type HWND = *mut core::ffi::c_void;
    type BOOL = i32;
    type LPARAM = isize;

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(
            lp_enum_func: unsafe extern "system" fn(HWND, LPARAM) -> BOOL,
            l_param: LPARAM,
        ) -> BOOL;
        fn GetWindowTextW(h_wnd: HWND, lp_string: *mut u16, n_max_count: i32) -> i32;
        fn SetWindowTextW(h_wnd: HWND, lp_string: *const u16) -> BOOL;
        fn IsWindowVisible(h_wnd: HWND) -> BOOL;
    }

    struct Ctx {
        title: Vec<u16>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &*(lparam as *const Ctx);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return 1;
        }
        let text = String::from_utf16_lossy(&buf[..n as usize]);
        // Default GStreamer / D3D sink titles on Windows.
        let lower = text.to_ascii_lowercase();
        if lower.contains("direct3d")
            || lower == "gstreamer"
            || lower.contains("d3d12")
            || lower.contains("d3d11")
            || text == "Direct3D12 Renderer"
            || text == "Direct3D11 Renderer"
        {
            SetWindowTextW(hwnd, ctx.title.as_ptr());
        }
        1
    }

    let mut title_wide: Vec<u16> = OsStr::new(title).encode_wide().collect();
    title_wide.push(0);
    let ctx = Ctx { title: title_wide };
    unsafe {
        EnumWindows(enum_proc, &ctx as *const Ctx as LPARAM);
    }
}

#[cfg(not(windows))]
fn rename_d3d_windows(_title: &str) {}

impl AirPlayConsumer for GStreamerPlayer {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            window = WINDOW_TITLE,
            "video format; starting GStreamer window '{WINDOW_TITLE}'"
        );
        match self.h264_pipeline.lock() {
            Ok(p) => {
                apply_window_title(&p, WINDOW_TITLE);
                if let Err(e) = p.set_state(gst::State::Playing) {
                    tracing::error!(error = %e, "failed to play H.264 pipeline");
                }
                apply_window_title(&p, WINDOW_TITLE);
                #[cfg(windows)]
                rename_d3d_windows(WINDOW_TITLE);
            }
            Err(e) => tracing::error!(error = %e, "H.264 pipeline mutex poisoned"),
        }
    }

    fn on_video(&self, data: &[u8]) {
        static FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if n == 1 {
            #[cfg(windows)]
            rename_d3d_windows(WINDOW_TITLE);
            tracing::info!(bytes = data.len(), "first frame → window '{WINDOW_TITLE}'");
        } else if n % 120 == 0 {
            tracing::info!(n, bytes = data.len(), "gstreamer push H.264");
        }
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
        tracing::info!(?info, "audio format; PC volume applies to this stream (not phone)");
        if let Ok(mut ct) = self.audio_compression.lock() {
            *ct = info.compression_type;
        }
        let vol = if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            milli_to_volume(self.volume_milli.load(Ordering::Relaxed))
        };
        for (label, pipe, vol_name) in [
            ("ALAC", &self.alac_pipeline, "vol_alac"),
            ("AAC-ELD", &self.aac_eld_pipeline, "vol_aac"),
        ] {
            match pipe.lock() {
                Ok(p) => {
                    set_pipeline_volume(&p, vol_name, vol);
                    if let Err(e) = p.set_state(gst::State::Playing) {
                        tracing::warn!(pipeline = label, error = %e, "failed to play audio pipeline");
                    }
                }
                Err(e) => {
                    tracing::error!(pipeline = label, error = %e, "audio pipeline mutex poisoned")
                }
            }
        }
    }

    fn on_audio(&self, data: &[u8]) {
        let compression = self.audio_compression.lock().ok().and_then(|g| *g);
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
        // volume keys thread may block on stdin; detach by not joining long.
        if let Some(handle) = self._volume_keys_thread.take() {
            // Don't join forever if blocked on stdin.
            let _ = handle;
        }
        let _ = self.volume_before_mute;
    }
}
