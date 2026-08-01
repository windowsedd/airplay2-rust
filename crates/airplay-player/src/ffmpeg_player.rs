//! FFmpeg / ffplay live H.264 window.
//!
//! Strategy (Windows-friendly):
//! 1. Buffer annex-B until we have SPS (or ~32 KiB), then spawn real `ffplay.exe`
//!    with stdin — **do not** pre-start on an empty pipe (ffplay exits immediately).
//! 2. If stdin spawn fails, fall back to opening the growing `dump.h264` file
//!    with `ffplay -f h264 -i <path>` (works on this machine).
//! 3. Restart if the process dies while frames still arrive.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::AirPlayConsumer;

const MAX_BUFFER: usize = 4 * 1024 * 1024;

/// Orientation for ffplay (`transpose` filter).
///
/// **Auto:** no transpose — default portrait UI; landscape when stream is wide (games).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FfmpegRotateMode {
    #[default]
    Auto,
    None,
    Cw,
    Ccw,
}

impl FfmpegRotateMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "false" | "0" => Self::None,
            "cw" | "clockwise" | "right" | "90" => Self::Cw,
            "ccw" | "counterclockwise" | "counter-clockwise" | "left" | "270" => Self::Ccw,
            _ => Self::Auto,
        }
    }
}

/// Live window via `ffplay`.
pub struct FFmpegPlayer {
    ffplay_path: PathBuf,
    /// Optional dump path for file-based fallback (set from auto mode).
    dump_path: Mutex<Option<PathBuf>>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    /// Annex-B buffer until player is fed.
    pending: Mutex<Vec<u8>>,
    started: AtomicBool,
    use_file_mode: AtomicBool,
    last_restart: Mutex<Instant>,
    frames: AtomicU64,
    rotate_mode: FfmpegRotateMode,
    /// Last reported stream size; drives transpose filter.
    last_size: Mutex<(u32, u32)>,
    /// Active transpose filter for next spawn (`None` = no -vf).
    vf_transpose: Mutex<Option<&'static str>>,
}

impl FFmpegPlayer {
    pub fn new() -> Result<Self, String> {
        Self::with_rotate(FfmpegRotateMode::Auto)
    }

    pub fn with_rotate(rotate_mode: FfmpegRotateMode) -> Result<Self, String> {
        let ffplay_path = resolve_ffplay()?;
        tracing::info!(path = %ffplay_path.display(), ?rotate_mode, "ffplay resolved");
        Ok(Self {
            ffplay_path,
            dump_path: Mutex::new(None),
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            pending: Mutex::new(Vec::with_capacity(256 * 1024)),
            started: AtomicBool::new(false),
            use_file_mode: AtomicBool::new(false),
            last_restart: Mutex::new(Instant::now() - Duration::from_secs(5)),
            frames: AtomicU64::new(0),
            rotate_mode,
            last_size: Mutex::new((0, 0)),
            vf_transpose: Mutex::new(None),
        })
    }

    /// Tell the player where `h264-dump` writes so file-mode fallback can open it.
    pub fn set_dump_path(&self, path: impl AsRef<Path>) {
        if let Ok(mut g) = self.dump_path.lock() {
            *g = Some(path.as_ref().to_path_buf());
        }
    }

    fn throttle_restart(&self) -> bool {
        if let Ok(mut t) = self.last_restart.lock() {
            if t.elapsed() < Duration::from_millis(800) {
                return false;
            }
            *t = Instant::now();
        }
        true
    }

