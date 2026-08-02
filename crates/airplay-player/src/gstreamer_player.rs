//! GStreamer live player for AirPlay screen-mirror video (and optional audio).
//!
//! - Windows prefers D3D11 H.264 decode and D3D11 rendering, with runtime fallback.
//! - Video window title: **airplay2-rust**.
//! - PC-side volume (not phone): GStreamer `volume` element + console keys:
//!   `+` / `=` louder, `-` quieter, `m` mute/unmute, `0`–`9` set level
//!
//! Pipeline (H.264 annex-B):
//! `appsrc ! queue ! h264parse ! selected-decoder ! selected-video-sink`
//!
//! Video and audio use **separate** pipelines (AirPlay delivers them on separate
//! sockets) but share one GStreamer clock and base-time so sinks present in lockstep.
//! Both sinks use `sync=true`; low latency is achieved with leaky queues, not by
//! freerunning video ahead of audio.
//!
//! The direct path performs no re-encoding, fixed scaling, decoded-frame crop,
//! rotation, or CPU frame extraction.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use airplay_lib::{AudioStreamInfo, CompressionType, VideoStreamInfo};
use airplay_server::{AirPlayConsumer, PlaybackInfo, StreamGeneration};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::lifecycle::{GenerationGate, PlayerLifecycle};
use crate::system_volume::{
    airplay_db_is_mute, create_system_volume_controller, SystemVolumeController, VolumeSyncMode,
};
use crate::{
    decoder_candidates, sink_candidates, CodecGate, DecoderChoice, GateResult, PreviewMode,
    PreviewOptions, SinkChoice,
};

const WINDOW_TITLE: &str = "airplay2-rust";

/// Slight audio hold-back so H.264 decode latency does not leave lips ahead of voice.
/// ~80 ms is typical for hardware decode + sink; adjustable via preview mode later.
const AUDIO_LIP_SYNC_DELAY_NS: u64 = 80_000_000;

fn video_pipeline_description(
    decoder: DecoderChoice,
    sink: SinkChoice,
    options: PreviewOptions,
) -> String {
    let leaky = if options.queue_leaky {
        "downstream"
    } else {
        "no"
    };
    // Keep media-socket ingestion non-blocking but bounded. Quality mode keeps
    // older queued access units; latency-oriented modes keep the newest ones.
    let appsrc_leaky = if options.queue_leaky {
        "downstream"
    } else {
        "upstream"
    };
    let sink_properties = match sink {
        SinkChoice::D3d11VideoSink => format!("force-aspect-ratio=true sync={}", options.sink_sync),
        SinkChoice::AutoVideoSink => format!("sync={}", options.sink_sync),
    };
    format!(
        "appsrc name=h264-src is-live=true format=time do-timestamp=true block=false \
           max-buffers={} max-bytes=4194304 leaky-type={} \
         caps=video/x-h264,stream-format=byte-stream,alignment=au \
         ! queue name=video-queue max-size-buffers={} max-size-bytes=0 \
           max-size-time=0 leaky={} \
         ! h264parse config-interval=-1 \
         ! {} \
         ! {} name=videosink {}",
        options.queue_max_buffers,
        appsrc_leaky,
        options.queue_max_buffers,
        leaky,
        decoder.factory_name(),
        sink.factory_name(),
        sink_properties,
    )
}

fn preflight_to_ready(pipeline: &gst::Pipeline) -> Result<(), String> {
    pipeline
        .set_state(gst::State::Ready)
        .map_err(|error| format!("request READY: {error}"))?;

    let (transition, current, pending) = pipeline.state(gst::ClockTime::from_seconds(3));
    if let Some(message) = pipeline
        .bus()
        .and_then(|bus| bus.pop_filtered(&[gst::MessageType::Error]))
    {
        if let gst::MessageView::Error(error) = message.view() {
            return Err(format!(
                "asynchronous READY error from {}: {} ({:?})",
                message
                    .src()
                    .map(|source| source.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                error.error(),
                error.debug()
            ));
        }
    }
    transition.map_err(|error| {
        format!("READY transition failed: {error} (current={current:?}, pending={pending:?})")
    })?;
    if current != gst::State::Ready {
        return Err(format!(
            "READY transition timed out (current={current:?}, pending={pending:?})"
        ));
    }
    Ok(())
}

fn build_video_pipeline(
    preview_mode: PreviewMode,
    hardware_decode: bool,
) -> Result<
    (
        gst::Pipeline,
        gst_app::AppSrc,
        DecoderChoice,
        SinkChoice,
        Arc<AtomicU64>,
        Arc<AtomicU64>,
    ),
    String,
> {
    let options = preview_mode.options();
    let mut failures = Vec::new();

    for decoder in decoder_candidates(hardware_decode) {
        let decoder_name = decoder.factory_name();
        if gst::ElementFactory::find(decoder_name).is_none() {
            tracing::warn!(
                decoder = decoder_name,
                "GStreamer decoder unavailable; trying fallback"
            );
            continue;
        }

        for sink in sink_candidates(cfg!(windows)) {
            let sink_name = sink.factory_name();
            if gst::ElementFactory::find(sink_name).is_none() {
                tracing::warn!(
                    sink = sink_name,
                    "GStreamer video sink unavailable; trying fallback"
                );
                continue;
            }

            let description = video_pipeline_description(decoder, sink, options);
            let pipeline = match gst::parse::launch(&description)
                .map_err(|error| error.to_string())
                .and_then(|element| {
                    element
                        .downcast::<gst::Pipeline>()
                        .map_err(|_| "launch result is not a Pipeline".to_string())
                }) {
                Ok(pipeline) => pipeline,
                Err(error) => {
                    tracing::warn!(
                        decoder = decoder_name,
                        sink = sink_name,
                        %error,
                        "GStreamer video pipeline construction failed"
                    );
                    failures.push(format!("{decoder_name}+{sink_name}: {error}"));
                    continue;
                }
            };
            install_aspect_ratio_handler(&pipeline);

            if let Err(error) = preflight_to_ready(&pipeline) {
                let _ = pipeline.set_state(gst::State::Null);
                tracing::warn!(
                    decoder = decoder_name,
                    sink = sink_name,
                    %error,
                    "GStreamer video pipeline preflight failed"
                );
                failures.push(format!("{decoder_name}+{sink_name}: {error}"));
                continue;
            }
            apply_force_aspect_ratio(&pipeline);

            let Some(src) = pipeline.by_name("h264-src") else {
                let _ = pipeline.set_state(gst::State::Null);
                failures.push(format!("{decoder_name}+{sink_name}: missing h264-src"));
                continue;
            };
            let h264_src = match src.downcast::<gst_app::AppSrc>() {
                Ok(src) => src,
                Err(_) => {
                    let _ = pipeline.set_state(gst::State::Null);
                    failures.push(format!(
                        "{decoder_name}+{sink_name}: h264-src is not appsrc"
                    ));
                    continue;
                }
            };

            let caps = gst::Caps::from_str(
                "video/x-h264,stream-format=(string)byte-stream,alignment=(string)au",
            )
            .map_err(|error| format!("H.264 caps: {error}"))?;
            h264_src.set_caps(Some(&caps));
            h264_src.set_format(gst::Format::Time);
            h264_src.set_is_live(true);
            h264_src.set_stream_type(gst_app::AppStreamType::Stream);
            h264_src.set_property("block", false);
            h264_src.set_max_bytes(4 * 1024 * 1024);

            let appsrc_saturation_events = Arc::new(AtomicU64::new(0));
            let saturation_events = Arc::clone(&appsrc_saturation_events);
            h264_src.connect("enough-data", false, move |_| {
                let count = saturation_events.fetch_add(1, Ordering::Relaxed) + 1;
                if count <= 5 || count % 120 == 0 {
                    tracing::warn!(
                        count,
                        "GStreamer appsrc is full; configured leaky policy is dropping access units"
                    );
                }
                None
            });

            let queue_overruns = Arc::new(AtomicU64::new(0));
            if let Some(queue) = pipeline.by_name("video-queue") {
                let overruns = Arc::clone(&queue_overruns);
                let leaky = options.queue_leaky;
                queue.connect("overrun", false, move |_| {
                    let overruns = overruns.fetch_add(1, Ordering::Relaxed) + 1;
                    if overruns <= 5 || overruns % 120 == 0 {
                        if leaky {
                            tracing::warn!(
                                overruns,
                                "GStreamer video queue overrun; downstream-leaky mode drops old buffers"
                            );
                        } else {
                            tracing::warn!(
                                overruns,
                                "GStreamer quality-mode video queue is full"
                            );
                        }
                    }
                    None
                });
            }

            tracing::info!(
                decoder = decoder_name,
                hardware = decoder.is_hardware(),
                sink = sink_name,
                preview_mode = %preview_mode,
                queue_max_buffers = options.queue_max_buffers,
                queue_leaky = options.queue_leaky,
                sink_sync = options.sink_sync,
                caps = %caps,
                "selected GStreamer video pipeline"
            );
            return Ok((
                pipeline,
                h264_src,
                decoder,
                sink,
                queue_overruns,
                appsrc_saturation_events,
            ));
        }
    }

    Err(format!(
        "no usable GStreamer H.264 decoder/video sink combination ({})",
        failures.join("; ")
    ))
}

/// How to apply `videoflip` for stream orientation.
///
/// **Default / Auto:** follow the phone stream with **no** forced flip:
/// - **Portrait** (height > width) — home screen / apps (default)
/// - **Landscape** (width ≥ height) — games & landscape apps
///
/// Use `Cw` / `Ccw` only if a specific device still paints sideways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RotateMode {
    /// Follow stream: no videoflip; portrait UI vs landscape game via dimensions.
    #[default]
    Auto,
    None,
    /// Force 90° clockwise.
    Cw,
    /// Force 90° counter-clockwise.
    Ccw,
}

