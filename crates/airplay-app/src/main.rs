//! Runnable AirPlay receiver binary.
//!
//! Loads TOML config, starts `AirPlayServer` with the selected player backend,
//! and shuts down cleanly on Ctrl+C.
//!
//! Player backends are selected by `player.implementation` and must be compiled
//! in via Cargo features (`h264-dump`, `gstreamer`, `ffmpeg`, `vlc`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use airplay_server::{AirPlayConfig, AirPlayServer};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Deserialize)]
struct AppConfig {
    #[serde(default)]
    airplay: AirplaySection,
    #[serde(default)]
    player: PlayerSection,
}

#[derive(Debug, Deserialize)]
struct AirplaySection {
    #[serde(default = "default_server_name")]
    server_name: String,
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "default_fps")]
    fps: u32,
}

impl Default for AirplaySection {
    fn default() -> Self {
        Self {
            server_name: default_server_name(),
            width: default_width(),
            height: default_height(),
            fps: default_fps(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PlayerSection {
    #[serde(default = "default_implementation")]
    implementation: String,
    #[serde(default = "default_output")]
    output: String,
}

impl Default for PlayerSection {
    fn default() -> Self {
        Self {
            implementation: default_implementation(),
            output: default_output(),
        }
    }
}

fn default_server_name() -> String {
    "airplay2-rust".into()
}
fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}
fn default_fps() -> u32 {
    24
}
fn default_implementation() -> String {
    // "auto" = dump.h264 + ffplay window + GStreamer window (whatever is built in).
    "auto".into()
}

fn resolve_implementation(configured: &str) -> String {
    let key = configured.trim().to_ascii_lowercase();
    match key.as_str() {
        "gst" => "gstreamer".into(),
        "ffplay" => "ffmpeg".into(),
        "h264_dump" | "dump" => "h264-dump".into(),
        other => other.to_string(),
    }
}
fn default_output() -> String {
    "dump.h264".into()
}

fn load_config(path: Option<&Path>) -> Result<AppConfig> {
    let candidates: Vec<PathBuf> = if let Some(p) = path {
        vec![p.to_path_buf()]
    } else {
        vec![
            PathBuf::from("config.toml"),
            PathBuf::from("crates/airplay-app/config.example.toml"),
            PathBuf::from("config.example.toml"),
        ]
    };

    for candidate in &candidates {
        if candidate.is_file() {
            let text = std::fs::read_to_string(candidate)
                .with_context(|| format!("read config {}", candidate.display()))?;
            let cfg: AppConfig = toml::from_str(&text)
                .with_context(|| format!("parse config {}", candidate.display()))?;
            tracing::info!(path = %candidate.display(), "loaded config");
            return Ok(cfg);
        }
    }

    if let Some(p) = path {
        bail!("config file not found: {}", p.display());
    }

    tracing::warn!("no config.toml found; using defaults (h264-dump → dump.h264)");
    Ok(AppConfig {
        airplay: AirplaySection::default(),
        player: PlayerSection::default(),
    })
}

fn parse_args() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                return args.next().map(PathBuf::from);
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: airplay-app [--config <path>]\n\n\
                     Starts an AirPlay 2 receiver.\n\
                     Config: TOML with [airplay] and [player] sections (see config.example.toml).\n\n\
                     Players (must be enabled at build time):\n\
                       h264-dump  — write raw H.264 to a file (feature h264-dump, default)\n\
                       gstreamer  — live window via GStreamer 1.x (feature gstreamer)\n\
                       ffmpeg     — pipe H.264 to ffplay (feature ffmpeg)\n\
                       vlc        — best-effort VLC stdin (feature vlc)"
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                eprintln!("unknown option: {other} (try --help)");
                std::process::exit(2);
            }
            other => {
                // bare path as config
                return Some(PathBuf::from(other));
            }
        }
    }
    None
}

fn missing_feature_msg(name: &str, feature: &str) -> String {
    format!(
        "player implementation '{name}' requires building with --features {feature}\n\
         Example: cargo run -p airplay-app --features {feature}\n\
         See README for install notes (GStreamer / FFmpeg / VLC)."
    )
}