    fn process_alive(&self) -> bool {
        let mut guard = match self.child.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let Some(child) = guard.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                tracing::warn!(?status, "ffplay exited");
                *guard = None;
                if let Ok(mut s) = self.stdin.lock() {
                    *s = None;
                }
                self.started.store(false, Ordering::SeqCst);
                false
            }
            Err(e) => {
                tracing::warn!(error = %e, "ffplay try_wait");
                false
            }
        }
    }

    fn transpose_for(&self, width: u32, height: u32) -> Option<&'static str> {
        // FFmpeg transpose: 1 = 90° CW, 2 = 90° CCW.
        // Auto/None: no filter — portrait stream stays upright; landscape = game.
        let _ = (width, height);
        match self.rotate_mode {
            FfmpegRotateMode::None | FfmpegRotateMode::Auto => None,
            FfmpegRotateMode::Cw => Some("transpose=1"),
            FfmpegRotateMode::Ccw => Some("transpose=2"),
        }
    }

    fn spawn_stdin_mode(&self) -> Result<(), String> {
        let mut cmd = Command::new(&self.ffplay_path);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
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
            "5000000",
            "-analyzeduration",
            "5000000",
        ]);
        if let Ok(vf) = self.vf_transpose.lock() {
            if let Some(filter) = *vf {
                cmd.args(["-vf", filter]);
            }
        }
        cmd.args(["-i", "pipe:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "stdin-mode spawn {} failed: {e}",
                self.ffplay_path.display()
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ffplay stdin missing".to_string())?;

        *self.child.lock().map_err(|e| e.to_string())? = Some(child);
        *self.stdin.lock().map_err(|e| e.to_string())? = Some(stdin);
        self.use_file_mode.store(false, Ordering::SeqCst);
        self.started.store(true, Ordering::SeqCst);
        tracing::info!("ffplay stdin-mode started (window title: airplay2-rust)");
        Ok(())
    }

    fn spawn_file_mode(&self) -> Result<(), String> {
        let path = self
            .dump_path
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| "no dump_path set for file-mode ffplay".to_string())?;

        if !path.is_file() {
            return Err(format!("dump file missing: {}", path.display()));
        }
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if meta.len() < 64 {
            return Err(format!("dump file too small yet ({} bytes)", meta.len()));
        }

        // Absolute path required.
        let abs = std::fs::canonicalize(&path).unwrap_or(path);

        // Use cmd `start` so Windows always creates a visible GUI process.
        // /B would hide console but still show SDL window for ffplay.
        let mut cmd = Command::new(&self.ffplay_path);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-window_title",
            "airplay2-rust",
            "-fflags",
            "nobuffer+genpts",
            "-framedrop",
            "-f",
            "h264",
            "-i",
            abs.to_str().ok_or("dump path not utf-8")?,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|e| format!("file-mode spawn {} failed: {e}", self.ffplay_path.display()))?;

        *self.child.lock().map_err(|e| e.to_string())? = Some(child);
        *self.stdin.lock().map_err(|e| e.to_string())? = None;
        self.use_file_mode.store(true, Ordering::SeqCst);
        self.started.store(true, Ordering::SeqCst);
        tracing::info!(
            path = %abs.display(),
            "ffplay file-mode started on dump.h264 (window title: airplay2-rust) — check taskbar"
        );
        Ok(())
    }

    fn start_player(&self) -> Result<(), String> {
        if self.process_alive() {
            return Ok(());
        }
        if !self.throttle_restart() {
            return Err("restart throttled".into());
        }

        // Prefer stdin (true live). Fall back to file (proven on this PC).
        match self.spawn_stdin_mode() {
            Ok(()) => Ok(()),
            Err(e1) => {
                tracing::warn!(error = %e1, "stdin-mode failed; trying file-mode on dump.h264");
                self.spawn_file_mode().map_err(|e2| format!("{e1} | {e2}"))
            }
        }
    }

    fn flush_pending_to_stdin(&self) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|e| e.to_string())?;
        if pending.is_empty() {
            return Ok(());
        }
        if self.use_file_mode.load(Ordering::SeqCst) {
            pending.clear();
            return Ok(());
        }
        let mut stdin_g = self.stdin.lock().map_err(|e| e.to_string())?;
        let Some(stdin) = stdin_g.as_mut() else {
            return Err("no stdin".into());
        };
        stdin
            .write_all(&pending)
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("write pending: {e}"))?;
        pending.clear();
        Ok(())
    }

    fn write_live(&self, data: &[u8]) -> Result<(), String> {
        if self.use_file_mode.load(Ordering::SeqCst) {
            // File mode: dump writer handles disk; optionally restart ffplay if dead.
            if !self.process_alive() {
                let _ = self.start_player();
            }
            return Ok(());
        }
        let mut stdin_g = self.stdin.lock().map_err(|e| e.to_string())?;
        let Some(stdin) = stdin_g.as_mut() else {
            return Err("no stdin".into());
        };
        stdin
            .write_all(data)
            .and_then(|_| stdin.flush())
            .map_err(|e| {
                // Force restart next time.
                *stdin_g = None;
                if let Ok(mut c) = self.child.lock() {
                    if let Some(mut ch) = c.take() {
                        let _ = ch.kill();
                        let _ = ch.wait();
                    }
                }
                self.started.store(false, Ordering::SeqCst);
                format!("write stdin: {e}")
            })?;
        Ok(())
    }

    fn stop_process(&self) {
        if let Ok(mut s) = self.stdin.lock() {
            *s = None;
        }
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut ch) = c.take() {
                let _ = ch.kill();
                let _ = ch.wait();
            }
        }
        self.started.store(false, Ordering::SeqCst);
    }
}

fn has_h264_sps(buf: &[u8]) -> bool {
    // Look for annex-B start code + NAL type 7 (SPS).
    let mut i = 0;
    while i + 4 < buf.len() {
        if buf[i] == 0 && buf[i + 1] == 0 {
            let (sc, nal_i) = if buf[i + 2] == 1 {
                (3, i + 3)
            } else if buf[i + 2] == 0 && buf[i + 3] == 1 {
                (4, i + 4)
            } else {
                i += 1;
                continue;
            };
            if nal_i < buf.len() {
                let nal_type = buf[nal_i] & 0x1f;
                if nal_type == 7 {
                    return true;
                }
            }
            i += sc;
        } else {
            i += 1;
        }
    }
    false
}