impl RotateMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "false" | "0" => Self::None,
            "cw" | "clockwise" | "right" | "90" => Self::Cw,
            "ccw" | "counterclockwise" | "counter-clockwise" | "left" | "270" => Self::Ccw,
            _ => Self::Auto, // "auto" | "true" | "1" | "portrait" | unknown
        }
    }
}

/// Live GStreamer-backed consumer: titled video window + PC volume control.
///
/// ## Window ownership
///
/// The playback HWND is **sink-owned** (`d3d11videosink` / `autovideosink` creates
/// it). Moving the pipeline to NULL destroys that window. The receiver process and
/// control listener stay alive; a later `on_video_format` restarts PLAYING and the
/// sink creates a new window.
///
/// ## Generations
///
/// Each SETUP claims a stream generation. Disconnect/TEARDOWN for an older
/// generation is ignored so YouTube stream replacement cannot stop the new pipeline.
///
/// ## A/V sync
///
/// Mirror video and audio are separate pipelines (separate AirPlay sockets) but
/// share one GStreamer system clock and a common base-time. Audio sinks use a
/// small render delay so decoded video catches up (lip-sync).
pub struct GStreamerPlayer {
    h264_pipeline: Mutex<gst::Pipeline>,
    h264_src: Mutex<gst_app::AppSrc>,
    codec_gate: Mutex<CodecGate>,
    selected_decoder: DecoderChoice,
    selected_sink: SinkChoice,
    preview_mode: PreviewMode,
    hardware_decode: bool,
    queue_overruns: Arc<AtomicU64>,
    appsrc_saturation_events: Arc<AtomicU64>,
    waiting_config_drops: AtomicU64,
    waiting_idr_drops: AtomicU64,
    frames_pushed: AtomicU64,
    alac_pipeline: Mutex<gst::Pipeline>,
    alac_src: Mutex<gst_app::AppSrc>,
    aac_eld_pipeline: Mutex<gst::Pipeline>,
    aac_eld_src: Mutex<gst_app::AppSrc>,
    audio_compression: Mutex<Option<CompressionType>>,
    hls_pipeline: Mutex<Option<gst::Pipeline>>,
    /// Shared linear volume (0.0–2.0), stored as milli-units (1000 = 1.0).
    volume_milli: Arc<AtomicU32>,
    /// Last volume requested by the AirPlay sender, in milli-decibels.
    airplay_volume_db_milli: AtomicI32,
    muted: Arc<AtomicBool>,
    volume_before_mute: Mutex<f64>,
    last_size: Mutex<(u32, u32)>,
    video_generation: GenerationGate,
    audio_generation: GenerationGate,
    video_lifecycle: PlayerLifecycle,
    audio_lifecycle: PlayerLifecycle,
    volume_sync: VolumeSyncMode,
    system_volume: Arc<dyn SystemVolumeController>,
    /// Shared clock for video + audio pipelines (A/V lockstep).
    shared_clock: gst::Clock,
    /// Base-time applied when the first of video/audio starts PLAYING.
    shared_base_time: Mutex<Option<gst::ClockTime>>,
    _main_loop_thread: Option<JoinHandle<()>>,
    _volume_keys_thread: Option<JoinHandle<()>>,
    main_loop_quit: Arc<AtomicBool>,
}

impl GStreamerPlayer {
    /// Initialize GStreamer, pipelines, window title fixer, and volume key thread.
    pub fn new() -> Result<Self, String> {
        Self::with_preview(1.0, PreviewMode::Balanced, true)
    }

    /// Create with initial PC volume in `0.0..=2.0` (1.0 = unity).
    pub fn with_volume(initial: f64) -> Result<Self, String> {
        Self::with_preview(initial, PreviewMode::Balanced, true)
    }

    /// Compatibility constructor. Direct GStreamer preview intentionally
    /// bypasses decoded-frame rotation and crop processing.
    pub fn with_options(
        initial: f64,
        rotate_mode: RotateMode,
        detect_game: bool,
    ) -> Result<Self, String> {
        Self::with_all_options(
            initial,
            PreviewMode::Balanced,
            true,
            rotate_mode,
            detect_game,
            VolumeSyncMode::Player,
            None,
        )
    }

    pub fn with_preview(
        initial: f64,
        preview_mode: PreviewMode,
        hardware_decode: bool,
    ) -> Result<Self, String> {
        Self::with_volume_sync(
            initial,
            preview_mode,
            hardware_decode,
            VolumeSyncMode::Player,
            None,
        )
    }

    /// Create with explicit AirPlay → volume sync mode and optional controller override.
    pub fn with_volume_sync(
        initial: f64,
        preview_mode: PreviewMode,
        hardware_decode: bool,
        volume_sync: VolumeSyncMode,
        system_volume: Option<Arc<dyn SystemVolumeController>>,
    ) -> Result<Self, String> {
        Self::with_all_options(
            initial,
            preview_mode,
            hardware_decode,
            RotateMode::Auto,
            false,
            volume_sync,
            system_volume,
        )
    }

