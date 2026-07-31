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
        let mut payload = vec![0u8; size];
        if size > 0 {
            stream.read_exact(&mut payload).await?;
        }

        match hdr.payload_type {
            0 => {
                // Encrypted picture: decrypt in place, AVCC → Annex-B, on_video.
                let decrypt_ok = {
                    let mut ap = airplay.lock().expect("airplay mutex poisoned");
                    match ap.decrypt_video(&mut payload) {
                        Ok(()) => true,
                        Err(e) => {
                            warn!("decrypt_video failed: {e}");
                            false
                        }
                    }
                };
                if !decrypt_ok {
                    continue;
                }
                prepare_picture_nal_units(&mut payload);
                consumer.on_video(&payload);
            }
            1 => {
                // SPS/PPS — no decrypt.
                match prepare_sps_pps_nal_units(&payload) {
                    Some(annex_b) => consumer.on_video(&annex_b),
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