fn resolve_ffplay() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let real =
            PathBuf::from(r"C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin\ffplay.exe");
        if real.is_file() {
            return Ok(real);
        }
        if let Ok(out) = Command::new("where").arg("ffplay").output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let p = PathBuf::from(line.trim());
                    if !p.is_file() {
                        continue;
                    }
                    let s = p.to_string_lossy().to_ascii_lowercase();
                    if s.contains(r"\chocolatey\bin\") {
                        continue; // skip ShimGen
                    }
                    if s.ends_with("ffplay.exe") {
                        return Ok(p);
                    }
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
        Err(e) => Err(format!("ffplay not found: {e}")),
    }
}

impl Default for FFmpegPlayer {
    fn default() -> Self {
        Self::new().expect("ffplay")
    }
}

impl AirPlayConsumer for FFmpegPlayer {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            "video format — will start ffplay after first SPS/frames (not empty pre-start)"
        );
        // Reset stream state for a new mirror session.
        self.stop_process();
        if let Ok(mut p) = self.pending.lock() {
            p.clear();
        }
        if let Ok(mut s) = self.last_size.lock() {
            *s = (0, 0);
        }
        if let Ok(mut v) = self.vf_transpose.lock() {
            *v = None;
        }
        self.frames.store(0, Ordering::SeqCst);
    }

    fn on_video_size(&self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let new_vf = self.transpose_for(width, height);
        let (size_changed, vf_changed) = {
            let mut last = match self.last_size.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let size_changed = *last != (width, height);
            if size_changed {
                *last = (width, height);
            }
            let mut vf = match self.vf_transpose.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let vf_changed = *vf != new_vf;
            if vf_changed {
                *vf = new_vf;
            }
            (size_changed, vf_changed)
        };
        if size_changed || vf_changed {
            let portrait = height > width;
            tracing::info!(
                width,
                height,
                portrait,
                mode = if portrait {
                    "portrait (default home/UI)"
                } else {
                    "landscape (game/app detected)"
                },
                ?new_vf,
                "ffplay orientation"
            );
        }
        // Restart player so -vf applies (only if already running).
        if vf_changed && self.started.load(Ordering::SeqCst) && self.throttle_restart() {
            tracing::info!("restarting ffplay for orientation change");
            self.stop_process();
        }
    }

    fn on_video(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let n = self.frames.fetch_add(1, Ordering::SeqCst) + 1;

        // Accumulate until we can start with real bitstream (SPS preferred).
        if !self.started.load(Ordering::SeqCst) || !self.process_alive() {
            if let Ok(mut pending) = self.pending.lock() {
                if pending.len() + data.len() <= MAX_BUFFER {
                    pending.extend_from_slice(data);
                }
                let ready = has_h264_sps(&pending) || pending.len() >= 32 * 1024;
                if ready {
                    let snapshot_len = pending.len();
                    drop(pending);
                    match self.start_player() {
                        Ok(()) => {
                            if let Err(e) = self.flush_pending_to_stdin() {
                                tracing::warn!(error = %e, "flush pending to ffplay failed");
                            } else {
                                tracing::info!(
                                    buffered = snapshot_len,
                                    frame = n,
                                    "ffplay started and fed buffered H.264 — look for window 'airplay2-rust'"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "could not start ffplay");
                        }
                    }
                }
            }
            return;
        }

        if let Err(e) = self.write_live(data) {
            tracing::warn!(error = %e, frame = n, "ffplay write failed");
        } else if n == 1 || n % 120 == 0 {
            tracing::info!(n, bytes = data.len(), "ffplay push H.264");
        }
    }

    fn on_video_src_disconnect(&self) {
        tracing::info!("video disconnected; stopping ffplay");
        self.stop_process();
        if let Ok(mut p) = self.pending.lock() {
            p.clear();
        }
        self.frames.store(0, Ordering::SeqCst);
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        tracing::info!(?info, "audio ignored by ffplay video backend");
    }

    fn on_audio(&self, _data: &[u8]) {}

    fn on_audio_src_disconnect(&self) {}
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
    fn detect_sps() {
        let mut buf = vec![0, 0, 0, 1, 0x67, 0x42, 0x00];
        assert!(has_h264_sps(&buf));
        buf = vec![0, 0, 1, 0x67, 0x42];
        assert!(has_h264_sps(&buf));
        assert!(!has_h264_sps(&[0, 0, 0, 1, 0x61, 0x00]));
    }

    #[test]
    fn resolve_ffplay_or_clear_error() {
        match resolve_ffplay() {
            Ok(p) => assert!(!p.as_os_str().is_empty()),
            Err(e) => assert!(e.contains("ffplay")),
        }
    }
}
