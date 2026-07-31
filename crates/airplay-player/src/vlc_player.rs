//! Best-effort VLC subprocess player for live H.264 mirror video.
//!
//! Spawns `vlc` (or `cvlc`) reading annex-B H.264 from stdin. VLC's live stdin
//! demux is historically unstable; prefer GStreamer or FFmpeg for production.
//! Requires `vlc`/`cvlc` on `PATH` and Cargo feature `vlc`. Audio is not played.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::AirPlayConsumer;

/// Pipes decrypted H.264 to a VLC child process (best-effort).
pub struct VlcPlayer {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    binary: String,
}

impl VlcPlayer {
    /// Create a player. Does not spawn VLC until the first video format event.
    pub fn new() -> Result<Self, String> {
        let binary = which_vlc()?;
        Ok(Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            binary,
        })
    }

    fn spawn_vlc(&self) -> Result<(Child, ChildStdin), String> {
        // `--demux h264` forces annex-B; `-` is stdin. Quiet interface reduces noise.
        let mut child = Command::new(&self.binary)
            .args([
                "-I",
                "dummy",
                "--demux=h264",
                "--h264-fps=60",
                "--network-caching=0",
                "--file-caching=0",
                "--live-caching=0",
                "--sout-mux-caching=0",
                "--no-audio",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("failed to spawn {} (is VLC on PATH?): {e}", self.binary))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "vlc stdin not piped".to_string())?;
        Ok((child, stdin))
    }

    fn ensure_started(&self) -> Result<(), String> {
        let mut child_guard = self
            .child
            .lock()
            .map_err(|e| format!("child mutex poisoned: {e}"))?;
        if child_guard.is_some() {
            return Ok(());
        }
        let (child, stdin) = self.spawn_vlc()?;
        *child_guard = Some(child);
        let mut stdin_guard = self
            .stdin
            .lock()
            .map_err(|e| format!("stdin mutex poisoned: {e}"))?;
        *stdin_guard = Some(stdin);
        tracing::info!(binary = %self.binary, "VLC started (H.264 annex-B on stdin; best-effort)");
        Ok(())
    }

    fn stop_process(&self) {
        if let Ok(mut stdin) = self.stdin.lock() {
            *stdin = None;
        }
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
                tracing::info!("VLC stopped");
            }
        }
    }
}

fn which_vlc() -> Result<String, String> {
    for name in ["vlc", "cvlc"] {
        match Command::new(name).arg("--version").output() {
            Ok(_) => return Ok(name.to_string()),
            Err(_) => continue,
        }
    }
    Err(
        "vlc/cvlc not found on PATH (install VLC and ensure the CLI is available). \
         Note: VLC live stdin playback is best-effort and may be unstable."
            .into(),
    )
}

impl Default for VlcPlayer {
    fn default() -> Self {
        Self::new().expect("vlc on PATH")
    }
}

impl AirPlayConsumer for VlcPlayer {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            "video format; ensuring VLC process"
        );
        if let Err(e) = self.ensure_started() {
            tracing::error!(error = %e, "could not start VLC");
        }
    }

    fn on_video(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Err(e) = self.ensure_started() {
            tracing::error!(error = %e, "could not start VLC for video push");
            return;
        }
        match self.stdin.lock() {
            Ok(mut guard) => {
                if let Some(stdin) = guard.as_mut() {
                    if let Err(e) = stdin.write_all(data).and_then(|_| stdin.flush()) {
                        tracing::warn!(error = %e, "failed to write H.264 to VLC stdin");
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "VLC stdin mutex poisoned"),
        }
    }

    fn on_video_src_disconnect(&self) {
        tracing::info!("video source disconnected; stopping VLC");
        self.stop_process();
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        tracing::info!(?info, "audio format (ignored by VLC video-only backend)");
    }

    fn on_audio(&self, _data: &[u8]) {
        // Video-only backend.
    }

    fn on_audio_src_disconnect(&self) {
        tracing::debug!("audio source disconnected (VLC backend ignores audio)");
    }
}

impl Drop for VlcPlayer {
    fn drop(&mut self) {
        self.stop_process();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_requires_vlc_or_errors_clearly() {
        match VlcPlayer::new() {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    e.contains("vlc") || e.contains("PATH"),
                    "unexpected error: {e}"
                );
            }
        }
    }
}
