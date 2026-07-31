//! FairPlay video encryptor (AES-CTR).
//!
//! Port of Java `com.github.serezhka.airplay.client.crypto.FairPlayVideoEncryptor`.
//! Key/IV derivation and residual-block handling mirror
//! [`airplay_lib::FairPlayVideoDecryptor`] (CTR is symmetric).

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use airplay_lib::{AirPlayError, Result};
use ctr::Ctr128BE;
use sha2::{Digest, Sha512};

type Aes128Ctr = Ctr128BE<Aes128>;

/// Stateful AES-CTR encryptor for FairPlay video payloads (sender side).
///
/// Key/IV derived via SHA-512 over `aes_key`, `shared_secret`, and
/// `"AirPlayStreamKey" | "AirPlayStreamIV" + stream_connection_id`.
pub struct FairPlayVideoEncryptor {
    cipher: Aes128Ctr,
    og: [u8; 16],
    next_encrypt_count: usize,
}

impl FairPlayVideoEncryptor {
    /// Construct encryptor from AES key, pairing shared secret, and stream
    /// connection id string (Java `Long.toUnsignedString`).
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

        let mut encrypt_aes_key = [0u8; 16];
        let mut encrypt_aes_iv = [0u8; 16];
        encrypt_aes_key.copy_from_slice(&hash1[..16]);
        encrypt_aes_iv.copy_from_slice(&hash2[..16]);

        let cipher = Aes128Ctr::new((&encrypt_aes_key).into(), (&encrypt_aes_iv).into());

        Ok(Self {
            cipher,
            og: [0u8; 16],
            next_encrypt_count: 0,
        })
    }

    /// Encrypt video buffer in place (stateful across calls for residual blocks).
    pub fn encrypt(&mut self, video: &mut [u8]) -> Result<()> {
        let next = self.next_encrypt_count;
        if next > 0 {
            for i in 0..next {
                video[i] ^= self.og[(16 - next) + i];
            }
        }

        let encryptlen = ((video.len() - next) / 16) * 16;
        if encryptlen > 0 {
            self.cipher
                .apply_keystream(&mut video[next..next + encryptlen]);
        }

        let restlen = (video.len() - next) % 16;
        let reststart = video.len() - restlen;
        self.next_encrypt_count = 0;
        if restlen > 0 {
            self.og.fill(0);
            self.og[..restlen].copy_from_slice(&video[reststart..]);
            self.cipher.apply_keystream(&mut self.og);
            video[reststart..].copy_from_slice(&self.og[..restlen]);
            self.next_encrypt_count = 16 - restlen;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airplay_lib::FairPlayVideoDecryptor;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let aes_key = [7u8; 16];
        let shared = [9u8; 32];
        let conn_id = "123456789";

        let mut enc =
            FairPlayVideoEncryptor::new(&aes_key, &shared, conn_id).expect("encryptor");
        let mut dec =
            FairPlayVideoDecryptor::new(&aes_key, &shared, conn_id).expect("decryptor");

        let mut buf = (0u8..100).collect::<Vec<_>>();
        let original = buf.clone();
        enc.encrypt(&mut buf).expect("encrypt");
        assert_ne!(buf, original, "ciphertext should differ");
        dec.decrypt(&mut buf).expect("decrypt");
        assert_eq!(buf, original);
    }

    #[test]
    fn rejects_short_key() {
        let err = FairPlayVideoEncryptor::new(&[1u8; 8], &[0u8; 32], "1");
        assert!(err.is_err());
    }
}
