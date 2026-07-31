//! Audio control UDP server (timing / sync packets).
//!
//! Matches Java `AudioControlServer` + `AudioControlHandler`: bind ephemeral
//! UDP, accept datagrams, and log packet type (no decrypt / consumer path).

use tokio::net::UdpSocket;
use tokio::task::AbortHandle;
use tracing::{debug, info};

/// Bind an ephemeral UDP port for audio control.
pub async fn bind() -> std::io::Result<(UdpSocket, u16)> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let port = socket.local_addr()?.port();
    Ok((socket, port))
}

/// Spawn a recv loop that reads and discards (logs) control packets.
pub fn run_recv(socket: UdpSocket) -> AbortHandle {
    let join = tokio::spawn(async move {
        let mut buf = vec![0u8; 512];
        info!("audio-control UDP server listening");
        loop {
            match socket.recv(&mut buf).await {
                Ok(n) => {
                    if n >= 2 {
                        // Java: type = contentBytes[1] & ~0x80
                        let pkt_type = buf[1] & !0x80;
                        debug!(
                            packet_type = pkt_type,
                            length = n,
                            "audio control packet"
                        );
                    }
                }
                Err(e) => {
                    debug!("audio-control recv ended: {e}");
                    break;
                }
            }
        }
    });
    join.abort_handle()
}
