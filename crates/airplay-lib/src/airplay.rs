//! AirPlay façade: pairing, FairPlay, RTSP setup, and decrypt.

use crate::decrypt::{FairPlayAudioDecryptor, FairPlayVideoDecryptor};
use crate::error::{AirPlayError, Result};
use crate::fairplay::FairPlay;
use crate::pairing::Pairing;
use crate::rtsp::Rtsp;
use crate::stream_info::MediaStreamInfo;

/// High-level AirPlay protocol helper matching Java `AirPlay`.
pub struct AirPlay {
    pairing: Pairing,
    fairplay: FairPlay,
    rtsp: Rtsp,
    fairplay_video_decryptor: Option<FairPlayVideoDecryptor>,
    fairplay_audio_decryptor: Option<FairPlayAudioDecryptor>,
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
            rtsp: Rtsp::new(),
            fairplay_video_decryptor: None,
            fairplay_audio_decryptor: None,
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

    /// `/fp-setup` FairPlay handshake.
    pub fn fair_play_setup(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.fairplay.fair_play_setup(request)
    }

    /// RTSP SETUP: store ekey/eiv or return media stream info.
    pub fn rtsp_setup(&mut self, plist_bytes: &[u8]) -> Result<Option<MediaStreamInfo>> {
        self.rtsp.setup(plist_bytes)
    }

    /// RTSP TEARDOWN: return media stream info if present.
    pub fn rtsp_teardown(&mut self, plist_bytes: &[u8]) -> Result<Option<MediaStreamInfo>> {
        self.rtsp.teardown(plist_bytes)
    }

    /// Decrypt FairPlay AES key from stored ekey (requires prior fp-setup + rtsp ekey).
    pub fn get_fairplay_aes_key(&self) -> Result<[u8; 16]> {
        let ekey = self
            .rtsp
            .ekey()
            .ok_or_else(|| AirPlayError::InvalidState("ekey not set; run rtsp_setup first".into()))?;
        self.fairplay.decrypt_aes_key(ekey)
    }

    /// Ready when shared secret, ekey, and stream connection id are available.
    pub fn is_fairplay_video_decryptor_ready(&self) -> bool {
        self.pairing.shared_secret().is_some()
            && self.rtsp.ekey().is_some()
            && self.rtsp.stream_connection_id().is_some()
    }

    /// Ready when shared secret, ekey, and eiv are available.
    pub fn is_fairplay_audio_decryptor_ready(&self) -> bool {
        self.pairing.shared_secret().is_some()
            && self.rtsp.ekey().is_some()
            && self.rtsp.eiv().is_some()
    }

    /// Decrypt video payload in place (lazy-inits decryptor when ready).
    pub fn decrypt_video(&mut self, video: &mut [u8]) -> Result<()> {
        if self.fairplay_video_decryptor.is_none() {
            if !self.is_fairplay_video_decryptor_ready() {
                return Err(AirPlayError::InvalidState(
                    "FairPlayVideoDecryptor not ready!".into(),
                ));
            }
            let aes_key = self.get_fairplay_aes_key()?;
            let shared = *self.pairing.shared_secret().expect("checked ready");
            let conn_id = self
                .rtsp
                .stream_connection_id()
                .expect("checked ready")
                .to_string();
            self.fairplay_video_decryptor =
                Some(FairPlayVideoDecryptor::new(&aes_key, &shared, &conn_id)?);
        }
        self.fairplay_video_decryptor
            .as_mut()
            .expect("just set")
            .decrypt(video)
    }

    /// Decrypt audio payload in place (lazy-inits decryptor when ready).
    pub fn decrypt_audio(&mut self, audio: &mut [u8], audio_length: usize) -> Result<()> {
        if self.fairplay_audio_decryptor.is_none() {
            if !self.is_fairplay_audio_decryptor_ready() {
                return Err(AirPlayError::InvalidState(
                    "FairPlayAudioDecryptor not ready!".into(),
                ));
            }
            let aes_key = self.get_fairplay_aes_key()?;
            let eiv = self.rtsp.eiv().expect("checked ready").to_vec();
            let shared = *self.pairing.shared_secret().expect("checked ready");
            self.fairplay_audio_decryptor =
                Some(FairPlayAudioDecryptor::new(&aes_key, &eiv, &shared)?);
        }
        self.fairplay_audio_decryptor
            .as_ref()
            .expect("just set")
            .decrypt(audio, audio_length)
    }

    /// Inject shared secret for tests that skip full pair-verify.
    #[cfg(test)]
    pub fn set_shared_secret_for_test(&mut self, secret: [u8; 32]) {
        self.pairing.set_shared_secret_for_test(secret);
    }
}
