//! Secure storage for encrypted offline data
//!
//! Provides encrypted persistence for sensitive data using AES-256-GCM

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const NONCE_SIZE: usize = 12; // AES-GCM nonce size
const MAGIC_HEADER: &[u8] = b"PNET"; // Magic header to identify encrypted files
const MAGIC_HEADER_SIZE: usize = 4;

/// Secure storage manager
pub struct SecureStorage {
    storage_dir: PathBuf,
    encryption_key: String,
}

impl SecureStorage {
    /// Create a new secure storage instance.
    /// `encryption_key` is the raw key string (hashed with SHA-256 internally to produce 32 bytes).
    /// Falls back to the `POLLINET_ENCRYPTION_KEY` environment variable when `encryption_key` is `None`.
    pub fn new(storage_dir: impl AsRef<Path>, encryption_key: Option<String>) -> Result<Self, StorageError> {
        let storage_dir = storage_dir.as_ref().to_path_buf();

        let key = encryption_key
            .or_else(|| env::var("POLLINET_ENCRYPTION_KEY").ok())
            .ok_or_else(|| {
                StorageError::Encryption(
                    "POLLINET_ENCRYPTION_KEY must be set — no insecure fallback allowed"
                        .to_string(),
                )
            })?;

        // Create directory if it doesn't exist
        if !storage_dir.exists() {
            fs::create_dir_all(&storage_dir).map_err(|e| {
                StorageError::Io(format!("Failed to create storage directory: {}", e))
            })?;
        }

        tracing::info!(
            "📁 Initialized secure storage at: {}",
            storage_dir.display()
        );

        Ok(Self { storage_dir, encryption_key: key })
    }

    /// Derive AES-256-GCM key from the stored encryption key string via SHA-256.
    fn get_encryption_key(&self) -> Result<Key<Aes256Gcm>, StorageError> {
        // Derive 256-bit key from the string using SHA-256
        // This ensures we always have exactly 32 bytes for AES-256-GCM
        let mut hasher = Sha256::new();
        hasher.update(self.encryption_key.as_bytes());
        let key_bytes = hasher.finalize();
        Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
    }

    /// Encrypt data using AES-256-GCM
    fn encrypt_data(&self, plaintext: &[u8]) -> Result<Vec<u8>, StorageError> {
        let key = self.get_encryption_key()?;
        let cipher = Aes256Gcm::new(&key);

        // Generate random nonce
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        // Encrypt the data
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|e| StorageError::Encryption(format!("Encryption failed: {}", e)))?;

        // Format: [MAGIC_HEADER][NONCE][CIPHERTEXT]
        let mut encrypted = Vec::with_capacity(MAGIC_HEADER_SIZE + NONCE_SIZE + ciphertext.len());
        encrypted.extend_from_slice(MAGIC_HEADER);
        encrypted.extend_from_slice(&nonce);
        encrypted.extend_from_slice(&ciphertext);

        Ok(encrypted)
    }

    /// Decrypt data using AES-256-GCM
    fn decrypt_data(&self, encrypted: &[u8]) -> Result<Vec<u8>, StorageError> {
        // Check minimum size
        if encrypted.len() < MAGIC_HEADER_SIZE + NONCE_SIZE {
            return Err(StorageError::Decryption(
                "Encrypted data too short".to_string(),
            ));
        }

        // Check magic header
        if &encrypted[..MAGIC_HEADER_SIZE] != MAGIC_HEADER {
            return Err(StorageError::Decryption(
                "Invalid magic header - file may not be encrypted".to_string(),
            ));
        }

        let key = self.get_encryption_key()?;
        let cipher = Aes256Gcm::new(&key);

        // Extract nonce and ciphertext
        let nonce_start = MAGIC_HEADER_SIZE;
        let nonce_end = nonce_start + NONCE_SIZE;
        let nonce = Nonce::from_slice(&encrypted[nonce_start..nonce_end]);
        let ciphertext = &encrypted[nonce_end..];

        // Decrypt the data
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| StorageError::Decryption(format!("Decryption failed: {}", e)))?;

        Ok(plaintext)
    }

}

/// Storage errors
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption error: {0}")]
    Decryption(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const TEST_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn test_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = SecureStorage::new(temp_dir.path(), Some(TEST_KEY.to_string())).unwrap();
        assert!(storage.storage_dir.exists());
    }
}