fn build_auto_consumer(output: &str) -> Result<Arc<dyn airplay_server::AirPlayConsumer>> {
    use airplay_server::AirPlayConsumer;

    let mut parts: Vec<Box<dyn AirPlayConsumer>> = Vec::new();
    let mut labels: Vec<&'static str> = Vec::new();

    #[cfg(feature = "h264-dump")]
    {
        let dump = airplay_player::H264Dump::new(output)
            .with_context(|| format!("open H.264 dump {output}"))?;
        parts.push(Box::new(dump));
        labels.push("h264-dump");
    }

    // ffplay is the most reliable *window* on Windows.
    #[cfg(feature = "ffmpeg")]
    {
        match airplay_player::FFmpegPlayer::new() {
            Ok(p) => {
                parts.push(Box::new(p));
                labels.push("ffmpeg/ffplay");
            }
            Err(e) => tracing::warn!(error = %e, "ffplay unavailable; no FFmpeg window"),
        }
    }

    #[cfg(feature = "gstreamer")]
    {
        match airplay_player::GStreamerPlayer::new() {
            Ok(p) => {
                parts.push(Box::new(p));
                labels.push("gstreamer");
            }
            Err(e) => tracing::warn!(error = %e, "GStreamer unavailable; no GStreamer window"),
        }
    }

    if parts.is_empty() {
        bail!(
            "auto player: no backends available. Build with default features \
             (h264-dump,gstreamer,ffmpeg) and install GStreamer + ffplay."
        );
    }

    tracing::info!(
        backends = %labels.join(" + "),
        "player: auto (tee) — look for window title 'airplay2-rust'"
    );
    Ok(Arc::new(airplay_player::TeePlayer::new(parts)))
}

fn build_consumer(
    implementation: &str,
    output: &str,
) -> Result<Arc<dyn airplay_server::AirPlayConsumer>> {
    let impl_key = implementation.to_ascii_lowercase();
    match impl_key.as_str() {
        "auto" | "default" | "mirror" => build_auto_consumer(output),
        "h264-dump" | "h264_dump" | "dump" => {
            #[cfg(feature = "h264-dump")]
            {
                let dump = airplay_player::H264Dump::new(output)
                    .with_context(|| format!("open H.264 dump {output}"))?;
                tracing::info!(output = %output, "player: h264-dump");
                Ok(Arc::new(dump))
            }
            #[cfg(not(feature = "h264-dump"))]
            {
                bail!("{}", missing_feature_msg(implementation, "h264-dump"));
            }
        }
        "gstreamer" | "gst" => {
            #[cfg(feature = "gstreamer")]
            {
                let player = airplay_player::GStreamerPlayer::new()
                    .map_err(|e| anyhow::anyhow!("GStreamer player: {e}"))?;
                tracing::info!("player: gstreamer");
                Ok(Arc::new(player))
            }
            #[cfg(not(feature = "gstreamer"))]
            {
                bail!("{}", missing_feature_msg(implementation, "gstreamer"));
            }
        }
        "ffmpeg" | "ffplay" => {
            #[cfg(feature = "ffmpeg")]
            {
                let player = airplay_player::FFmpegPlayer::new()
                    .map_err(|e| anyhow::anyhow!("FFmpeg player: {e}"))?;
                tracing::info!("player: ffmpeg (ffplay)");
                Ok(Arc::new(player))
            }
            #[cfg(not(feature = "ffmpeg"))]
            {
                bail!("{}", missing_feature_msg(implementation, "ffmpeg"));
            }
        }
        "vlc" => {
            #[cfg(feature = "vlc")]
            {
                let player = airplay_player::VlcPlayer::new()
                    .map_err(|e| anyhow::anyhow!("VLC player: {e}"))?;
                tracing::info!("player: vlc (best-effort)");
                Ok(Arc::new(player))
            }
            #[cfg(not(feature = "vlc"))]
            {
                bail!("{}", missing_feature_msg(implementation, "vlc"));
            }
        }
        other => {
            bail!(
                "unsupported player implementation '{other}' \
                 (supported: auto, h264-dump, gstreamer, ffmpeg, vlc)"
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config_path = parse_args();
    let cfg = load_config(config_path.as_deref())?;

    let server_cfg = AirPlayConfig {
        server_name: cfg.airplay.server_name.clone(),
        width: cfg.airplay.width,
        height: cfg.airplay.height,
        fps: cfg.airplay.fps,
    };

    let implementation = resolve_implementation(&cfg.player.implementation);
    let consumer = build_consumer(&implementation, &cfg.player.output).with_context(|| {
        format!(
            "failed to start player '{implementation}'. \
             For a live window you need GStreamer 1.x on PATH (see README). \
             Or run: cargo run -p airplay-app --no-default-features --features h264-dump"
        )
    })?;

    let mut server = AirPlayServer::new(server_cfg, consumer);
    server.start().await.context("start AirPlay server")?;

    tracing::info!(
        name = %cfg.airplay.server_name,
        port = server.port(),
        player = %implementation,
        "AirPlay receiver running — Screen Mirror to this name; GStreamer opens a video window"
    );

    tokio::signal::ctrl_c()
        .await
        .context("wait for Ctrl+C")?;

    tracing::info!("Ctrl+C received; shutting down");
    server.stop().await;
    Ok(())
}