    fn with_all_options(
        initial: f64,
        preview_mode: PreviewMode,
        hardware_decode: bool,
        rotate_mode: RotateMode,
        detect_game: bool,
        volume_sync: VolumeSyncMode,
        system_volume: Option<Arc<dyn SystemVolumeController>>,
    ) -> Result<Self, String> {
        gst::init().map_err(|e| format!("gstreamer init failed: {e}"))?;

        let system_volume = system_volume.unwrap_or_else(|| match volume_sync {
            VolumeSyncMode::System => create_system_volume_controller(),
            _ => Arc::new(crate::system_volume::NoopSystemVolumeController),
        });

        let volume_milli = Arc::new(AtomicU32::new(volume_to_milli(initial.clamp(0.0, 2.0))));
        let muted = Arc::new(AtomicBool::new(false));
        let main_loop_quit = Arc::new(AtomicBool::new(false));

        // Pump GLib so video sinks can create/update the HWND.
        let quit_flag = Arc::clone(&main_loop_quit);
        let main_loop_thread = thread::Builder::new()
            .name("gstreamer-main-loop".into())
            .spawn(move || {
                let main_context = glib::MainContext::default();
                let guard = match main_context.acquire() {
                    Ok(g) => g,
                    Err(e) => {
                        tracing::error!(error = %e, "failed to acquire GLib main context");
                        tracing::info!("GStreamer main loop task exited (acquire failed)");
                        return;
                    }
                };
                let _guard = guard;
                while !quit_flag.load(Ordering::Relaxed) {
                    let _ = main_context.iteration(false);
                    // Keep retitling Windows D3D sinks (they often reset the title).
                    #[cfg(windows)]
                    rename_d3d_windows(WINDOW_TITLE);
                    thread::sleep(Duration::from_millis(50));
                }
                tracing::info!("GStreamer main loop task exited");
            })
            .map_err(|e| format!("spawn gstreamer main loop: {e}"))?;

        let (
            h264_pipeline,
            h264_src,
            selected_decoder,
            selected_sink,
            queue_overruns,
            appsrc_saturation_events,
        ) = build_video_pipeline(preview_mode, hardware_decode)?;

        apply_window_title(&h264_pipeline, WINDOW_TITLE);
        if rotate_mode != RotateMode::Auto || detect_game {
            tracing::info!(
                ?rotate_mode,
                detect_game,
                "direct GStreamer preview bypasses decoded-frame rotation and crop processing"
            );
        }

        // volume element = PC-side gain (does not change phone volume).
        // Audio sinks use sync=true and share the video clock (see attach_shared_clock).
        // ts-offset delays audio slightly so H.264 decode latency does not leave lips early.
        let audio_delay_ns = AUDIO_LIP_SYNC_DELAY_NS as i64;
        let alac_pipeline = gst::parse::launch(&format!(
            "appsrc name=alac_src is-live=true format=time do-timestamp=true block=false \
             max-buffers=32 max-bytes=262144 leaky-type=downstream \
             ! queue max-size-buffers=16 max-size-time=0 max-size-bytes=0 leaky=downstream \
             ! avdec_alac ! audioconvert ! audioresample \
             ! volume name=vol_alac volume=1.0 \
             ! autoaudiosink name=audiosink sync=true ts-offset={audio_delay_ns}"
        ))
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

        let aac_eld_pipeline = gst::parse::launch(&format!(
            "appsrc name=aac_eld_src is-live=true format=time do-timestamp=true block=false \
             max-buffers=32 max-bytes=262144 leaky-type=downstream \
             ! queue max-size-buffers=16 max-size-time=0 max-size-bytes=0 leaky=downstream \
             ! avdec_aac ! audioconvert ! audioresample \
             ! volume name=vol_aac volume=1.0 \
             ! autoaudiosink name=audiosink sync=true ts-offset={audio_delay_ns}"
        ))
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

        // Shared clock for A/V lockstep across separate pipelines.
        let shared_clock = gst::SystemClock::obtain();
        attach_shared_clock(&h264_pipeline, &shared_clock);
        attach_shared_clock(&alac_pipeline, &shared_clock);
        attach_shared_clock(&aac_eld_pipeline, &shared_clock);
        // Provide a small latency budget so sinks can wait for both streams.
        for p in [&h264_pipeline, &alac_pipeline, &aac_eld_pipeline] {
            p.set_latency(gst::ClockTime::from_mseconds(100));
        }

        install_bus_watch(&h264_pipeline, "video", 0);
        install_bus_watch(&alac_pipeline, "audio", 0);
        install_bus_watch(&aac_eld_pipeline, "audio", 0);

        tracing::info!(
            audio_lip_sync_delay_ms = AUDIO_LIP_SYNC_DELAY_NS / 1_000_000,
            preview_mode = %preview_mode,
            sink_sync = preview_mode.options().sink_sync,
            "A/V sync: shared clock + audio ts-offset; sinks paced (sync=true)"
        );

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
                tracing::info!("PC volume keys task exited");
            })
            .ok();

        tracing::info!(
            window = WINDOW_TITLE,
            volume = vol,
            preview_mode = %preview_mode,
            hardware_decode,
            decoder = selected_decoder.factory_name(),
            sink = selected_sink.factory_name(),
            "GStreamer direct preview ready; PC volume: +/- m 0-9"
        );

        Ok(Self {
            h264_pipeline: Mutex::new(h264_pipeline),
            h264_src: Mutex::new(h264_src),
            codec_gate: Mutex::new(CodecGate::default()),
            selected_decoder,
            selected_sink,
            preview_mode,
            hardware_decode,
            queue_overruns,
            appsrc_saturation_events,
            waiting_config_drops: AtomicU64::new(0),
            waiting_idr_drops: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(0),
            alac_pipeline: Mutex::new(alac_pipeline),
            alac_src: Mutex::new(alac_src),
            aac_eld_pipeline: Mutex::new(aac_eld_pipeline),
            aac_eld_src: Mutex::new(aac_eld_src),
            audio_compression: Mutex::new(None),
            hls_pipeline: Mutex::new(None),
            volume_milli,
            airplay_volume_db_milli: AtomicI32::new(0),
            muted,
            volume_before_mute: Mutex::new(1.0),
            last_size: Mutex::new((0, 0)),
            video_generation: GenerationGate::new(),
            audio_generation: GenerationGate::new(),
            video_lifecycle: PlayerLifecycle::new(),
            audio_lifecycle: PlayerLifecycle::new(),
            volume_sync,
            system_volume,
            shared_clock,
            shared_base_time: Mutex::new(None),
            _main_loop_thread: Some(main_loop_thread),
            _volume_keys_thread: volume_keys_thread,
            main_loop_quit,
        })
    }

    /// AirPlay volume sync mode for this player.
    pub fn volume_sync_mode(&self) -> VolumeSyncMode {
        self.volume_sync
    }

    /// Start a mirror pipeline with the shared A/V clock and a common base-time.
    ///
    /// The first of video/audio to start captures `base_time` from the shared
    /// clock; later pipelines reuse it so sinks present on the same timeline.
    fn start_pipeline_synced(&self, pipeline: &gst::Pipeline, label: &str) -> Result<(), String> {
        attach_shared_clock(pipeline, &self.shared_clock);
        // Live pipelines: do not use pipeline base-time auto-distribute fighting us.
        pipeline.set_start_time(gst::ClockTime::NONE);
        pipeline.set_base_time(gst::ClockTime::ZERO);

        let base = {
            let mut slot = self
                .shared_base_time
                .lock()
                .map_err(|_| "shared_base_time mutex poisoned".to_string())?;
            if slot.is_none() {
                // Prefer the clock's current time as a common origin. A clock that
                // reports no time yet is not fatal: fall back to zero so both
                // pipelines still share the same origin.
                let now = self.shared_clock.time().unwrap_or(gst::ClockTime::ZERO);
                *slot = Some(now);
                tracing::info!(
                    base_time_ns = now.nseconds(),
                    "A/V shared base-time established"
                );
            }
            match *slot {
                Some(base) => base,
                None => gst::ClockTime::ZERO,
            }
        };
        pipeline.set_base_time(base);

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("{label} play failed: {e}"))?;
        Ok(())
    }

    /// Clear shared base-time when both mirror A/V are stopped so the next
    /// session starts a fresh timeline.
    fn maybe_reset_shared_base_time(&self) {
        let video_idle = matches!(
            self.video_lifecycle.get(),
            crate::PlayerState::Idle | crate::PlayerState::Stopped
        );
        let audio_idle = matches!(
            self.audio_lifecycle.get(),
            crate::PlayerState::Idle | crate::PlayerState::Stopped
        );
        if video_idle && audio_idle {
            if let Ok(mut slot) = self.shared_base_time.lock() {
                if slot.take().is_some() {
                    tracing::debug!("A/V shared base-time cleared");
                }
            }
        }
    }

    /// Stop the H.264 pipeline if `generation` still owns video. Idempotent.
    fn stop_video_pipeline_if_owner(&self, generation: StreamGeneration) {
        if !self.video_generation.stop_if_owner(generation) {
            return;
        }
        if !self.video_lifecycle.begin_stop() {
            tracing::debug!(generation, "H.264 pipeline stop already in progress");
            return;
        }
        // Re-check after winning begin_stop: a newer SETUP may have claimed.
        // Restore Playing so we don't leave gen N+1 stuck in Stopping.
        if !self.video_generation.is_owner(generation) {
            tracing::debug!(
                generation,
                active = self.video_generation.active(),
                "aborting video stop; generation replaced during shutdown"
            );
            self.video_lifecycle.mark_playing();
            return;
        }
        tracing::info!(
            generation,
            pipeline_type = "video",
            "video source disconnected; stopping H.264 pipeline (session-local)"
        );
        if let Ok(mut gate) = self.codec_gate.lock() {
            gate.reset();
        }
        self.waiting_config_drops.store(0, Ordering::Relaxed);
        self.waiting_idr_drops.store(0, Ordering::Relaxed);
        self.frames_pushed.store(0, Ordering::Relaxed);
        self.queue_overruns.store(0, Ordering::Relaxed);
        self.appsrc_saturation_events.store(0, Ordering::Relaxed);
        if let Ok(mut size) = self.last_size.lock() {
            *size = (0, 0);
        }
        // Drop the mutex before the (potentially blocking) state change.
        let pipeline = match self.h264_pipeline.lock() {
            Ok(p) => p.clone(),
            Err(e) => {
                tracing::error!(error = %e, "H.264 pipeline mutex poisoned on disconnect");
                if self.video_generation.is_owner(generation) {
                    self.video_lifecycle.mark_stopped();
                }
                return;
            }
        };
        // Final owner check immediately before NULL — never stop a newer stream.
        if !self.video_generation.is_owner(generation) {
            tracing::debug!(
                generation,
                active = self.video_generation.active(),
                "skipping H.264 NULL; newer generation is active"
            );
            self.video_lifecycle.mark_playing();
            return;
        }
        // Send EOS then NULL so sinks release cleanly; errors are non-fatal.
        if let Ok(src) = self.h264_src.lock() {
            let _ = src.end_of_stream();
        }
        if let Err(e) = pipeline.set_state(gst::State::Null) {
            tracing::warn!(error = %e, generation, "H.264 pipeline NULL transition failed");
        }
        if self.video_generation.is_owner(generation) {
            self.video_lifecycle.mark_stopped();
        }
        self.maybe_reset_shared_base_time();
        // Sink-owned HWND is destroyed with NULL; process + control server remain.
        tracing::info!(
            generation,
            "H.264 pipeline stopped; receiver remains available for next stream"
        );
    }

    fn stop_audio_pipelines_if_owner(&self, generation: StreamGeneration) {
        if !self.audio_generation.stop_if_owner(generation) {
            return;
        }
        if !self.audio_lifecycle.begin_stop() {
            tracing::debug!(generation, "audio pipeline stop already in progress");
            return;
        }
        if !self.audio_generation.is_owner(generation) {
            tracing::debug!(
                generation,
                active = self.audio_generation.active(),
                "aborting audio stop; generation replaced during shutdown"
            );
            self.audio_lifecycle.mark_playing();
            return;
        }
        tracing::debug!(
            generation,
            pipeline_type = "audio",
            "audio source disconnected; stopping audio pipelines"
        );
        let pipes: Vec<gst::Pipeline> = [&self.alac_pipeline, &self.aac_eld_pipeline]
            .iter()
            .filter_map(|pipe| pipe.lock().ok().map(|p| p.clone()))
            .collect();
        if !self.audio_generation.is_owner(generation) {
            self.audio_lifecycle.mark_playing();
            return;
        }
        for p in pipes {
            if let Err(e) = p.set_state(gst::State::Null) {
                tracing::warn!(error = %e, generation, "audio pipeline NULL transition failed");
            }
        }
        if let Ok(mut ct) = self.audio_compression.lock() {
            *ct = None;
        }
        if self.audio_generation.is_owner(generation) {
            self.audio_lifecycle.mark_stopped();
        }
        self.maybe_reset_shared_base_time();
    }

    /// Re-enter PLAYING if the audio pipeline was paused/null'd by a transient error
    /// or a stale stop that lost the race with a new generation.
    fn ensure_audio_pipeline_playing(&self, label: &str, pipe: &Mutex<gst::Pipeline>) {
        // Only revive when this player still believes audio is active.
        if !matches!(
            self.audio_lifecycle.get(),
            crate::PlayerState::Playing | crate::PlayerState::Starting
        ) {
            return;
        }
        let Ok(p) = pipe.lock() else {
            return;
        };
        let (_res, current, _pending) = p.state(gst::ClockTime::ZERO);
        if current == gst::State::Playing {
            return;
        }
        if current == gst::State::Paused {
            if let Err(e) = p.set_state(gst::State::Playing) {
                tracing::debug!(pipeline = label, error = %e, "audio resume from Paused failed");
            }
            return;
        }
        // Null/Ready — try to bring back up for the live mirror session.
        let vol = if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            milli_to_volume(self.volume_milli.load(Ordering::Relaxed))
        };
        let vol_name = if label.starts_with("ALAC") {
            "vol_alac"
        } else {
            "vol_aac"
        };
        set_pipeline_volume(&p, vol_name, vol);
        drop(p);
        // Re-lock via start path needs owned pipeline clone.
        let Ok(p) = pipe.lock() else {
            return;
        };
        if let Err(e) = self.start_pipeline_synced(&p, label) {
            tracing::warn!(
                pipeline = label,
                error = %e,
                ?current,
                "audio pipeline revive to PLAYING failed"
            );
        } else {
            tracing::info!(pipeline = label, from = ?current, "audio pipeline revived to PLAYING");
        }
    }

    /// Set software (pipeline) linear volume in `0.0..=2.0`.
    /// Does not change Windows system master volume.
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
        // Media playbin (YouTube HLS) exposes a top-level `volume` property.
        if let Ok(slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.as_ref() {
                if p.find_property("volume").is_some() {
                    p.set_property("volume", v);
                }
                if p.find_property("mute").is_some() {
                    p.set_property("mute", v <= 0.000_1);
                }
            }
        }
        tracing::info!(
            linear = v,
            percent = (v * 100.0).round() as i32,
            "software volume set (GStreamer)"
        );
    }

    pub fn volume(&self) -> f64 {
        milli_to_volume(self.volume_milli.load(Ordering::Relaxed))
    }

    fn push_to_appsrc(src: &gst_app::AppSrc, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let Ok(mut buffer) = gst::Buffer::with_size(data.len()) else {
            tracing::warn!(
                "gstreamer: failed to allocate buffer of size {}",
                data.len()
            );
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
            // Flushing/EOS after a brief pipeline restart is common; caller may revive.
            tracing::debug!(error = %e, "gstreamer: push_buffer failed");
        }
    }
}

