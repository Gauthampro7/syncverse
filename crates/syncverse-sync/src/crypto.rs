//! ChaCha20-Poly1305 authenticated encryption and room key derivation for Watch-Party 2.0.
//!
//! Provides end-to-end cryptographic confidentiality and tamper-proof message integrity
//! across public WAN links using RFC 8439 ChaCha20-Poly1305 AEAD and SHA-256 key derivation.

use std::sync::atomic::{AtomicU64, Ordering};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
use ring::digest::{digest, SHA256};

use crate::error::SyncError;

/// Tag length in bytes for Poly1305 authentication (16 bytes).
pub const AEAD_TAG_LEN: usize = 16;

/// Nonce length in bytes for ChaCha20-Poly1305 (12 bytes: 4 bytes salt + 8 bytes counter).
pub const AEAD_NONCE_LEN: usize = 12;

/// Cryptographic session managing ChaCha20-Poly1305 AEAD sealing and opening for a watch-party room.
pub struct RoomCryptoSession {
    sealing_key: LessSafeKey,
    salt: [u8; 4],
    packet_counter: AtomicU64,
}

impl RoomCryptoSession {
    /// Derives a 256-bit symmetric AEAD key from a room identifier and password.
    ///
    /// Uses SHA-256 domain-separated key stretching:
    /// `Key = SHA-256("Syncverse-WatchParty-v2:" || room_name || ":" || password)`
    pub fn from_password(room_name: &str, password: &str) -> Result<Self, SyncError> {
        let mut context = Vec::with_capacity(32 + room_name.len() + password.len());
        context.extend_from_slice(b"Syncverse-WatchParty-v2:");
        context.extend_from_slice(room_name.as_bytes());
        context.push(b':');
        context.extend_from_slice(password.as_bytes());

        let key_digest = digest(&SHA256, &context);
        let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key_digest.as_ref())
            .map_err(|_| SyncError::Crypto("Failed to create ChaCha20-Poly1305 unbound key".to_string()))?;

        // Generate 4-byte session salt from key hash prefix
        let salt_digest = digest(&SHA256, key_digest.as_ref());
        let mut salt = [0u8; 4];
        salt.copy_from_slice(&salt_digest.as_ref()[..4]);

        Ok(Self {
            sealing_key: LessSafeKey::new(unbound_key),
            salt,
            packet_counter: AtomicU64::new(1),
        })
    }

    /// Creates a 12-byte nonce from the session salt and counter.
    #[inline]
    fn make_nonce(&self, counter: u64) -> [u8; AEAD_NONCE_LEN] {
        let mut nonce = [0u8; AEAD_NONCE_LEN];
        nonce[..4].copy_from_slice(&self.salt);
        nonce[4..].copy_from_slice(&counter.to_le_bytes());
        nonce
    }

    /// Encrypts and authenticates a plaintext payload in-place using ChaCha20-Poly1305.
    ///
    /// Appends the 16-byte Poly1305 authentication tag and returns `(counter, ciphertext_with_tag)`.
    pub fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<(u64, Vec<u8>), SyncError> {
        let counter = self.packet_counter.fetch_add(1, Ordering::Relaxed);
        let nonce_bytes = self.make_nonce(counter);
        let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes)
            .map_err(|_| SyncError::Crypto("Nonce uniqueness assumption failed".to_string()))?;

        let mut in_out = plaintext.to_vec();
        self.sealing_key
            .seal_in_place_append_tag(nonce, Aad::from(aad), &mut in_out)
            .map_err(|_| SyncError::Crypto("ChaCha20-Poly1305 encryption failed".to_string()))?;

        Ok((counter, in_out))
    }

    /// Decrypts and authenticates a ciphertext buffer using the provided nonce counter and AAD.
    ///
    /// Verifies the Poly1305 authentication tag in constant time. Returns the decrypted plaintext
    /// or `SyncError::Crypto` if verification fails.
    pub fn decrypt(&self, counter: u64, ciphertext_and_tag: &[u8], aad: &[u8]) -> Result<Vec<u8>, SyncError> {
        if ciphertext_and_tag.len() < AEAD_TAG_LEN {
            return Err(SyncError::Crypto("Ciphertext too short for Poly1305 tag".to_string()));
        }

        let nonce_bytes = self.make_nonce(counter);
        let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes)
            .map_err(|_| SyncError::Crypto("Nonce uniqueness assumption failed".to_string()))?;

        let mut in_out = ciphertext_and_tag.to_vec();
        let decrypted_slice = self
            .sealing_key
            .open_in_place(nonce, Aad::from(aad), &mut in_out)
            .map_err(|_| SyncError::Crypto("ChaCha20-Poly1305 authentication failed: invalid tag or corrupted ciphertext".to_string()))?;

        Ok(decrypted_slice.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_round_trip() {
        let session = RoomCryptoSession::from_password("anime-night", "secret123").unwrap();
        let plaintext = b"Hello, synchronized watch party!";
        let aad = b"HEADER_AAD";

        let (counter, ciphertext) = session.encrypt(plaintext, aad).unwrap();
        assert_ne!(plaintext.as_slice(), ciphertext.as_slice());
        assert_eq!(ciphertext.len(), plaintext.len() + AEAD_TAG_LEN);

        let decrypted = session.decrypt(counter, &ciphertext, aad).unwrap();
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_crypto_wrong_password_fails() {
        let session1 = RoomCryptoSession::from_password("anime-night", "correct_password").unwrap();
        let session2 = RoomCryptoSession::from_password("anime-night", "wrong_password").unwrap();

        let plaintext = b"Confidential playback state";
        let aad = b"AAD";

        let (counter, ciphertext) = session1.encrypt(plaintext, aad).unwrap();
        let decrypt_res = session2.decrypt(counter, &ciphertext, aad);
        assert!(decrypt_res.is_err(), "Decryption with wrong password must fail authentication");
    }

    #[test]
    fn test_crypto_tampered_ciphertext_fails() {
        let session = RoomCryptoSession::from_password("room", "pass").unwrap();
        let plaintext = b"Unmodified message";
        let aad = b"AAD";

        let (counter, mut ciphertext) = session.encrypt(plaintext, aad).unwrap();
        // Flip a bit in the ciphertext
        ciphertext[0] ^= 0x01;

        let res = session.decrypt(counter, &ciphertext, aad);
        assert!(res.is_err(), "Tampered ciphertext must fail Poly1305 authentication");
    }
}
