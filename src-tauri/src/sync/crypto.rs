//! Encryption for everything the phone and the PC send each other: XChaCha20-Poly1305 with the
//! 32-byte key made when they paired. A message is a random 24-byte nonce followed by the
//! ciphertext. The "label" (which request or reply it is) is authenticated too, so a message
//! can't be replayed as a different one.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

pub type Key = [u8; KEY_LEN];

pub fn new_key() -> Key {
    let mut k = [0u8; KEY_LEN];
    getrandom::fill(&mut k).expect("the system can make random numbers");
    k
}

pub fn key_to_text(k: &Key) -> String {
    URL_SAFE_NO_PAD.encode(k)
}

pub fn key_from_text(s: &str) -> Option<Key> {
    URL_SAFE_NO_PAD.decode(s.trim()).ok()?.try_into().ok()
}

pub fn seal(key: &Key, label: &str, plain: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).expect("the system can make random numbers");
    let sealed = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: label.as_bytes() })
        .expect("encrypting in memory can't fail");
    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    out
}

/// None if the message wasn't made with this key and label (or was changed on the way).
pub fn open(key: &Key, label: &str, msg: &[u8]) -> Option<Vec<u8>> {
    if msg.len() < NONCE_LEN {
        return None;
    }
    let (nonce, sealed) = msg.split_at(NONCE_LEN);
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: sealed, aad: label.as_bytes() })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tamper() {
        let k = new_key();
        let msg = seal(&k, "sync", b"hello");
        assert_eq!(open(&k, "sync", &msg).unwrap(), b"hello");
        assert!(open(&k, "pair", &msg).is_none(), "another label");
        assert!(open(&new_key(), "sync", &msg).is_none(), "another key");
        let mut bad = msg.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(open(&k, "sync", &bad).is_none(), "changed on the way");
        assert_ne!(seal(&k, "sync", b"hello"), msg, "a fresh nonce each time");
        assert_eq!(key_from_text(&key_to_text(&k)), Some(k));
        assert_eq!(key_from_text("short"), None);
    }
}