fn volume_to_milli(v: f64) -> u32 {
    (v.clamp(0.0, 2.0) * 1000.0).round() as u32
}

fn milli_to_volume(m: u32) -> f64 {
    m as f64 / 1000.0
}

fn airplay_db_to_linear(volume_db: f64) -> f64 {
    // Used only for VolumeSyncMode::Player (pipeline gain).
    // 0.0 dB => 1.0 (maximum). Mute threshold => 0.0.
    crate::system_volume::airplay_db_to_amplitude(volume_db)
}

fn set_pipeline_volume(pipeline: &gst::Pipeline, name: &str, volume: f64) {
    if let Some(elem) = pipeline.by_name(name) {
        if elem.find_property("volume").is_some() {
            elem.set_property("volume", volume);
        }
    }
}

/// Pad probe: sample decoded BGRx frames for letterboxed landscape games (Hoyoverse-style).
#[cfg(any())]
fn install_letterbox_probe(pipeline: &gst::Pipeline, tracker: Arc<Mutex<ModeTracker>>) {
    let Some(crop_el) = pipeline.by_name("vcrop") else {
        tracing::warn!("videocrop missing; letterbox game detect disabled");
        return;
    };
    let Some(pad) = crop_el.static_pad("sink") else {
        tracing::warn!("videocrop sink pad missing");
        return;
    };

    let counter = AtomicU64::new(0);
    pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
        let n = counter.fetch_add(1, Ordering::Relaxed);
        if n % LETTERBOX_SAMPLE_EVERY != 0 {
            return gst::PadProbeReturn::Ok;
        }

        let Some(buffer) = info.buffer() else {
            return gst::PadProbeReturn::Ok;
        };
        let Some(caps) = pad.current_caps() else {
            return gst::PadProbeReturn::Ok;
        };
        let Some(s) = caps.structure(0) else {
            return gst::PadProbeReturn::Ok;
        };
        let width = s.get::<i32>("width").unwrap_or(0).max(0) as u32;
        let height = s.get::<i32>("height").unwrap_or(0).max(0) as u32;
        if width == 0 || height == 0 || width >= height {
            // Landscape stream: size path already handles game mode.
            return gst::PadProbeReturn::Ok;
        }

        let map = match buffer.map_readable() {
            Ok(m) => m,
            Err(_) => return gst::PadProbeReturn::Ok,
        };
        let data = map.as_slice();
        // BGRx = 4 bytes/pixel; stride often = width * 4 (tight).
        let bpp = 4usize;
        let stride = (width as usize).saturating_mul(bpp);
        if stride == 0 || data.len() < stride.saturating_mul(height as usize) {
            return gst::PadProbeReturn::Ok;
        }

        let sample = detect_letterbox_game(data, width, height, stride, bpp);
        let changed = {
            let Ok(mut t) = tracker.lock() else {
                return gst::PadProbeReturn::Ok;
            };
            // Only letterbox-detect while stream is portrait (home advertise / tall frame).
            if t.mode == ContentMode::GameLandscape && !t.crop.is_active() {
                return gst::PadProbeReturn::Ok;
            }
            t.on_letterbox_sample(sample)
        };

        if changed {
            if let Ok(t) = tracker.lock() {
                if let Some(parent) = pad.parent_element() {
                    for (prop, val) in [
                        ("top", t.crop.top as i32),
                        ("bottom", t.crop.bottom as i32),
                        ("left", 0i32),
                        ("right", 0i32),
                    ] {
                        if parent.find_property(prop).is_some() {
                            parent.set_property(prop, val);
                        }
                    }
                }
                tracing::info!(
                    content = t.mode.as_str(),
                    crop_top = t.crop.top,
                    crop_bottom = t.crop.bottom,
                    width,
                    height,
                    "content mode (letterbox): {}",
                    if t.mode == ContentMode::GameLandscape {
                        "landscape game detected — cropped black bars"
                    } else {
                        "home / portrait UI"
                    }
                );
            }
        }

        gst::PadProbeReturn::Ok
    });

    tracing::info!("letterbox game detector installed (portrait stream + black bars → crop)");
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

