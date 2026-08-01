//! Runnable AirPlay receiver binary.
//!
//! Loads TOML config, starts `AirPlayServer` with the selected player backend,
//! system tray menu (Exit / Open config / dump folder / Status), and shuts down
//! cleanly on Ctrl+C or tray Exit.
//!
//! Player backends are selected by `player.implementation` and must be compiled
//! in via Cargo features (`h264-dump`, `gstreamer`, `ffmpeg`, `vlc`).

mod tray;

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
    /// Advertised maxFPS to the iPhone (cap, not a hard lock).
    #[serde(default = "default_fps", alias = "max_fps", alias = "maxFPS")]
    fps: u32,
    /// Advertised display refreshRate (usually 60).
    #[serde(default = "default_refresh_rate", alias = "refreshRate")]
    refresh_rate: u32,
}

impl Default for AirplaySection {
    fn default() -> Self {
        Self {
            server_name: default_server_name(),
            width: default_width(),
            height: default_height(),
            fps: default_fps(),
            refresh_rate: default_refresh_rate(),
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
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_fps() -> u32 {
    60
}
fn default_refresh_rate() -> u32 {
    60
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

fn default_config() -> AppConfig {
    AppConfig {
        airplay: AirplaySection::default(),
        player: PlayerSection {
            implementation: "auto".into(),
            output: "dump.h264".into(),
        },
    }
}

/// Returns `(config, path of config.toml used or written)`.
fn load_config(path: Option<&Path>) -> Result<(AppConfig, PathBuf)> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = path {
        candidates.push(p.to_path_buf());
    } else {
        candidates.push(PathBuf::from("config.toml"));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("config.toml"));
            }
        }
        candidates.push(PathBuf::from("crates/airplay-app/config.example.toml"));
        candidates.push(PathBuf::from("config.example.toml"));
    }

    for candidate in &candidates {
        if candidate.is_file() {
            let text = std::fs::read_to_string(candidate)
                .with_context(|| format!("read config {}", candidate.display()))?;
            let mut cfg: AppConfig = toml::from_str(&text)
                .with_context(|| format!("parse config {}", candidate.display()))?;
            let impl_key = cfg.player.implementation.to_ascii_lowercase();
            if matches!(impl_key.as_str(), "h264-dump" | "h264_dump" | "dump") {
                tracing::warn!(
                    path = %candidate.display(),
                    "config player is h264-dump (file only); switching to auto for a live window. \
                     Set AIRPLAY_FORCE_DUMP=1 to keep dump-only."
                );
                if std::env::var_os("AIRPLAY_FORCE_DUMP").is_none() {
                    cfg.player.implementation = "auto".into();
                }
            }
            tracing::info!(
                path = %candidate.display(),
                player = %cfg.player.implementation,
                "loaded config"
            );
            return Ok((cfg, candidate.clone()));
        }
    }

    if let Some(p) = path {
        bail!("config file not found: {}", p.display());
    }

