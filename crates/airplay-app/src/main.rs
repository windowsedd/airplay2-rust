//! Runnable AirPlay receiver binary.
//!
//! Loads TOML config, starts `AirPlayServer` with the selected player backend,
//! and shuts down cleanly on Ctrl+C.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use airplay_player::H264Dump;
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
    "h264-dump".into()
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
                     Starts an AirPlay 2 receiver. Default player writes raw H.264 to dump.h264.\n\
                     Config: TOML with [airplay] and [player] sections (see config.example.toml)."
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

    let implementation = cfg.player.implementation.to_ascii_lowercase();
    let consumer: Arc<dyn airplay_server::AirPlayConsumer> = match implementation.as_str() {
        "h264-dump" | "h264_dump" | "dump" => {
            let dump = H264Dump::new(&cfg.player.output)
                .with_context(|| format!("open H.264 dump {}", cfg.player.output))?;
            tracing::info!(
                output = %cfg.player.output,
                "player: h264-dump"
            );
            Arc::new(dump)
        }
        other => {
            bail!(
                "unsupported player implementation '{other}' (supported: h264-dump)"
            );
        }
    };

    let mut server = AirPlayServer::new(server_cfg, consumer);
    server.start().await.context("start AirPlay server")?;

    tracing::info!(
        name = %cfg.airplay.server_name,
        port = server.port(),
        "AirPlay receiver running — press Ctrl+C to stop"
    );

    tokio::signal::ctrl_c()
        .await
        .context("wait for Ctrl+C")?;

    tracing::info!("Ctrl+C received; shutting down");
    server.stop().await;
    Ok(())
}
