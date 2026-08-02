//! Audio UDP media server: recv → parse → decrypt → consumer.
//!
//! Transport matches Java `AudioServer` (UDP datagrams). A small reorder window
//! accepts out-of-order packets and **sequence-number wrap** (u16), so audio does
//! not go permanently silent after ~65k packets.

use std::sync::{Arc, Mutex};

use airplay_lib::AirPlay;
use tokio::net::UdpSocket;
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

use crate::consumer::AirPlayConsumer;
use crate::packet::audio::parse_audio_packet;

/// How far behind the expected sequence a packet may be and still be accepted.
/// Larger gaps are treated as a stream reset (common after Wi‑Fi hiccups).
const MAX_REORDER_LAG: u16 = 64;
/// How far ahead of the last accepted seq we still treat as the same stream.
const MAX_FORWARD_JUMP: u16 = 3000;

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
        let mut tracker = SeqTracker::new();
        let mut buf = vec![0u8; 4096];
        let mut packets_ok: u64 = 0;
        let mut packets_dup: u64 = 0;
        let mut packets_drop: u64 = 0;
        let mut decrypt_fail: u64 = 0;

        info!("audio UDP server listening");

        loop {
            let n = match socket.recv(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    // Transient socket errors should not kill the whole session.
                    match e.kind() {
                        std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted => {
                            continue;
                        }
                        std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted => {
                            debug!("audio recv connection reset: {e}");
                            // UDP: keep listening; sender may resume.
                            continue;
                        }
                        _ => {
                            warn!("audio recv error (continuing): {e}");
                            continue;
                        }
                    }
                }
            };
            if n < 12 {
                continue;
            }

            let mut pkt = match parse_audio_packet(&buf[..n]) {
                Some(p) => p,
                None => continue,
            };

            match tracker.accept(pkt.sequence_number) {
                SeqDecision::Accept => {}
                SeqDecision::Resync => {
                    info!(
                        seq = pkt.sequence_number,
                        "audio sequence resync (gap or wrap); continuing"
                    );
                }
                SeqDecision::Duplicate => {
                    packets_dup += 1;
                    continue;
                }
                SeqDecision::TooOld => {
                    packets_drop += 1;
                    if packets_drop <= 5 || packets_drop % 200 == 0 {
                        debug!(
                            seq = pkt.sequence_number,
                            packets_drop, "audio packet too old; drop"
                        );
                    }
                    continue;
                }
            }

            let len = pkt.encoded_audio.len();
            if len == 0 {
                continue;
            }

            let decrypt_ok = {
                let mut ap = match airplay.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        warn!("airplay mutex poisoned on audio: {e}");
                        continue;
                    }
                };
                match ap.decrypt_audio(&mut pkt.encoded_audio, len) {
                    Ok(()) => true,
                    Err(e) => {
                        decrypt_fail += 1;
                        if decrypt_fail <= 5 || decrypt_fail % 60 == 0 {
                            warn!(decrypt_fail, "decrypt_audio failed: {e}");
                        }
                        false
                    }
                }
            };
            if !decrypt_ok {
                continue;
            }

            consumer.on_audio(&pkt.encoded_audio);
            packets_ok += 1;
            if packets_ok == 1 {
                info!(bytes = len, seq = pkt.sequence_number, "first audio frame → consumer");
            } else if packets_ok % 500 == 0 {
                debug!(
                    packets_ok,
                    packets_dup,
                    packets_drop,
                    decrypt_fail,
                    seq = pkt.sequence_number,
                    "audio recv progress"
                );
            }
        }
        // Unreachable with current continue-on-error loop; kept for abort path.
        #[allow(unreachable_code)]
        {
            info!(
                packets_ok,
                packets_dup, packets_drop, decrypt_fail, "audio recv task exited"
            );
        }
    });
    join.abort_handle()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeqDecision {
    Accept,
    Duplicate,
    TooOld,
    Resync,
}

/// Tracks RTP-like u16 sequence numbers with wrap-around.
struct SeqTracker {
    last: Option<u16>,
}

impl SeqTracker {
    fn new() -> Self {
        Self { last: None }
    }

    fn accept(&mut self, seq: u16) -> SeqDecision {
        let decision = match self.last {
            None => SeqDecision::Accept,
            Some(prev) if seq == prev => SeqDecision::Duplicate,
            Some(prev) => {
                let forward = seq.wrapping_sub(prev);
                if forward == 0 {
                    SeqDecision::Duplicate
                } else if forward <= MAX_FORWARD_JUMP {
                    // Includes wrap: e.g. prev=65535, seq=0 → forward=1
                    if forward > 1 + MAX_REORDER_LAG {
                        SeqDecision::Resync
                    } else {
                        SeqDecision::Accept
                    }
                } else {
                    // Large "backward" step in modular space.
                    let backward = prev.wrapping_sub(seq);
                    if backward > 0 && backward <= MAX_REORDER_LAG {
                        SeqDecision::TooOld
                    } else {
                        // Huge jump either way → treat as new stream timeline.
                        SeqDecision::Resync
                    }
                }
            }
        };
        match decision {
            SeqDecision::Accept | SeqDecision::Resync => {
                self.last = Some(seq);
            }
            SeqDecision::Duplicate | SeqDecision::TooOld => {}
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_monotonic_and_wrap() {
        let mut t = SeqTracker::new();
        assert_eq!(t.accept(65530), SeqDecision::Accept);
        assert_eq!(t.accept(65531), SeqDecision::Accept);
        assert_eq!(t.accept(65531), SeqDecision::Duplicate);
        assert_eq!(t.accept(65535), SeqDecision::Accept);
        // wrap to 0 must keep flowing
        assert_eq!(t.accept(0), SeqDecision::Accept);
        assert_eq!(t.accept(1), SeqDecision::Accept);
    }

    #[test]
    fn drops_old_reorder_within_window() {
        let mut t = SeqTracker::new();
        assert_eq!(t.accept(100), SeqDecision::Accept);
        assert_eq!(t.accept(101), SeqDecision::Accept);
        assert_eq!(t.accept(90), SeqDecision::TooOld);
    }

    #[test]
    fn large_gap_resynchronizes_instead_of_permanent_silence() {
        let mut t = SeqTracker::new();
        assert_eq!(t.accept(1), SeqDecision::Accept);
        // Jump far ahead (packet loss burst) — must not lock forever.
        let d = t.accept(5000);
        assert!(matches!(d, SeqDecision::Resync | SeqDecision::Accept));
        assert_eq!(t.accept(5001), SeqDecision::Accept);
    }

    #[test]
    fn wrap_does_not_treat_new_epoch_as_duplicate_forever() {
        let mut t = SeqTracker::new();
        // Simulate long session near wrap
        t.accept(65530);
        for s in 65531..=65535 {
            assert!(matches!(
                t.accept(s),
                SeqDecision::Accept | SeqDecision::Resync
            ));
        }
        // After wrap, audio must keep flowing
        for s in 0..10u16 {
            let d = t.accept(s);
            assert!(
                matches!(d, SeqDecision::Accept | SeqDecision::Resync),
                "seq {s} => {d:?}"
            );
        }
    }
}
