//! FairPlay audio decryptor (AES-CBC).
//!
//! Port of Java `com.github.serezhka.airplay.lib.internal.FairPlayAudioDecryptor`.

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use aes::Aes128;
use cbc::Decryptor;
use sha2::{Digest, Sha512};

use crate::error::{AirPlayError, Result};

type Aes128CbcDec = Decryptor<Aes128>;

/// AES-CBC (NoPadding) decryptor for FairPlay audio payloads.
///
/// Key = SHA-512(aesKey || sharedSecret)[0..16], IV = eiv.
/// Each `decrypt` call re-inits CBC with the same IV (mirrors Java).
pub struct FairPlayAudioDecryptor {
    aes_iv: Vec<u8>,
    eaes_key: [u8; 16],
}

impl FairPlayAudioDecryptor {
    pub fn new(aes_key: &[u8], aes_iv: &[u8], shared_secret: &[u8]) -> Result<Self> {
        if aes_key.len() < 16 {
            return Err(AirPlayError::Decrypt(format!(
                "aes_key too short: {}",
                aes_key.len()
            )));
        }
        if aes_iv.is_empty() {
            return Err(AirPlayError::Decrypt("aes_iv is empty".into()));
        }
        if shared_secret.len() < 32 {
            return Err(AirPlayError::Decrypt(format!(
                "shared_secret too short: {}",
                shared_secret.len()
            )));
        }

        let mut hasher = Sha512::new();
        hasher.update(aes_key);
        hasher.update(shared_secret);
        let digest = hasher.finalize();
        let mut eaes_key = [0u8; 16];
        eaes_key.copy_from_slice(&digest[..16]);

        Ok(Self {
            aes_iv: aes_iv.to_vec(),
            eaes_key,
        })
    }

    /// Decrypt full 16-byte blocks of `audio[..audio_length]` in place.
    ///
    /// Processes `audio_length / 16 * 16` bytes; remainder is left untouched.
    /// CBC is re-initialized with the same IV on every call (Java semantics).
    pub fn decrypt(&self, audio: &mut [u8], audio_length: usize) -> Result<()> {
        if audio_length > audio.len() {
            return Err(AirPlayError::Decrypt(format!(
                "audio_length {audio_length} exceeds buffer {}",
                audio.len()
            )));
        }

        let block_len = (audio_length / 16) * 16;
        if block_len == 0 {
            return Ok(());
        }

        if self.aes_iv.len() < 16 {
            return Err(AirPlayError::Decrypt(format!(
                "aes_iv too short: {}",
                self.aes_iv.len()
            )));
        }
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&self.aes_iv[..16]);

        let decryptor = Aes128CbcDec::new((&self.eaes_key).into(), (&iv).into());
        decryptor
            .decrypt_padded_mut::<NoPadding>(&mut audio[..block_len])
            .map_err(|e| AirPlayError::Decrypt(format!("AES-CBC decrypt failed: {e:?}")))?;
        Ok(())
    }
}
