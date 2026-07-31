//! AirPlay façade: pairing (and later FairPlay / RTSP / decrypt).

use crate::error::Result;
use crate::fairplay::FairPlay;
use crate::pairing::Pairing;

/// High-level AirPlay protocol helper matching Java `AirPlay`.
pub struct AirPlay {
    pairing: Pairing,
    fairplay: FairPlay,
}

impl Default for AirPlay {
    fn default() -> Self {
        Self::new()
    }
}

impl AirPlay {
    pub fn new() -> Self {
        Self {
            pairing: Pairing::new(),
            fairplay: FairPlay::new(),
        }
    }

    pub fn pair_setup(&self) -> [u8; 32] {
        self.pairing.pair_setup()
    }

    pub fn pair_verify(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.pairing.pair_verify(request)
    }

    pub fn is_pair_verified(&self) -> bool {
        self.pairing.is_pair_verified()
    }

    pub fn shared_secret(&self) -> Option<&[u8; 32]> {
        self.pairing.shared_secret()
    }

    /// `/fp-setup` FairPlay handshake (setup messages only until OmgHax lands).
    pub fn fair_play_setup(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.fairplay.fair_play_setup(request)
    }
}
