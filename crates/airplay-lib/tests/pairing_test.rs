//! Integration test ported from Java `AirPlayPairingTest.pairingTest`.

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use airplay_lib::AirPlay;
use ctr::Ctr128BE;
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use sha2::{Digest, Sha512};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

type Aes128Ctr = Ctr128BE<Aes128>;

#[test]
fn pairing_test() {
    let mut airplay = AirPlay::new();

    // /pair-setup
    let public_key = airplay.pair_setup();
    assert_eq!(public_key.len(), 32);

    // /pair-verify phase 1 (flag > 0): client Curve25519 + Ed25519 public keys
    let client_ed = SigningKey::generate(&mut OsRng);
    let client_curve_secret = StaticSecret::random_from_rng(OsRng);
    let client_curve_pub = X25519PublicKey::from(&client_curve_secret);

    let mut pair_verify1 = Vec::with_capacity(68);
    pair_verify1.extend_from_slice(&[1, 0, 0, 0]);
    pair_verify1.extend_from_slice(client_curve_pub.as_bytes());
    pair_verify1.extend_from_slice(client_ed.verifying_key().as_bytes());

    let pair_verify_response = airplay
        .pair_verify(&pair_verify1)
        .expect("pair_verify phase 1");
    assert_eq!(pair_verify_response.len(), 96);

    // Derive shared secret and AES-CTR key/IV (mirror Java test client)
    let atv_curve_pub_bytes: [u8; 32] = pair_verify_response[..32]
        .try_into()
        .expect("atv curve pub");
    let atv_curve_pub = X25519PublicKey::from(atv_curve_pub_bytes);
    let shared_secret = client_curve_secret.diffie_hellman(&atv_curve_pub);
    let ecdh_secret = shared_secret.as_bytes();

    let mut hasher = Sha512::new();
    hasher.update(b"Pair-Verify-AES-Key");
    hasher.update(ecdh_secret);
    let aes_key: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();

    let mut hasher = Sha512::new();
    hasher.update(b"Pair-Verify-AES-IV");
    hasher.update(ecdh_secret);
    let aes_iv: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();

    // Advance CTR with server's encrypted signature (bytes 32..96), then encrypt client sig
    let mut cipher = Aes128Ctr::new(&aes_key.into(), &aes_iv.into());
    let mut server_enc_sig = pair_verify_response[32..96].to_vec();
    cipher.apply_keystream(&mut server_enc_sig);

    let mut data_to_sign = [0u8; 64];
    data_to_sign[..32].copy_from_slice(client_curve_pub.as_bytes());
    data_to_sign[32..].copy_from_slice(&atv_curve_pub_bytes);
    let client_sig = client_ed.sign(&data_to_sign);
    let mut encrypted_client_sig = client_sig.to_bytes();
    cipher.apply_keystream(&mut encrypted_client_sig);

    // /pair-verify phase 2 (flag == 0)
    let mut pair_verify2 = Vec::with_capacity(68);
    pair_verify2.extend_from_slice(&[0, 0, 0, 0]);
    pair_verify2.extend_from_slice(&encrypted_client_sig);

    let phase2 = airplay.pair_verify(&pair_verify2).expect("pair_verify phase 2");
    assert!(phase2.is_empty());
    assert!(airplay.is_pair_verified());
}
