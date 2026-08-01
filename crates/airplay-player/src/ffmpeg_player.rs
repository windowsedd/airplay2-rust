//! FFmpeg / ffplay subprocess player for live H.264 mirror video.
//!
//! Spawns `ffplay` with stdin as annex-B H.264. On Windows we force a new console
//! window group so the GUI is not swallowed by the cargo host, and we restart
//! ffplay if it dies. Requires `ffplay` on `PATH` and Cargo feature `ffmpeg`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::AirPlayConsumer;

/// Pipes decrypted H.264 to an `ffplay` child process (live window).
pub struct FFmpegPlayer {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    last_restart: Mutex<Instant>,
    frames: Mutex<u64>,
    /// Full path to ffplay if resolved (avoids broken Chocolatey shims).
    ffplay_path: PathBuf,
}

impl FFmpegPlayer {
    /// Create a player and **pre-start** ffplay so a window appears immediately.
    pub fn new() -> Result<Self, String> {
        let ffplay_path = resolve_ffplay()?;
        let player = Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            last_restart: Mutex::new(Instant::now() - Duration::from_secs(10)),
            frames: Mutex::new(0),
            ffplay_path,
        };
        // Open the window early (black until first NALs arrive).
        if let Err(e) = player.ensure_started() {
            tracing::warn!(error = %e, "pre-start ffplay failed; will retry on first frame");
        }
        Ok(player)
    }

    fn spawn_ffplay(&self) -> Result<(Child, ChildStdin), String> {
        // Try preferred path first, then plain "ffplay" on PATH.
        let candidates: Vec<PathBuf> = {
            let mut v = vec![self.ffplay_path.clone()];
            if self.ffplay_path.as_os_str() != "ffplay" {
                v.push(PathBuf::from("ffplay"));
            }
            v
        };

        let mut last_err = String::from("no ffplay candidate");
        for path in &candidates {
            match Self::spawn_one(path) {
                Ok(pair) => return Ok(pair),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "ffplay spawn attempt failed");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    fn spawn_one(path: &PathBuf) -> Result<(Child, ChildStdin), String> {
        let mut cmd = Command::new(path);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-window_title",
            "airplay2-rust",
            "-fflags",
            "nobuffer+discardcorrupt+genpts",
            "-flags",
            "low_delay",
            "-framedrop",
            "-f",
            "h264",
            "-probesize",
            "2000000",
            "-analyzeduration",
            "2000000",
            "-i",
            "pipe:0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

        // Do NOT use CREATE_BREAKAWAY_FROM_JOB — it returns Access Denied (os error 5)
        // when the parent is inside a Windows Job (cargo, VS Code, Terminal, etc.).

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn ffplay at {}: {e}", path.display()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ffplay stdin not piped".to_string())?;

        tracing::info!(
            path = %path.display(),
            "ffplay window started (title: airplay2-rust) — check taskbar"
        );
        Ok((child, stdin))
    }

    fn ensure_started(&self) -> Result<(), String> {
        // Restart if the previous process died.
        if let Ok(mut child_guard) = self.child.lock() {
            if let Some(child) = child_guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        tracing::warn!(
                            ?status,
                            "ffplay exited unexpectedly; will restart"
                        );
                        *child_guard = None;
                        if let Ok(mut s) = self.stdin.lock() {
                            *s = None;
                        }
                    }
                    Ok(None) => return Ok(()), // still running
                    Err(e) => tracing::warn!(error = %e, "ffplay try_wait failed"),
                }
            }
            if child_guard.is_some() {
                return Ok(());
            }
        }

        // Rate-limit restarts.
        if let Ok(mut t) = self.last_restart.lock() {
            if t.elapsed() < Duration::from_millis(500) {
                return Err("ffplay restart throttled".into());
            }
            *t = Instant::now();
        }

        let (child, stdin) = self.spawn_ffplay()?;
        if let Ok(mut g) = self.child.lock() {
            *g = Some(child);
        }
        if let Ok(mut g) = self.stdin.lock() {
            *g = Some(stdin);
        }
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

/// Prefer the real FFmpeg `ffplay.exe` over Chocolatey ShimGen (shims often break under Job objects).
fn resolve_ffplay() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        // 1) Chocolatey real tools path (not the shim in bin\)
        let real = PathBuf::from(r"C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin\ffplay.exe");
        if real.is_file() {
            tracing::info!(path = %real.display(), "using real ffplay.exe (not Chocolatey shim)");
            return Ok(real);
        }

        // 2) `where ffplay` — skip *\chocolatey\bin\* shims when a deeper tools path exists
        if let Ok(out) = Command::new("where").arg("ffplay").output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut shim: Option<PathBuf> = None;
                for line in text.lines() {
                    let p = PathBuf::from(line.trim());
                    if !p.is_file() {
                        continue;
                    }
                    let s = p.to_string_lossy().to_ascii_lowercase();
                    // Prefer non-shim paths
                    if s.contains(r"\chocolatey\bin\") {
                        shim = Some(p);
                        continue;
                    }
                    if s.ends_with("ffplay.exe") {
                        return Ok(p);
                    }
                }
                if let Some(p) = shim {
                    return Ok(p);
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(out) = Command::new("which").arg("ffplay").output() {
            if out.status.success() {
                let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }

    match Command::new("ffplay").arg("-version").output() {
        Ok(_) => Ok(PathBuf::from("ffplay")),
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

        let n = {
            let mut f = self.frames.lock().unwrap_or_else(|e| e.into_inner());
            *f += 1;
            *f
        };
        if n == 1 {
            tracing::info!(
                bytes = data.len(),
                "first H.264 annex-B chunk → ffplay (window title: airplay2-rust)"
            );
        } else if n % 120 == 0 {
            tracing::info!(n, bytes = data.len(), "ffplay push H.264");
        }

        match self.stdin.lock() {
            Ok(mut guard) => {
                if let Some(stdin) = guard.as_mut() {
                    if let Err(e) = stdin.write_all(data).and_then(|_| stdin.flush()) {
                        tracing::warn!(
                            error = %e,
                            "failed to write H.264 to ffplay stdin (process may have died)"
                        );
                        // Drop stdin so ensure_started restarts next time.
                        *guard = None;
                        if let Ok(mut c) = self.child.lock() {
                            if let Some(mut ch) = c.take() {
                                let _ = ch.kill();
                                let _ = ch.wait();
                            }
                        }
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "ffplay stdin mutex poisoned"),
        }
    }

    fn on_video_src_disconnect(&self) {
        tracing::info!("video source disconnected; stopping ffplay");
        if let Ok(mut f) = self.frames.lock() {
            *f = 0;
        }
        self.stop_process();
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        tracing::info!(?info, "audio format (ignored by ffmpeg/ffplay video-only backend)");
    }

    fn on_audio(&self, _data: &[u8]) {}

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
    fn resolve_or_errors_clearly() {
        match resolve_ffplay() {
            Ok(p) => {
                assert!(!p.as_os_str().is_empty());
            }
            Err(e) => {
                assert!(
                    e.contains("ffplay") || e.contains("PATH"),
                    "unexpected error: {e}"
                );
            }
        }
    }
}
