//! Authenticated encryption for OAuth credentials at rest.
//!
//! Uses AES-256-GCM with a random 96-bit nonce per message. The stored blob is
//! `base64(nonce || ciphertext||tag)`, so it is self-contained and tamper-evident
//! (GCM's auth tag fails `open` if the ciphertext or nonce is modified). The 32-byte
//! key comes from `TOKEN_ENCRYPTION_KEY` (see [`crate::config`]). Plaintext is never
//! logged or persisted.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

/// AES-GCM standard nonce length (96 bits).
const NONCE_LEN: usize = 12;

/// Encrypt `plaintext` and return `base64(nonce || ciphertext||tag)`.
pub fn seal(key: &[u8; 32], plaintext: &str) -> Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("AES-GCM encryption failed: {e}"))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(nonce.as_slice());
    blob.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(blob))
}

/// Decrypt a blob produced by [`seal`]. Fails on a wrong key, a truncated blob,
/// or any tampering (GCM tag mismatch).
pub fn open(key: &[u8; 32], b64: &str) -> Result<String> {
    let blob = BASE64.decode(b64).context("invalid base64 ciphertext")?;
    if blob.len() < NONCE_LEN {
        return Err(anyhow!("ciphertext too short"));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow!("AES-GCM decryption/authentication failed"))?;
    String::from_utf8(plaintext).context("decrypted bytes are not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        // Deterministic non-zero test key.
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(1);
        }
        k
    }

    #[test]
    fn round_trips() {
        let k = key();
        let secret = "rt_abc123-this-is-a-refresh-token";
        let sealed = seal(&k, secret).unwrap();
        assert_ne!(sealed, secret, "stored value must not be plaintext");
        assert_eq!(open(&k, &sealed).unwrap(), secret);
    }

    #[test]
    fn distinct_nonces_produce_distinct_ciphertext() {
        let k = key();
        let a = seal(&k, "same-plaintext").unwrap();
        let b = seal(&k, "same-plaintext").unwrap();
        assert_ne!(a, b, "random nonce should make ciphertexts differ");
    }

    #[test]
    fn wrong_key_fails() {
        let sealed = seal(&key(), "secret").unwrap();
        let mut other = key();
        other[0] ^= 0xFF;
        assert!(open(&other, &sealed).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let k = key();
        let sealed = seal(&k, "secret").unwrap();
        let mut raw = BASE64.decode(&sealed).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01; // flip a bit in the auth tag / ciphertext
        let tampered = BASE64.encode(raw);
        assert!(open(&k, &tampered).is_err());
    }

    #[test]
    fn truncated_blob_fails() {
        let k = key();
        assert!(open(&k, &BASE64.encode([0u8; 4])).is_err());
    }
}
