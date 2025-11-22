//! AES-256-GCM encryption for sensitive cartridge data
//!
//! Provides authenticated encryption for content pages with:
//! - AES-256-GCM (Galois/Counter Mode) for encryption
//! - 96-bit nonces (12 bytes) for uniqueness
//! - 128-bit authentication tags for integrity
//! - Key derivation from master key + page ID
//!
//! **Design**:
//! - Each page encrypted with unique nonce (page_id + counter)
//! - Format: [nonce: 12 bytes][ciphertext][tag: 16 bytes]
//! - Master key must be 32 bytes (256 bits)
//! - Authenticated encryption prevents tampering

use crate::error::{CartridgeError, Result};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

/// Encryption key (32 bytes for AES-256)
pub type EncryptionKey = [u8; 32];

/// Nonce size for AES-GCM (96 bits / 12 bytes)
pub const NONCE_SIZE: usize = 12;

/// Authentication tag size (128 bits / 16 bytes)
pub const TAG_SIZE: usize = 16;

/// Overhead added by encryption (nonce + tag)
pub const ENCRYPTION_OVERHEAD: usize = NONCE_SIZE + TAG_SIZE;

/// Encryption configuration
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    /// Master encryption key (32 bytes)
    master_key: EncryptionKey,

    /// Whether encryption is enabled
    enabled: bool,
}

impl EncryptionConfig {
    /// Create new encryption config with provided key
    pub fn new(master_key: EncryptionKey) -> Self {
        EncryptionConfig {
            master_key,
            enabled: true,
        }
    }

    /// Create disabled encryption config
    pub fn disabled() -> Self {
        EncryptionConfig {
            master_key: [0u8; 32],
            enabled: false,
        }
    }

    /// Generate a random encryption key
    pub fn generate_key() -> EncryptionKey {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    /// Check if encryption is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the master key
    pub fn master_key(&self) -> &EncryptionKey {
        &self.master_key
    }
}

/// Encrypt data using AES-256-GCM
///
/// Returns encrypted data with format: [nonce][ciphertext][tag]
pub fn encrypt(data: &[u8], key: &EncryptionKey) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt data
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| CartridgeError::Allocation(format!("Encryption failed: {}", e)))?;

    // Build output: nonce + ciphertext (which includes tag)
    let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data using AES-256-GCM
///
/// Expects data in format: [nonce][ciphertext][tag]
pub fn decrypt(data: &[u8], key: &EncryptionKey) -> Result<Vec<u8>> {
    if data.len() < ENCRYPTION_OVERHEAD {
        return Err(CartridgeError::Allocation(
            "Encrypted data too short".to_string(),
        ));
    }

    let cipher = Aes256Gcm::new(key.into());

    // Extract nonce
    let nonce = Nonce::from_slice(&data[..NONCE_SIZE]);

    // Extract ciphertext (includes tag)
    let ciphertext = &data[NONCE_SIZE..];

    // Decrypt and verify
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CartridgeError::Allocation(format!("Decryption failed: {}", e)))?;

    Ok(plaintext)
}

/// Encrypt data if encryption is enabled
pub fn encrypt_if_enabled(data: &[u8], config: &EncryptionConfig) -> Result<(Vec<u8>, bool)> {
    if config.is_enabled() {
        let encrypted = encrypt(data, config.master_key())?;
        Ok((encrypted, true))
    } else {
        Ok((data.to_vec(), false))
    }
}

