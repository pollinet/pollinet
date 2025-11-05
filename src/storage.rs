//! Secure storage for nonce bundles and offline transaction data
//!
//! Provides encrypted persistence for sensitive nonce account data

use std::path::{Path, PathBuf};
use std::fs;
use thiserror::Error;
use crate::transaction::{OfflineTransactionBundle, CachedNonceData};

const BUNDLE_FILENAME: &str = "pollinet_nonce_bundle.json";
const BUNDLE_VERSION: u32 = 1;

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
            fs::create_dir_all(&storage_dir)
                .map_err(|e| StorageError::Io(format!("Failed to create storage directory: {}", e)))?;
        }
        
        tracing::info!("📁 Initialized secure storage at: {}", storage_dir.display());
        
        Ok(Self { storage_dir })
    }
    
    /// Get the path to the nonce bundle file
    fn bundle_path(&self) -> PathBuf {
        self.storage_dir.join(BUNDLE_FILENAME)
    }
    
    /// Save nonce bundle to secure storage
    pub fn save_bundle(&self, bundle: &OfflineTransactionBundle) -> Result<(), StorageError> {
        let path = self.bundle_path();
        
        // Serialize bundle to JSON
        let json = serde_json::to_string_pretty(bundle)
            .map_err(|e| StorageError::Serialization(format!("Failed to serialize bundle: {}", e)))?;
        
        // TODO: Add encryption here for production use
        // For now, we'll save as plain JSON for demo purposes
        // In production, use platform keystore to encrypt the data
        
        // Write to file
        fs::write(&path, json)
            .map_err(|e| StorageError::Io(format!("Failed to write bundle: {}", e)))?;
        
        tracing::info!("💾 Saved nonce bundle to: {}", path.display());
        tracing::debug!("   Total nonces: {}, Available: {}", bundle.nonce_caches.len(), bundle.available_nonces());
        
        Ok(())
    }
    
    /// Load nonce bundle from secure storage
    pub fn load_bundle(&self) -> Result<Option<OfflineTransactionBundle>, StorageError> {
        let path = self.bundle_path();
        
        if !path.exists() {
            tracing::debug!("📂 No existing bundle found at: {}", path.display());
            return Ok(None);
        }
        
        // Read from file
        let json = fs::read_to_string(&path)
            .map_err(|e| StorageError::Io(format!("Failed to read bundle: {}", e)))?;
        
        // TODO: Add decryption here for production use
        
        // Deserialize bundle
        let bundle: OfflineTransactionBundle = serde_json::from_str(&json)
            .map_err(|e| StorageError::Serialization(format!("Failed to deserialize bundle: {}", e)))?;
        
        tracing::info!("📂 Loaded nonce bundle from: {}", path.display());
        tracing::debug!("   Total nonces: {}, Available: {}", bundle.nonce_caches.len(), bundle.available_nonces());
        
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

