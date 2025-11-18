//! Secure storage for nonce bundles and offline transaction data
//!
//! Provides encrypted persistence for sensitive nonce account data

use crate::transaction::OfflineTransactionBundle;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::env;
use thiserror::Error;

const BUNDLE_FILENAME: &str = "pollinet_nonce_bundle.json";
const NONCE_SIZE: usize = 12; // AES-GCM nonce size
const MAGIC_HEADER: &[u8] = b"PNET"; // Magic header to identify encrypted files
const MAGIC_HEADER_SIZE: usize = 4;

/// Secure storage manager for nonce bundles
pub struct SecureStorage {
    storage_dir: PathBuf,
}

impl SecureStorage {
    /// Create a new secure storage instance
    pub fn new(storage_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let storage_dir = storage_dir.as_ref().to_path_buf();

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

        Ok(Self { storage_dir })
    }

    /// Get the path to the nonce bundle file
    fn bundle_path(&self) -> PathBuf {
        self.storage_dir.join(BUNDLE_FILENAME)
    }

    /// Derive encryption key from environment variable or use default
    fn get_encryption_key() -> Key<Aes256Gcm> {
        let key_str = env::var("POLLINET_ENCRYPTION_KEY")
            .unwrap_or_else(|_| "pollinet-default-encryption-key".to_string());
        
        // Derive 256-bit key from the string using SHA-256
        // This ensures we always have exactly 32 bytes for AES-256-GCM
        let mut hasher = Sha256::new();
        hasher.update(key_str.as_bytes());
        let key_bytes = hasher.finalize();
        *Key::<Aes256Gcm>::from_slice(&key_bytes)
    }

