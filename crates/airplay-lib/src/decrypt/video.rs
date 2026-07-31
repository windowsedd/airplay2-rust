//! FairPlay video decryptor (AES-CTR).
//!
//! Port of Java `com.github.serezhka.airplay.lib.internal.FairPlayVideoDecryptor`.

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use ctr::Ctr128BE;
use sha2::{Digest, Sha512};

use crate::error::{AirPlayError, Result};

type Aes128Ctr = Ctr128BE<Aes128>;

/// Stateful AES-CTR decryptor for FairPlay video payloads.
///
/// Key/IV derived via SHA-512 over `aesKey`, `sharedSecret`, and
/// `"AirPlayStreamKey" | "AirPlayStreamIV" + streamConnectionID`.
/// Residual partial blocks are handled via `next_decrypt_count` / `og` exactly
/// as in the Java implementation.
pub struct FairPlayVideoDecryptor {
    cipher: Aes128Ctr,
    og: [u8; 16],
    next_decrypt_count: usize,
}

impl FairPlayVideoDecryptor {
    /// Construct decryptor from decrypted AES key, pairing shared secret, and
    /// unsigned stream connection id string (Java `Long.toUnsignedString`).
    pub fn new(aes_key: &[u8], shared_secret: &[u8], stream_connection_id: &str) -> Result<Self> {
        if aes_key.len() < 16 {
            return Err(AirPlayError::Decrypt(format!(
                "aes_key too short: {}",
                aes_key.len()
            )));
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
        let eaes_key = hasher.finalize();

        let skey = format!("AirPlayStreamKey{stream_connection_id}");
        let mut hasher = Sha512::new();
        hasher.update(skey.as_bytes());
        hasher.update(&eaes_key[..16]);
        let hash1 = hasher.finalize();

        let siv = format!("AirPlayStreamIV{stream_connection_id}");
        let mut hasher = Sha512::new();
        hasher.update(siv.as_bytes());
        hasher.update(&eaes_key[..16]);
        let hash2 = hasher.finalize();

        let mut decrypt_aes_key = [0u8; 16];
        let mut decrypt_aes_iv = [0u8; 16];
        decrypt_aes_key.copy_from_slice(&hash1[..16]);
        decrypt_aes_iv.copy_from_slice(&hash2[..16]);

        let cipher = Aes128Ctr::new((&decrypt_aes_key).into(), (&decrypt_aes_iv).into());

        Ok(Self {
            cipher,
            og: [0u8; 16],
            next_decrypt_count: 0,
        })
    }

    /// Decrypt video buffer in place (stateful across calls for residual blocks).
    pub fn decrypt(&mut self, video: &mut [u8]) -> Result<()> {
        let next = self.next_decrypt_count;
        if next > 0 {
            for i in 0..next {
                video[i] ^= self.og[(16 - next) + i];
            }
        }

        let encryptlen = ((video.len() - next) / 16) * 16;
        if encryptlen > 0 {
            // In-place keystream XOR (Java Cipher.update in-place; the subsequent
            // System.arraycopy to the same range is a no-op).
            self.cipher
                .apply_keystream(&mut video[next..next + encryptlen]);
        }

        let restlen = (video.len() - next) % 16;
        let reststart = video.len() - restlen;
        self.next_decrypt_count = 0;
        if restlen > 0 {
            self.og.fill(0);
            self.og[..restlen].copy_from_slice(&video[reststart..]);
            self.cipher.apply_keystream(&mut self.og);
            video[reststart..].copy_from_slice(&self.og[..restlen]);
            self.next_decrypt_count = 16 - restlen;
        }
        Ok(())
    }
}
