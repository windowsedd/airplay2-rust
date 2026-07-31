//! FFmpeg / ffplay subprocess player for live H.264 mirror video.
//!
//! Spawns `ffplay` with stdin as annex-B H.264 (`-f h264 -i -`). Requires
//! `ffplay` on `PATH` and Cargo feature `ffmpeg`. Audio is not played.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::AirPlayConsumer;

/// Pipes decrypted H.264 to an `ffplay` child process.
pub struct FFmpegPlayer {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
}

impl FFmpegPlayer {
    /// Create a player. Does not spawn `ffplay` until the first video format event.
    ///
    /// Checks that `ffplay` is resolvable on `PATH` so misconfiguration fails early.
    pub fn new() -> Result<Self, String> {
        which_ffplay()?;
        Ok(Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
        })
    }

    fn spawn_ffplay() -> Result<(Child, ChildStdin), String> {
        let mut child = Command::new("ffplay")
            .args([
                "-fflags",
                "nobuffer",
                "-flags",
                "low_delay",
                "-framedrop",
                "-f",
                "h264",
                "-codec:v",
                "h264",
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
                "-vf",
                "setpts=0",
                "-i",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                format!(
                    "failed to spawn ffplay (is FFmpeg/ffplay on PATH?): {e}"
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ffplay stdin not piped".to_string())?;
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
        let (child, stdin) = Self::spawn_ffplay()?;
        *child_guard = Some(child);
        let mut stdin_guard = self
            .stdin
            .lock()
            .map_err(|e| format!("stdin mutex poisoned: {e}"))?;
        *stdin_guard = Some(stdin);
        tracing::info!("ffplay started (H.264 annex-B on stdin)");
        Ok(())
    }

    fn stop_process(&self) {
        if let Ok(mut stdin) = self.stdin.lock() {
            *stdin = None; // drop closes pipe
        }
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
                tracing::info!("ffplay stopped");
            }
        }
    }
}

fn which_ffplay() -> Result<(), String> {
    match Command::new("ffplay").arg("-version").output() {
        Ok(out) if out.status.success() || out.status.code() == Some(0) => Ok(()),
        // Some builds print version to stderr and exit 0; accept any spawn that runs.
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "ffplay not found on PATH (install FFmpeg and ensure ffplay is available): {e}"
        )),
    }
}

impl Default for FFmpegPlayer {
    fn default() -> Self {
        Self::new().expect("ffplay on PATH")
    }
}

impl AirPlayConsumer for FFmpegPlayer {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            "video format; ensuring ffplay process"
        );
        if let Err(e) = self.ensure_started() {
            tracing::error!(error = %e, "could not start ffplay");
        }
    }

    fn on_video(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Err(e) = self.ensure_started() {
            tracing::error!(error = %e, "could not start ffplay for video push");
            return;
        }
        match self.stdin.lock() {
            Ok(mut guard) => {
                if let Some(stdin) = guard.as_mut() {
                    if let Err(e) = stdin.write_all(data).and_then(|_| stdin.flush()) {
                        tracing::warn!(error = %e, "failed to write H.264 to ffplay stdin");
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "ffplay stdin mutex poisoned"),
        }
    }

    fn on_video_src_disconnect(&self) {
        tracing::info!("video source disconnected; stopping ffplay");
        self.stop_process();
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        tracing::info!(?info, "audio format (ignored by ffmpeg/ffplay video-only backend)");
    }

    fn on_audio(&self, _data: &[u8]) {
        // Video-only backend; audio not piped to ffplay.
    }

    fn on_audio_src_disconnect(&self) {
        tracing::debug!("audio source disconnected (ffmpeg backend ignores audio)");
    }
}

impl Drop for FFmpegPlayer {
    fn drop(&mut self) {
        self.stop_process();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_requires_ffplay_or_errors_clearly() {
        // On CI/dev machines with FFmpeg installed this succeeds; without it, error mentions PATH.
        match FFmpegPlayer::new() {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    e.contains("ffplay") || e.contains("PATH"),
                    "unexpected error: {e}"
                );
            }
        }
    }
}