fn apply_force_aspect_ratio(pipeline: &gst::Pipeline) {
    // autovideosink creates its concrete child during state changes. Revisit
    // the whole hierarchy so any compatible sink preserves sender geometry.
    for item in pipeline.iterate_recurse() {
        let Ok(element) = item else { continue };
        if element.find_property("force-aspect-ratio").is_some() {
            element.set_property("force-aspect-ratio", true);
        }
    }
}

fn install_aspect_ratio_handler(pipeline: &gst::Pipeline) {
    pipeline.connect_deep_element_added(|_, _, element| {
        if element.find_property("force-aspect-ratio").is_some() {
            element.set_property("force-aspect-ratio", true);
        }
    });
}

/// Pin a pipeline to the shared clock so video and audio present on one timeline.
///
/// AirPlay delivers mirror video and audio on separate sockets, so they live in
/// separate pipelines. Without a common clock each pipeline picks its own and the
/// two drift apart (lips ahead of voice).
fn attach_shared_clock(pipeline: &gst::Pipeline, clock: &gst::Clock) {
    // use_clock() disables clock auto-selection and forces this exact clock.
    pipeline.use_clock(Some(clock));
}

/// Bus watcher: log Error/Warning/Eos/StateChanged. Never panics; never exits the process.
fn install_bus_watch(pipeline: &gst::Pipeline, pipeline_type: &'static str, generation: u64) {
    let Some(bus) = pipeline.bus() else {
        tracing::warn!(pipeline_type, generation, "pipeline has no bus");
        return;
    };
    bus.set_sync_handler(move |_bus, msg| {
        use gst::MessageView;
        let source = msg
            .src()
            .map(|src| src.path_string())
            .unwrap_or_else(|| glib::GString::from("unknown"));
        match msg.view() {
            MessageView::Error(err) => {
                tracing::error!(
                    source = %source,
                    error = %err.error(),
                    debug = ?err.debug(),
                    pipeline_type,
                    generation,
                    "GStreamer pipeline error (session-local; control server unaffected)"
                );
            }
            MessageView::Warning(w) => {
                tracing::warn!(
                    source = %source,
                    error = %w.error(),
                    debug = ?w.debug(),
                    pipeline_type,
                    generation,
                    "GStreamer warning"
                );
            }
            MessageView::Eos(_) => {
                tracing::info!(
                    source = %source,
                    pipeline_type,
                    generation,
                    "GStreamer EOS"
                );
            }
            MessageView::StateChanged(sc) => {
                let is_pipeline = msg
                    .src()
                    .and_then(|src| src.downcast_ref::<gst::Pipeline>())
                    .is_some();
                if is_pipeline {
                    tracing::info!(
                        source = %source,
                        old = ?sc.old(),
                        current = ?sc.current(),
                        pending = ?sc.pending(),
                        pipeline_type,
                        generation,
                        "GStreamer pipeline state changed"
                    );
                }
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
    fn on_video_format(&self, info: &VideoStreamInfo, generation: StreamGeneration) {
        // Mark this generation active first so any in-flight stale disconnect is ignored.
        self.video_generation.claim(generation);
        self.video_lifecycle.reset_for_start();
        self.video_lifecycle.mark_starting();
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            generation,
            window = WINDOW_TITLE,
            decoder = self.selected_decoder.factory_name(),
            hardware = self.selected_decoder.is_hardware(),
            hardware_requested = self.hardware_decode,
            sink = self.selected_sink.factory_name(),
            preview_mode = %self.preview_mode,
            "video format; starting direct GStreamer preview"
        );
        if let Ok(mut gate) = self.codec_gate.lock() {
            gate.reset();
        }
        self.waiting_config_drops.store(0, Ordering::Relaxed);
        self.waiting_idr_drops.store(0, Ordering::Relaxed);
        self.frames_pushed.store(0, Ordering::Relaxed);
        self.queue_overruns.store(0, Ordering::Relaxed);
        self.appsrc_saturation_events.store(0, Ordering::Relaxed);
        if let Ok(mut s) = self.last_size.lock() {
            *s = (0, 0);
        }
        let pipeline = match self.h264_pipeline.lock() {
            Ok(p) => p.clone(),
            Err(e) => {
                tracing::error!(error = %e, generation, "H.264 pipeline mutex poisoned");
                return;
            }
        };
        install_bus_watch(&pipeline, "video", generation);
        apply_window_title(&pipeline, WINDOW_TITLE);
        apply_force_aspect_ratio(&pipeline);
        // Ensure a clean restart after a prior NULL (sink will recreate the HWND).
        let _ = pipeline.set_state(gst::State::Ready);
        if let Err(e) = self.start_pipeline_synced(&pipeline, "H.264") {
            tracing::error!(error = %e, generation, "failed to play H.264 pipeline");
            return;
        }
        apply_window_title(&pipeline, WINDOW_TITLE);
        apply_force_aspect_ratio(&pipeline);
        #[cfg(windows)]
        rename_d3d_windows(WINDOW_TITLE);
        self.video_lifecycle.mark_playing();
    }

    fn on_video_size(&self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let changed = {
            let mut last = match self.last_size.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if *last == (width, height) {
                false
            } else {
                *last = (width, height);
                true
            }
        };
        if changed {
            tracing::info!(
                width,
                height,
                aspect_ratio = width as f64 / height as f64,
                "video resolution changed; preserving sender geometry"
            );
        }
    }

    fn on_video(&self, data: &[u8]) {
        let gated = match self.codec_gate.lock() {
            Ok(mut gate) => gate.push(data),
            Err(error) => {
                tracing::error!(%error, "H.264 codec gate mutex poisoned");
                return;
            }
        };
        let access_unit = match gated {
            GateResult::CodecConfigUpdated => {
                tracing::info!(bytes = data.len(), "updated H.264 SPS/PPS; waiting for IDR");
                return;
            }
            GateResult::WaitingForConfig => {
                let dropped = self.waiting_config_drops.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped <= 5 || dropped % 120 == 0 {
                    tracing::warn!(
                        dropped,
                        bytes = data.len(),
                        "missing SPS/PPS; dropping access unit"
                    );
                }
                return;
            }
            GateResult::WaitingForIdr => {
                let dropped = self.waiting_idr_drops.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped <= 5 || dropped % 120 == 0 {
                    tracing::warn!(
                        dropped,
                        bytes = data.len(),
                        "waiting for IDR; dropping inter frame"
                    );
                }
                return;
            }
            GateResult::Rejected(error) => {
                tracing::warn!(%error, bytes = data.len(), "rejected H.264 input buffer");
                return;
            }
            GateResult::AccessUnit(access_unit) => access_unit,
        };
        let n = self.frames_pushed.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 {
            #[cfg(windows)]
            rename_d3d_windows(WINDOW_TITLE);
            tracing::info!(
                bytes = access_unit.len(),
                "first complete access unit to preview"
            );
        } else if n % 120 == 0 {
            tracing::info!(
                n,
                bytes = access_unit.len(),
                "gstreamer push H.264 access unit"
            );
        }
        match self.h264_src.lock() {
            Ok(src) => Self::push_to_appsrc(&src, &access_unit),
            Err(e) => tracing::error!(error = %e, "H.264 appsrc mutex poisoned"),
        }
    }

    fn on_video_src_disconnect(&self, generation: StreamGeneration) {
        self.stop_video_pipeline_if_owner(generation);
    }

    fn on_audio_format(&self, info: &AudioStreamInfo, generation: StreamGeneration) {
        self.audio_generation.claim(generation);
        self.audio_lifecycle.reset_for_start();
        self.audio_lifecycle.mark_starting();
        tracing::info!(
            ?info,
            generation,
            "audio format; PC volume applies to this stream (not phone)"
        );
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
            let pipeline = match pipe.lock() {
                Ok(p) => p.clone(),
                Err(e) => {
                    tracing::error!(pipeline = label, error = %e, "audio pipeline mutex poisoned");
                    continue;
                }
            };
            install_bus_watch(&pipeline, "audio", generation);
            set_pipeline_volume(&pipeline, vol_name, vol);
            if let Err(e) = self.start_pipeline_synced(&pipeline, label) {
                tracing::warn!(
                    pipeline = label,
                    error = %e,
                    generation,
                    "failed to play audio pipeline"
                );
            }
        }
        self.audio_lifecycle.mark_playing();
    }

    fn on_audio(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let compression = self.audio_compression.lock().ok().and_then(|g| *g);
        match compression {
            Some(CompressionType::Alac) => {
                self.ensure_audio_pipeline_playing("ALAC", &self.alac_pipeline);
                match self.alac_src.lock() {
                    Ok(src) => Self::push_to_appsrc(&src, data),
                    Err(e) => tracing::error!(error = %e, "ALAC appsrc mutex poisoned"),
                }
            }
            Some(CompressionType::AacEld) | Some(CompressionType::Aac) => {
                self.ensure_audio_pipeline_playing("AAC-ELD", &self.aac_eld_pipeline);
                match self.aac_eld_src.lock() {
                    Ok(src) => Self::push_to_appsrc(&src, data),
                    Err(e) => tracing::error!(error = %e, "AAC-ELD appsrc mutex poisoned"),
                }
            }
            other => {
                // Avoid silent permanent mute when format callback was missed.
                static ONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let n = ONCE.fetch_add(1, Ordering::Relaxed);
                if n < 5 || n % 200 == 0 {
                    tracing::warn!(
                        ?other,
                        n,
                        "audio frame without known compression; waiting for on_audio_format"
                    );
                }
            }
        }
    }

    fn on_audio_src_disconnect(&self, generation: StreamGeneration) {
        self.stop_audio_pipelines_if_owner(generation);
    }

    fn on_volume(&self, volume_db: f64) {
        // Protocol: 0.0 dB = maximum, NOT mute. Clamp only non-finite extremes.
        let volume_db = if volume_db.is_finite() {
            volume_db.clamp(-144.0, 0.0)
        } else {
            -144.0
        };
        self.airplay_volume_db_milli
            .store((volume_db * 1000.0).round() as i32, Ordering::Relaxed);

        match self.volume_sync {
            VolumeSyncMode::Disabled => {
                tracing::debug!(volume_db, "AirPlay volume ignored (volume_sync=disabled)");
            }
            VolumeSyncMode::System => {
                // Keep GStreamer pipeline at unity so OS master volume is the only attenuator.
                self.set_volume(1.0);
                if let Err(e) = self.system_volume.set_airplay_volume_db(volume_db) {
                    tracing::warn!(
                        error = %e,
                        volume_db,
                        "system volume update failed (playback continues)"
                    );
                } else if airplay_db_is_mute(volume_db) {
                    tracing::info!(
                        db = volume_db,
                        source = "airplay",
                        "AirPlay volume update (mute)"
                    );
                } else if (volume_db - 0.0).abs() < 1e-6 {
                    tracing::info!(
                        db = volume_db,
                        source = "airplay",
                        "AirPlay volume update (maximum)"
                    );
                } else {
                    tracing::info!(db = volume_db, source = "airplay", "AirPlay volume update");
                }
            }
            VolumeSyncMode::Player => {
                let linear = airplay_db_to_linear(volume_db);
                self.set_volume(linear);
                tracing::info!(
                    volume_db,
                    linear,
                    source = "airplay",
                    "AirPlay volume applied to GStreamer pipeline"
                );
            }
        }
    }

    fn on_mute(&self, muted: bool) {
        match self.volume_sync {
            VolumeSyncMode::Disabled => {
                tracing::debug!(muted, "AirPlay mute ignored (volume_sync=disabled)");
            }
            VolumeSyncMode::System => {
                if let Err(e) = self.system_volume.set_muted(muted) {
                    tracing::warn!(
                        error = %e,
                        muted,
                        "system mute update failed (playback continues)"
                    );
                } else {
                    tracing::info!(muted, source = "airplay", "AirPlay mute update");
                }
                // Keep pipeline unity while OS mute owns silence.
                if !muted {
                    self.set_volume(1.0);
                }
            }
            VolumeSyncMode::Player => {
                if muted {
                    self.set_volume(0.0);
                } else {
                    let db =
                        self.airplay_volume_db_milli.load(Ordering::Relaxed) as f64 / 1000.0;
                    self.set_volume(airplay_db_to_linear(db));
                }
                tracing::info!(muted, source = "airplay", "AirPlay mute applied to pipeline");
            }
        }
    }

    fn volume(&self) -> Option<f64> {
        Some(self.airplay_volume_db_milli.load(Ordering::Relaxed) as f64 / 1000.0)
    }

    fn on_media_playlist(&self, playlist_uri: &str) {
        // Never log full URI query strings (may contain tokens).
        let safe = playlist_uri.split('?').next().unwrap_or(playlist_uri);
        tracing::info!(uri_path = %safe, "starting GStreamer media playbin (HLS)");

        // Mirror appsrc path must not feed media URLs; stop mirror video first.
        if let Ok(p) = self.h264_pipeline.lock() {
            let _ = p.set_state(gst::State::Null);
        }

        let pipeline = match build_media_playbin(playlist_uri) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "media playbin construction failed");
                return;
            }
        };
        install_bus_watch(&pipeline, "media", 0);
        apply_window_title(&pipeline, WINDOW_TITLE);

        // Apply current software volume to the new media pipeline.
        let vol = milli_to_volume(self.volume_milli.load(Ordering::Relaxed));
        if pipeline.find_property("volume").is_some() {
            pipeline.set_property("volume", vol);
        }
        if pipeline.find_property("mute").is_some() {
            pipeline.set_property("mute", vol <= 0.000_1);
        }

        if let Err(e) = pipeline.set_state(gst::State::Playing) {
            tracing::error!(error = %e, "media playbin PLAYING failed");
            let _ = pipeline.set_state(gst::State::Null);
            return;
        }

        if let Ok(mut slot) = self.hls_pipeline.lock() {
            if let Some(old) = slot.take() {
                let _ = old.set_state(gst::State::Null);
            }
            *slot = Some(pipeline);
        }
        tracing::info!("media playbin started (session-local; receiver remains available)");
    }

    fn on_media_playlist_remove(&self) {
        tracing::info!("stopping media playbin");
        if let Ok(mut slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.take() {
                let _ = p.set_state(gst::State::Null);
            }
        }
    }

    fn on_media_playlist_pause(&self) {
        if let Ok(slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.as_ref() {
                if let Err(e) = p.set_state(gst::State::Paused) {
                    tracing::warn!(error = %e, "media pause failed");
                } else {
                    tracing::info!("media paused");
                }
            }
        }
    }

    fn on_media_playlist_resume(&self) {
        if let Ok(slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.as_ref() {
                if let Err(e) = p.set_state(gst::State::Playing) {
                    tracing::warn!(error = %e, "media resume failed");
                } else {
                    tracing::info!("media resumed");
                }
            }
        }
    }

    fn on_media_playlist_seek(&self, position_seconds: f64) {
        if position_seconds < 0.0 || !position_seconds.is_finite() {
            return;
        }
        let Ok(slot) = self.hls_pipeline.lock() else {
            return;
        };
        let Some(p) = slot.as_ref() else {
            return;
        };
        let nanos = (position_seconds * 1_000_000_000.0).round() as u64;
        let clock = gst::ClockTime::from_nseconds(nanos);
        match p.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, clock) {
            Ok(()) => tracing::info!(position_seconds, "media seek"),
            Err(e) => tracing::warn!(position_seconds, error = %e, "media seek failed"),
        }
    }

    fn on_media_playlist_seek_fraction(&self, fraction: f64) {
        if !(0.0..=1.0).contains(&fraction) || !fraction.is_finite() {
            return;
        }
        let Ok(slot) = self.hls_pipeline.lock() else {
            return;
        };
        let Some(p) = slot.as_ref() else {
            return;
        };
        let Some(duration) = p.query_duration::<gst::ClockTime>() else {
            tracing::debug!(fraction, "seek fraction deferred (duration unknown)");
            return;
        };
        let target = duration.nseconds() as f64 * fraction;
        let clock = gst::ClockTime::from_nseconds(target.round() as u64);
        match p.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, clock) {
            Ok(()) => tracing::info!(fraction, "media seek fraction"),
            Err(e) => tracing::warn!(fraction, error = %e, "media seek fraction failed"),
        }
    }

    fn on_media_error(&self, message: &str) {
        tracing::warn!(
            %message,
            "media error (receiver stays alive; mirroring/control unaffected)"
        );
        // Tear down media pipeline only — do not exit process or close control.
        if let Ok(mut slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.take() {
                let _ = p.set_state(gst::State::Null);
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
                return PlaybackInfo { duration, position };
            }
        }
        PlaybackInfo {
            duration: 0.0,
            position: 0.0,
        }
    }
}

