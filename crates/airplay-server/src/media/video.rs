//! Video TCP media server: accept → frame → decrypt → consumer.

use std::sync::{Arc, Mutex};

use airplay_lib::AirPlay;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

use crate::consumer::AirPlayConsumer;
use crate::packet::video::{
    parse_video_header, prepare_picture_nal_units, prepare_sps_pps_nal_units, VIDEO_HEADER_LEN,
};

/// Bind an ephemeral TCP port for video data.
pub async fn bind() -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Spawn the video accept loop. Abort the returned handle on TEARDOWN.
pub fn run_accept(
    listener: TcpListener,
    airplay: Arc<Mutex<AirPlay>>,
    consumer: Arc<dyn AirPlayConsumer>,
) -> AbortHandle {
    let join = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    info!(%peer, "video client connected");
                    let airplay = Arc::clone(&airplay);
                    let consumer = Arc::clone(&consumer);
                    tokio::spawn(async move {
                        if let Err(e) = handle_video_connection(stream, airplay, consumer).await {
                            debug!(%peer, "video connection closed: {e}");
                        }
                    });
                }
                Err(e) => {
                    // Listener closed / aborted — exit accept loop.
                    debug!("video accept ended: {e}");
                    break;
                }
            }
        }
    });
    join.abort_handle()
}

async fn handle_video_connection(
    mut stream: TcpStream,
    airplay: Arc<Mutex<AirPlay>>,
    consumer: Arc<dyn AirPlayConsumer>,
) -> std::io::Result<()> {
    let mut packets: u64 = 0;
    let mut frames_out: u64 = 0;
    let mut decrypt_fail: u64 = 0;
    loop {
        let mut header = [0u8; VIDEO_HEADER_LEN];
        stream.read_exact(&mut header).await?;

        let hdr = match parse_video_header(&header) {
            Some(h) => h,
            None => {
                warn!("video header parse failed");
                continue;
            }
        };

        let size = hdr.payload_size as usize;
        // Guard absurd sizes (corrupt header) so we don't OOM.
        if size > 16 * 1024 * 1024 {
            warn!(size, payload_type = hdr.payload_type, "video payload too large; drop connection");
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "video payload too large",
            ));
        }
        let mut payload = vec![0u8; size];
        if size > 0 {
            stream.read_exact(&mut payload).await?;
        }

        packets += 1;
        if packets <= 8 || packets % 120 == 0 {
            info!(
                n = packets,
                payload_type = hdr.payload_type,
                size,
                frames_out,
                decrypt_fail,
                "video packet"
            );
        }

        match hdr.payload_type {
            0 => {
                // Encrypted picture: decrypt in place, AVCC → Annex-B, on_video.
                let decrypt_ok = {
                    let mut ap = airplay.lock().expect("airplay mutex poisoned");
                    match ap.decrypt_video(&mut payload) {
                        Ok(()) => true,
                        Err(e) => {
                            decrypt_fail += 1;
                            if decrypt_fail <= 5 || decrypt_fail % 60 == 0 {
                                warn!(decrypt_fail, "decrypt_video failed: {e}");
                            }
                            false
                        }
                    }
                };
                if !decrypt_ok {
                    continue;
                }
                prepare_picture_nal_units(&mut payload);
                frames_out += 1;
                if frames_out == 1 {
                    info!(bytes = payload.len(), "first decrypted video frame → consumer");
                }
                consumer.on_video(&payload);
            }
            1 => {
                // SPS/PPS — no decrypt.
                match prepare_sps_pps_nal_units(&payload) {
                    Some(annex_b) => {
                        frames_out += 1;
                        info!(bytes = annex_b.len(), "SPS/PPS annex-B → consumer");
                        consumer.on_video(&annex_b);
                    }
                    None => warn!("prepare_sps_pps_nal_units failed (truncated payload)"),
                }
            }
            other => {
                debug!(
                    payload_type = other,
                    length = size,
                    "video packet skipped"
                );
            }
        }
    }
}