    /// Encrypt data using AES-256-GCM
    fn encrypt_data(&self, plaintext: &[u8]) -> Result<Vec<u8>, StorageError> {
        let key = Self::get_encryption_key();
        let cipher = Aes256Gcm::new(&key);
        
        // Generate random nonce
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        
        // Encrypt the data
        let ciphertext = cipher.encrypt(&nonce, plaintext.as_ref())
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
                "Encrypted data too short".to_string()
            ));
        }
        
        // Check magic header
        if &encrypted[..MAGIC_HEADER_SIZE] != MAGIC_HEADER {
            return Err(StorageError::Decryption(
                "Invalid magic header - file may not be encrypted".to_string()
            ));
        }
        
        let key = Self::get_encryption_key();
        let cipher = Aes256Gcm::new(&key);
        
        // Extract nonce and ciphertext
        let nonce_start = MAGIC_HEADER_SIZE;
        let nonce_end = nonce_start + NONCE_SIZE;
        let nonce = Nonce::from_slice(&encrypted[nonce_start..nonce_end]);
        let ciphertext = &encrypted[nonce_end..];
        
        // Decrypt the data
        let plaintext = cipher.decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| StorageError::Decryption(format!("Decryption failed: {}", e)))?;
        
        Ok(plaintext)
    }

    /// Save nonce bundle to secure storage
    pub fn save_bundle(&self, bundle: &OfflineTransactionBundle) -> Result<(), StorageError> {
        let path = self.bundle_path();

        // Serialize bundle to JSON
        let json = serde_json::to_string_pretty(bundle).map_err(|e| {
            StorageError::Serialization(format!("Failed to serialize bundle: {}", e))
        })?;

        // Encrypt the JSON data
        let encrypted_data = self.encrypt_data(json.as_bytes())?;

        // Write encrypted data to file
        fs::write(&path, encrypted_data)
            .map_err(|e| StorageError::Io(format!("Failed to write bundle: {}", e)))?;

        tracing::info!("💾 Saved encrypted nonce bundle to: {}", path.display());
        tracing::debug!(
            "   Total nonces: {}, Available: {}",
            bundle.nonce_caches.len(),
            bundle.available_nonces()
        );

        Ok(())
    }

    /// Load nonce bundle from secure storage
    pub fn load_bundle(&self) -> Result<Option<OfflineTransactionBundle>, StorageError> {
        let path = self.bundle_path();

        if !path.exists() {
            tracing::debug!("📂 No existing bundle found at: {}", path.display());
            return Ok(None);
        }

        // Read encrypted data from file
        let encrypted_data = fs::read(&path)
            .map_err(|e| StorageError::Io(format!("Failed to read bundle: {}", e)))?;

        // Check if file is encrypted (has magic header) or plain JSON (backward compatibility)
        let json = if encrypted_data.len() >= MAGIC_HEADER_SIZE 
            && &encrypted_data[..MAGIC_HEADER_SIZE] == MAGIC_HEADER {
            // File is encrypted, decrypt it
            let decrypted_bytes = self.decrypt_data(&encrypted_data)?;
            String::from_utf8(decrypted_bytes)
                .map_err(|e| StorageError::Decryption(format!("Invalid UTF-8 after decryption: {}", e)))?
        } else {
            // File is plain JSON (backward compatibility with old unencrypted files)
            tracing::warn!("⚠️  Loading unencrypted bundle file (backward compatibility mode)");
            String::from_utf8(encrypted_data)
                .map_err(|e| StorageError::Io(format!("Failed to read bundle as UTF-8: {}", e)))?
        };

        // Deserialize bundle
        let bundle: OfflineTransactionBundle = serde_json::from_str(&json).map_err(|e| {
            StorageError::Serialization(format!("Failed to deserialize bundle: {}", e))
        })?;

        tracing::info!("📂 Loaded nonce bundle from: {}", path.display());
        tracing::debug!(
            "   Total nonces: {}, Available: {}",
            bundle.nonce_caches.len(),
            bundle.available_nonces()
        );

        Ok(Some(bundle))
    }

    /// Delete the stored bundle
    pub fn delete_bundle(&self) -> Result<(), StorageError> {
        let path = self.bundle_path();

        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| StorageError::Io(format!("Failed to delete bundle: {}", e)))?;
            tracing::info!("🗑️  Deleted nonce bundle from: {}", path.display());
        }

        Ok(())
    }

    /// Check if a bundle exists in storage
    pub fn bundle_exists(&self) -> bool {
        self.bundle_path().exists()
    }

    /// Get bundle file info (size, age, etc.)
    pub fn get_bundle_info(&self) -> Option<BundleInfo> {
        let path = self.bundle_path();

        if !path.exists() {
            return None;
        }

        let metadata = fs::metadata(&path).ok()?;
        let modified = metadata.modified().ok()?;
        let age_secs = modified.elapsed().ok()?.as_secs();

        Some(BundleInfo {
            path: path.to_string_lossy().to_string(),
            size_bytes: metadata.len(),
            age_seconds: age_secs,
        })
    }
}

/// Information about a stored bundle
#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub path: String,
    pub size_bytes: u64,
    pub age_seconds: u64,
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
    use crate::transaction::CachedNonceData;
    use tempfile::TempDir;

    #[test]
    fn test_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = SecureStorage::new(temp_dir.path()).unwrap();
        assert!(!storage.bundle_exists());
    }

    #[test]
    fn test_save_and_load_bundle() {
        let temp_dir = TempDir::new().unwrap();
        let storage = SecureStorage::new(temp_dir.path()).unwrap();

        // Create a test bundle
        let bundle = OfflineTransactionBundle {
            nonce_caches: vec![CachedNonceData {
                nonce_account: "test_account".to_string(),
                authority: "test_authority".to_string(),
                blockhash: "test_hash".to_string(),
                lamports_per_signature: 5000,
                cached_at: 1234567890,
                used: false,
            }],
            max_transactions: 1,
            created_at: 1234567890,
        };

        // Save bundle
        storage.save_bundle(&bundle).unwrap();
        assert!(storage.bundle_exists());

        // Load bundle
        let loaded = storage.load_bundle().unwrap().unwrap();
        assert_eq!(loaded.nonce_caches.len(), 1);
        assert_eq!(loaded.nonce_caches[0].nonce_account, "test_account");
    }
}