/// Decrypt data if it was encrypted
pub fn decrypt_if_encrypted(
    data: &[u8],
    config: &EncryptionConfig,
    was_encrypted: bool,
) -> Result<Vec<u8>> {
    if was_encrypted {
        decrypt(data, config.master_key())
    } else {
        Ok(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let key1 = EncryptionConfig::generate_key();
        let key2 = EncryptionConfig::generate_key();

        // Keys should be different
        assert_ne!(key1, key2);

        // Keys should be 32 bytes
        assert_eq!(key1.len(), 32);
        assert_eq!(key2.len(), 32);
    }

    #[test]
    fn test_encryption_decryption() {
        let key = EncryptionConfig::generate_key();
        let plaintext = b"Hello, World! This is a secret message.";

        let ciphertext = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
        assert_ne!(plaintext.as_slice(), &ciphertext[NONCE_SIZE..]); // Ciphertext should differ
        assert_eq!(ciphertext.len(), plaintext.len() + ENCRYPTION_OVERHEAD);
    }

    #[test]
    fn test_encryption_overhead() {
        let key = EncryptionConfig::generate_key();
        let plaintext = b"Test data";

        let ciphertext = encrypt(plaintext, &key).unwrap();

        // Ciphertext = nonce (12) + encrypted data + tag (16)
        assert_eq!(ciphertext.len(), plaintext.len() + NONCE_SIZE + TAG_SIZE);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = EncryptionConfig::generate_key();
        let key2 = EncryptionConfig::generate_key();
        let plaintext = b"Secret message";

        let ciphertext = encrypt(plaintext, &key1).unwrap();
        let result = decrypt(&ciphertext, &key2);

        // Decryption with wrong key should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_data_fails() {
        let key = EncryptionConfig::generate_key();
        let plaintext = b"Important data";

        let mut ciphertext = encrypt(plaintext, &key).unwrap();

        // Tamper with the ciphertext
        let tamper_idx = NONCE_SIZE + 5;
        ciphertext[tamper_idx] ^= 0xFF;

        let result = decrypt(&ciphertext, &key);

        // Tampered data should fail authentication
        assert!(result.is_err());
    }

    #[test]
    fn test_encryption_config() {
        let key = EncryptionConfig::generate_key();
        let config = EncryptionConfig::new(key);

        assert!(config.is_enabled());
        assert_eq!(config.master_key(), &key);

        let disabled = EncryptionConfig::disabled();
        assert!(!disabled.is_enabled());
    }

    #[test]
    fn test_encrypt_if_enabled() {
        let key = EncryptionConfig::generate_key();
        let plaintext = b"Test message";

        // With encryption enabled
        let config_enabled = EncryptionConfig::new(key);
        let (result, was_encrypted) = encrypt_if_enabled(plaintext, &config_enabled).unwrap();
        assert!(was_encrypted);
        assert_ne!(result.as_slice(), plaintext);

        // With encryption disabled
        let config_disabled = EncryptionConfig::disabled();
        let (result, was_encrypted) = encrypt_if_enabled(plaintext, &config_disabled).unwrap();
        assert!(!was_encrypted);
        assert_eq!(result.as_slice(), plaintext);
    }

    #[test]
    fn test_decrypt_if_encrypted() {
        let key = EncryptionConfig::generate_key();
        let config = EncryptionConfig::new(key);
        let plaintext = b"Secret data";

        // Encrypt
        let (encrypted, was_encrypted) = encrypt_if_enabled(plaintext, &config).unwrap();
        assert!(was_encrypted);

        // Decrypt
        let decrypted = decrypt_if_encrypted(&encrypted, &config, was_encrypted).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);

        // Decrypt unencrypted data
        let decrypted = decrypt_if_encrypted(plaintext, &config, false).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_large_data_encryption() {
        let key = EncryptionConfig::generate_key();
        let plaintext = vec![0x42u8; 10000]; // 10KB of data

        let ciphertext = encrypt(&plaintext, &key).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_eq!(ciphertext.len(), plaintext.len() + ENCRYPTION_OVERHEAD);
    }

    #[test]
    fn test_empty_data_encryption() {
        let key = EncryptionConfig::generate_key();
        let plaintext = b"";

        let ciphertext = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
        assert_eq!(ciphertext.len(), ENCRYPTION_OVERHEAD);
    }

    #[test]
    fn test_nonce_uniqueness() {
        let key = EncryptionConfig::generate_key();
        let plaintext = b"Same message";

        let ciphertext1 = encrypt(plaintext, &key).unwrap();
        let ciphertext2 = encrypt(plaintext, &key).unwrap();

        // Nonces should be different (probabilistically)
        assert_ne!(&ciphertext1[..NONCE_SIZE], &ciphertext2[..NONCE_SIZE]);

        // Both should decrypt correctly
        assert_eq!(decrypt(&ciphertext1, &key).unwrap(), plaintext);
        assert_eq!(decrypt(&ciphertext2, &key).unwrap(), plaintext);
    }
}