    let starter = concat!(
        "[airplay]\n",
        "server_name = \"airplay2-rust\"\n",
        "width = 1920\n",
        "height = 1080\n",
        "fps = 60\n",
        "refresh_rate = 60\n",
        "\n",
        "[player]\n",
        "implementation = \"auto\"\n",
        "output = \"dump.h264\"\n",
    );
    let mut used = PathBuf::from("config.toml");
    for path in [
        PathBuf::from("config.toml"),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("config.toml")))
            .unwrap_or_else(|| PathBuf::from("config.toml")),
    ] {
        if path.is_file() {
            used = path;
            break;
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                continue;
            }
        }
        if std::fs::write(&path, starter).is_ok() {
            tracing::info!(path = %path.display(), "wrote config.toml (player=auto)");
            used = path;
            break;
        }
    }
    tracing::info!("defaults: player=auto");
    Ok((default_config(), used))
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
                     Default player: auto (dump.h264 + ffplay window).\n\n\
                     Build / run:\n\
                       cargo run\n\
                       cargo build --release\n\
                       .\\target\\release\\airplay-app.exe\n\n\
                     Players (compile-time features):\n\
                       auto       - dump + ffplay (+ gstreamer if built with feature)\n\
                       h264-dump  - file only\n\
                       ffmpeg     - ffplay window\n\
                       gstreamer  - needs --features gstreamer + pkg-config on Windows\n\
                       vlc        - needs --features vlc"
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

    // ffplay live window (video only). Needs real ffplay.exe on Windows.
    #[cfg(feature = "ffmpeg")]
    {
        match airplay_player::FFmpegPlayer::new() {
            Ok(p) => {
                // File-mode fallback opens this path if stdin spawn fails.
                p.set_dump_path(output);
                parts.push(Box::new(p));
                labels.push("ffmpeg/ffplay");
                tracing::info!(
                    dump = %output,
                    "ffplay backend ready — window 'airplay2-rust' opens after first SPS/frames"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "ffplay unavailable; NO live video window. Install FFmpeg (choco install ffmpeg). \
                     dump.h264 will still be written if h264-dump is enabled."
                );
            }
        }
    }
    #[cfg(not(feature = "ffmpeg"))]
    {
        tracing::error!(
            "binary built without --features ffmpeg; NO live window. Rebuild with default features."
        );
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

    let config_path_arg = parse_args();
    let (cfg, config_path) = load_config(config_path_arg.as_deref())?;

    let fps = cfg.airplay.fps.clamp(1, 120);
    let refresh_rate = cfg.airplay.refresh_rate.clamp(1, 240);
    if cfg.airplay.fps != fps {
        tracing::warn!(requested = cfg.airplay.fps, using = fps, "fps clamped to 1..=120");
    }

    let server_cfg = AirPlayConfig {
        server_name: cfg.airplay.server_name.clone(),
        width: cfg.airplay.width,
        height: cfg.airplay.height,
        fps,
        refresh_rate,
    };

    tracing::info!(
        name = %server_cfg.server_name,
        width = server_cfg.width,
        height = server_cfg.height,
        max_fps = server_cfg.fps,
        refresh_rate = server_cfg.refresh_rate,
        "advertising display to iPhone (resolution drives quality; bitrate is chosen by the phone)"
    );

    let implementation = resolve_implementation(&cfg.player.implementation);
    let dump_path = PathBuf::from(&cfg.player.output);
    let consumer = build_consumer(&implementation, &cfg.player.output).with_context(|| {
        format!(
            "failed to start player '{implementation}'. \
             For a live window you need GStreamer 1.x on PATH (see README). \
             Or run: cargo run -p airplay-app --no-default-features --features h264-dump"
        )
    })?;

    let mut server = AirPlayServer::new(server_cfg, consumer);
    server.start().await.context("start AirPlay server")?;

    let port = server.port();
    tracing::info!(
        name = %cfg.airplay.server_name,
        port,
        player = %implementation,
        max_fps = fps,
        "AirPlay receiver running — tray icon available; Ctrl+C or tray Exit to stop"
    );

    let tray_info = tray::TrayInfo {
        server_name: cfg.airplay.server_name.clone(),
        port,
        player: implementation.clone(),
        width: cfg.airplay.width,
        height: cfg.airplay.height,
        fps,
        config_path: config_path.clone(),
        dump_path: dump_path.clone(),
    };

    // System tray (taskbar). Soft-fail if tray cannot start (headless / no GUI).
    let tray_rx = match tray::spawn_tray(tray_info.clone()) {
        Ok((rx, _handle)) => Some(rx),
        Err(e) => {
            tracing::warn!(error = %e, "system tray unavailable; use Ctrl+C to exit");
            None
        }
    };

    // Wait for Ctrl+C or tray Exit (poll tray so we can open config without blocking).
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl+C received; shutting down");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(150)) => {
                if let Some(rx) = tray_rx.as_ref() {
                    while let Ok(cmd) = rx.try_recv() {
                        match cmd {
                            tray::TrayCommand::Quit => {
                                tracing::info!("tray Exit; shutting down");
                                server.stop().await;
                                return Ok(());
                            }
                            other => tray::handle_tray_command(other, &tray_info),
                        }
                    }
                }
            }
        }
    }

    server.stop().await;
    Ok(())
}
