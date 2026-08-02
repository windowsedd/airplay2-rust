//! AirPlay pair-setup / pair-verify (Ed25519 + X25519 + AES-CTR).
//!
//! Semantics match Java `com.github.serezhka.airplay.lib.internal.Pairing`.

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use ctr::Ctr128BE;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use sha2::{Digest, Sha512};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::error::{AirPlayError, Result};

type Aes128Ctr = Ctr128BE<Aes128>;

/// AirPlay pairing state machine (pair-setup + two-phase pair-verify).
pub struct Pairing {
    ed_key: SigningKey,
    ed_theirs: Option<[u8; 32]>,
    ecdh_ours: Option<[u8; 32]>,
    ecdh_theirs: Option<[u8; 32]>,
    ecdh_secret: Option<[u8; 32]>,
    pair_verified: bool,
}

impl Default for Pairing {
    fn default() -> Self {
        Self::new()
    }
}

impl Pairing {
    pub fn new() -> Self {
        Self {
            ed_key: SigningKey::generate(&mut OsRng),
            ed_theirs: None,
            ecdh_ours: None,
            ecdh_theirs: None,
            ecdh_secret: None,
            pair_verified: false,
        }
    }

    /// `/pair-setup`: return this device's Ed25519 public key (32 bytes).
    pub fn pair_setup(&self) -> [u8; 32] {
        self.ed_key.verifying_key().to_bytes()
    }

    /// `/pair-verify`:
    /// - flag > 0: read peer ECDH + Ed25519 pubs, return `ecdh_ours || enc(sig)` (96 bytes)
    /// - flag == 0: decrypt/verify peer signature; return empty vec; set `pair_verified`
    pub fn pair_verify(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        if request.len() < 4 {
            return Err(AirPlayError::Pairing(
                "pair_verify request too short".into(),
            ));
        }
        let flag = request[0];
        // bytes 1..4 skipped (Java: request.skip(3))
        let body = &request[4..];

        if flag > 0 {
            self.pair_verify_phase1(body)
        } else {
            self.pair_verify_phase2(body)?;
            Ok(Vec::new())
        }
    }

    pub fn is_pair_verified(&self) -> bool {
        self.pair_verified
    }

    pub fn shared_secret(&self) -> Option<&[u8; 32]> {
        self.ecdh_secret.as_ref()
    }

    /// Inject ECDH shared secret for unit tests that skip pair-verify.
    #[cfg(test)]
    pub fn set_shared_secret_for_test(&mut self, secret: [u8; 32]) {
        self.ecdh_secret = Some(secret);
    }

    fn pair_verify_phase1(&mut self, body: &[u8]) -> Result<Vec<u8>> {
        if body.len() < 64 {
            return Err(AirPlayError::Pairing(
                "pair_verify phase1 body too short".into(),
            ));
        }

        let mut ecdh_theirs = [0u8; 32];
        let mut ed_theirs = [0u8; 32];
        ecdh_theirs.copy_from_slice(&body[..32]);
        ed_theirs.copy_from_slice(&body[32..64]);

        let our_secret = StaticSecret::random_from_rng(OsRng);
        let our_public = X25519PublicKey::from(&our_secret);
        let ecdh_ours = our_public.to_bytes();

        let their_public = X25519PublicKey::from(ecdh_theirs);
        let shared = our_secret.diffie_hellman(&their_public);
        let ecdh_secret = *shared.as_bytes();

        tracing::debug!("Pairing shared secret established");

        self.ed_theirs = Some(ed_theirs);
        self.ecdh_ours = Some(ecdh_ours);
        self.ecdh_theirs = Some(ecdh_theirs);
        self.ecdh_secret = Some(ecdh_secret);
        self.pair_verified = false;

        // sign(ecdh_ours || ecdh_theirs)
        let mut data_to_sign = [0u8; 64];
        data_to_sign[..32].copy_from_slice(&ecdh_ours);
        data_to_sign[32..].copy_from_slice(&ecdh_theirs);
        let signature = self.ed_key.sign(&data_to_sign);
        let mut encrypted_signature = signature.to_bytes();

        // AES-CTR encrypt signature (first 64 keystream bytes)
        let mut cipher = self.init_cipher()?;
        cipher.apply_keystream(&mut encrypted_signature);

        let mut response = Vec::with_capacity(96);
        response.extend_from_slice(&ecdh_ours);
        response.extend_from_slice(&encrypted_signature);
        Ok(response)
    }

    fn pair_verify_phase2(&mut self, body: &[u8]) -> Result<()> {
        if body.len() < 64 {
            return Err(AirPlayError::Pairing(
                "pair_verify phase2 body too short".into(),
            ));
        }

        let ecdh_secret = self.ecdh_secret.ok_or_else(|| {
            AirPlayError::InvalidState("pair_verify phase2 without phase1".into())
        })?;
        let ecdh_ours = self.ecdh_ours.ok_or_else(|| {
            AirPlayError::InvalidState("missing ecdh_ours".into())
        })?;
        let ecdh_theirs = self.ecdh_theirs.ok_or_else(|| {
            AirPlayError::InvalidState("missing ecdh_theirs".into())
        })?;
        let ed_theirs = self.ed_theirs.ok_or_else(|| {
            AirPlayError::InvalidState("missing ed_theirs".into())
        })?;

        let mut signature = [0u8; 64];
        signature.copy_from_slice(&body[..64]);

        // Fresh cipher; advance 64 zero-bytes (Java: update(new byte[64])) then decrypt
        let mut cipher = Self::init_cipher_from_secret(&ecdh_secret)?;
        let mut skip = [0u8; 64];
        cipher.apply_keystream(&mut skip);
        cipher.apply_keystream(&mut signature);

        // verify over ecdh_theirs || ecdh_ours
        let mut sig_message = [0u8; 64];
        sig_message[..32].copy_from_slice(&ecdh_theirs);
        sig_message[32..].copy_from_slice(&ecdh_ours);

        let verifying_key = VerifyingKey::from_bytes(&ed_theirs).map_err(|e| {
            AirPlayError::Pairing(format!("invalid peer Ed25519 public key: {e}"))
        })?;
        let sig = Signature::from_bytes(&signature);

        // Java sets pairVerified from verifyOneShot and does not throw on false.
        self.pair_verified = verifying_key.verify(&sig_message, &sig).is_ok();
        tracing::info!(verified = self.pair_verified, "Pair verified");
        Ok(())
    }

    fn init_cipher(&self) -> Result<Aes128Ctr> {
        let secret = self.ecdh_secret.ok_or_else(|| {
            AirPlayError::InvalidState("no shared secret for cipher".into())
        })?;
        Self::init_cipher_from_secret(&secret)
    }

    fn init_cipher_from_secret(ecdh_secret: &[u8; 32]) -> Result<Aes128Ctr> {
        let mut hasher = Sha512::new();
        hasher.update(b"Pair-Verify-AES-Key");
        hasher.update(ecdh_secret);
        let aes_key: [u8; 16] = hasher.finalize()[..16]
            .try_into()
            .map_err(|_| AirPlayError::Crypto("AES key slice".into()))?;

        let mut hasher = Sha512::new();
        hasher.update(b"Pair-Verify-AES-IV");
        hasher.update(ecdh_secret);
        let aes_iv: [u8; 16] = hasher.finalize()[..16]
            .try_into()
            .map_err(|_| AirPlayError::Crypto("AES IV slice".into()))?;

        Ok(Aes128Ctr::new(&aes_key.into(), &aes_iv.into()))
    }
}
