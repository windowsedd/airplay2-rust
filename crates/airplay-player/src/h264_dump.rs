//! H.264 bitstream dump consumer for debugging without a GPU/UI player.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use airplay_lib::{AudioStreamInfo, VideoStreamInfo};
use airplay_server::AirPlayConsumer;

/// Writes decrypted video annex-B (or raw NAL) bytes to a file.
///
/// Intended for protocol debugging: mirror from a device, then inspect
/// `dump.h264` with ffplay/ffmpeg.
pub struct H264Dump {
    path: PathBuf,
    file: Mutex<File>,
}

impl H264Dump {
    /// Open (create/truncate) the dump file at `path`.
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    /// Path of the dump file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AirPlayConsumer for H264Dump {
    fn on_video_format(&self, info: &VideoStreamInfo) {
        tracing::info!(
            stream_connection_id = %info.stream_connection_id,
            path = %self.path.display(),
            "video format; dumping H.264 to file"
        );
    }

    fn on_video(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        match self.file.lock() {
            Ok(mut file) => {
                if let Err(e) = file.write_all(data) {
                    tracing::warn!(error = %e, "failed to write video dump");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "H264Dump mutex poisoned");
            }
        }
    }

    fn on_video_src_disconnect(&self) {
        match self.file.lock() {
            Ok(mut file) => {
                if let Err(e) = file.flush() {
                    tracing::warn!(error = %e, "failed to flush video dump");
                } else {
                    tracing::info!(path = %self.path.display(), "video source disconnected; dump flushed");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "H264Dump mutex poisoned on disconnect");
            }
        }
    }

    fn on_audio_format(&self, info: &AudioStreamInfo) {
        tracing::info!(?info, "audio format (ignored by h264-dump)");
    }

    fn on_audio(&self, _data: &[u8]) {
        // Video-only dump backend; ignore audio samples.
    }

    fn on_audio_src_disconnect(&self) {
        tracing::debug!("audio source disconnected (h264-dump ignores audio)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airplay_server::AirPlayConsumer;

    #[test]
    fn writes_video_bytes_to_file() {
        let dir = std::env::temp_dir().join(format!(
            "airplay-h264-dump-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dump.h264");

        let dump = H264Dump::new(&path).expect("create dump");
        dump.on_video_format(&VideoStreamInfo::new("conn-1"));
        dump.on_video(b"\x00\x00\x00\x01\x67nalu");
        dump.on_video(b"more");
        dump.on_video_src_disconnect();

        let written = std::fs::read(&path).expect("read dump");
        assert_eq!(written, b"\x00\x00\x00\x01\x67nalumore");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