/// Build a media URI player (`playbin3` preferred, `playbin` fallback).
///
/// Sets `uri` as a property — never via pipeline-string interpolation.
fn build_media_playbin(uri: &str) -> Result<gst::Pipeline, String> {
    let element = gst::ElementFactory::make("playbin3")
        .build()
        .or_else(|_| gst::ElementFactory::make("playbin").build())
        .map_err(|e| {
            format!(
                "playbin3/playbin unavailable (install gstreamer playback plugins): {e}"
            )
        })?;
    element.set_property("uri", uri);
    element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "playbin is not a Pipeline".to_string())
}

impl Drop for GStreamerPlayer {
    fn drop(&mut self) {
        // Process-level teardown only — never called for session TEARDOWN.
        let pipes: Vec<gst::Pipeline> = [
            &self.h264_pipeline,
            &self.alac_pipeline,
            &self.aac_eld_pipeline,
        ]
        .iter()
        .filter_map(|pipe| pipe.lock().ok().map(|p| p.clone()))
        .collect();
        for p in pipes {
            let _ = p.set_state(gst::State::Null);
        }
        if let Ok(mut slot) = self.hls_pipeline.lock() {
            if let Some(p) = slot.take() {
                let _ = p.set_state(gst::State::Null);
            }
        }
        self.main_loop_quit.store(true, Ordering::Relaxed);
        if let Some(handle) = self._main_loop_thread.take() {
            // Bounded join: don't hang process exit if the loop is stuck.
            let _ = handle.join();
        }
        // volume keys thread may block on stdin; detach by not joining long.
        if let Some(handle) = self._volume_keys_thread.take() {
            // Don't join forever if blocked on stdin.
            let _ = handle;
        }
        let _ = self.volume_before_mute;
        tracing::info!("GStreamer player dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DecoderChoice, PreviewMode, SinkChoice};

    #[test]
    fn airplay_db_maps_to_linear_gstreamer_gain() {
        // 0.0 dB is maximum, never mute.
        assert_eq!(airplay_db_to_linear(0.0), 1.0);
        assert!(!crate::airplay_db_is_mute(0.0));
        assert!((airplay_db_to_linear(-20.0) - 0.1).abs() < 1e-12);
        assert_eq!(airplay_db_to_linear(-144.0), 0.0);
        assert!(crate::airplay_db_is_mute(-144.0));
        assert!(!crate::airplay_db_is_mute(-30.0));
        assert_eq!(airplay_db_to_linear(-200.0), 0.0);
        assert_eq!(airplay_db_to_linear(6.0), 1.0);
    }

    #[test]
    #[ignore = "requires installed GStreamer runtime"]
    fn system_volume_mode_keeps_pipeline_unity_and_forwards_db() {
        use crate::system_volume::RecordingSystemVolumeController;
        use airplay_server::AirPlayConsumer;
        use std::sync::Arc;

        let rec = Arc::new(RecordingSystemVolumeController::new());
        let player = GStreamerPlayer::with_volume_sync(
            1.0,
            PreviewMode::Balanced,
            false,
            VolumeSyncMode::System,
            Some(rec.clone()),
        )
        .expect("gstreamer player");

        // Maximum
        player.on_volume(0.0);
        // Intermediate
        player.on_volume(-20.5);
        // Mute sentinel
        player.on_volume(-144.0);
        player.on_mute(true);
        player.on_mute(false);

        // Pipeline stays near unity in system mode (set_volume(1.0) on each update).
        assert!((player.volume_milli.load(Ordering::Relaxed) as f64 / 1000.0 - 1.0).abs() < 1e-6);

        let vols = rec.volumes();
        assert!(vols.contains(&0.0));
        assert!(vols.iter().any(|v| (*v + 20.5).abs() < 1e-9));
        assert!(vols.iter().any(|v| *v <= -100.0));
        assert_eq!(rec.mutes(), vec![true, false]);
    }

    #[test]
    #[ignore = "requires installed GStreamer runtime"]
    fn system_volume_failure_does_not_panic() {
        use crate::system_volume::RecordingSystemVolumeController;
        use airplay_server::AirPlayConsumer;
        use std::sync::Arc;

        let rec = Arc::new(RecordingSystemVolumeController::new());
        rec.fail.store(true, Ordering::Relaxed);
        let player = GStreamerPlayer::with_volume_sync(
            1.0,
            PreviewMode::Balanced,
            false,
            VolumeSyncMode::System,
            Some(rec),
        )
        .expect("gstreamer player");
        // Must not panic / terminate.
        player.on_volume(-12.0);
        player.on_mute(true);
    }

    #[test]
    fn direct_pipeline_has_required_caps_and_no_processing_or_encoding() {
        let text = video_pipeline_description(
            DecoderChoice::D3d11H264Dec,
            SinkChoice::D3d11VideoSink,
            PreviewMode::Balanced.options(),
        );
        for required in [
            "block=false",
            "max-buffers=3",
            "max-bytes=4194304",
            "leaky-type=downstream",
            "stream-format=byte-stream",
            "alignment=au",
            "max-size-buffers=3",
            "leaky=downstream",
            "h264parse config-interval=-1",
            "d3d11h264dec",
            "d3d11videosink",
            "force-aspect-ratio=true",
            "sync=true",
        ] {
            assert!(text.contains(required), "missing {required}: {text}");
        }
        for forbidden in [
            "x264enc",
            "openh264enc",
            "jpeg",
            "png",
            "videoscale",
            "videocrop",
            "videoflip",
            "BGRx",
        ] {
            assert!(!text.contains(forbidden), "forbidden {forbidden}: {text}");
        }
    }

    #[test]
    fn quality_pipeline_bounds_nonblocking_appsrc_without_leaking_downstream_queue() {
        let text = video_pipeline_description(
            DecoderChoice::AvdecH264,
            SinkChoice::AutoVideoSink,
            PreviewMode::Quality.options(),
        );
        assert!(text.contains("block=false"));
        assert!(text.contains("max-buffers=8"));
        assert!(text.contains("max-bytes=4194304"));
        assert!(text.contains("leaky-type=upstream"));
        assert!(text.contains("max-size-buffers=8"));
        assert!(text.contains("leaky=no"));
    }

    #[test]
    fn generation_gate_blocks_stale_video_stop() {
        let gate = GenerationGate::new();
        gate.claim(10);
        assert!(gate.stop_if_owner(10));
        gate.claim(11);
        assert!(!gate.stop_if_owner(10));
    }

    #[test]
    fn player_lifecycle_stop_is_single_flight() {
        let life = PlayerLifecycle::new();
        life.mark_playing();
        assert!(life.begin_stop());
        assert!(!life.begin_stop());
        life.mark_stopped();
        assert!(!life.begin_stop());
        life.reset_for_start();
        life.mark_playing();
        assert!(life.begin_stop());
    }

    #[test]
    #[ignore = "requires installed GStreamer playback plugins"]
    fn media_playbin_sets_uri_as_property() {
        gst::init().unwrap();
        let uri = "http://127.0.0.1:7000/playlist/master.m3u8";
        let playbin = build_media_playbin(uri).unwrap();
        assert_eq!(playbin.property::<String>("uri"), uri);
        playbin.set_state(gst::State::Null).unwrap();
    }

    #[test]
    #[ignore = "requires an installed GStreamer runtime and Windows video plugins"]
    fn installed_windows_pipeline_preflights() {
        gst::init().unwrap();
        let (pipeline, _src, decoder, sink, _overruns, _saturation_events) =
            build_video_pipeline(PreviewMode::Balanced, true).unwrap();
        assert_eq!(decoder, DecoderChoice::D3d11H264Dec);
        assert_eq!(sink, SinkChoice::D3d11VideoSink);
        let (transition, current, _) = pipeline.state(gst::ClockTime::ZERO);
        assert!(transition.is_ok());
        assert_eq!(current, gst::State::Ready);
        pipeline.set_state(gst::State::Null).unwrap();
    }
}
