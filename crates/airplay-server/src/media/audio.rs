//! Audio UDP media server: recv → parse → decrypt → consumer.
//!
//! Transport matches Java `AudioServer` (UDP datagrams). Reordering buffer is
//! optional for v1 — decrypt immediately when sequence increases.

use std::sync::{Arc, Mutex};

use airplay_lib::AirPlay;
use tokio::net::UdpSocket;
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

use crate::consumer::AirPlayConsumer;
use crate::packet::audio::parse_audio_packet;

/// Bind an ephemeral UDP port for audio data.
pub async fn bind() -> std::io::Result<(UdpSocket, u16)> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let port = socket.local_addr()?.port();
    Ok((socket, port))
}

/// Spawn the audio receive loop. Abort the returned handle on TEARDOWN.
pub fn run_recv(
    socket: UdpSocket,
    airplay: Arc<Mutex<AirPlay>>,
    consumer: Arc<dyn AirPlayConsumer>,
) -> AbortHandle {
    let join = tokio::spawn(async move {
        // v1 simple path: no reordering buffer; process when seq increases.
        // (Java AudioHandler keeps a 512-slot reorder buffer — optional later.)
        let mut prev_seq: u16 = 0;
        let mut have_prev = false;
        let mut buf = vec![0u8; 2048];

        info!("audio UDP server listening");

        loop {
            let n = match socket.recv(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    debug!("audio recv ended: {e}");
                    break;
                }
            };
            if n < 12 {
                continue;
            }

            let mut pkt = match parse_audio_packet(&buf[..n]) {
                Some(p) => p,
                None => continue,
            };

            let seq = pkt.sequence_number;
            if have_prev && seq <= prev_seq {
                // Duplicate / out-of-order — drop (simple path).
                continue;
            }

            let len = pkt.encoded_audio.len();
            if len == 0 {
                have_prev = true;
                prev_seq = seq;
                continue;
            }

            let decrypt_ok = {
                let mut ap = airplay.lock().expect("airplay mutex poisoned");
                match ap.decrypt_audio(&mut pkt.encoded_audio, len) {
                    Ok(()) => true,
                    Err(e) => {
                        warn!("decrypt_audio failed: {e}");
                        false
                    }
                }
            };
            if !decrypt_ok {
                continue;
            }

            consumer.on_audio(&pkt.encoded_audio);
            have_prev = true;
            prev_seq = seq;
        }
    });
    join.abort_handle()
}
