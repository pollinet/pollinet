//! PolliNet SDK - Decentralized Solana transaction propagation over BLE mesh networks
//!
//! This SDK enables offline Solana transactions to be distributed opportunistically
//! over Bluetooth Low Energy (BLE) mesh networks, inspired by biological pollination.

pub mod ble;
pub mod intent;
pub mod queue;
pub mod storage;
pub mod submission;
pub mod util;

#[cfg(feature = "android")]
pub mod ffi;

use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Core PolliNet SDK instance
pub struct PolliNetSDK {
    /// Local transaction cache (used by cleanupStaleFragments FFI)
    pub local_cache: Arc<RwLock<ble::fragmenter::TransactionCache>>,
    /// Queue manager for all queue operations
    queue_manager: Arc<queue::QueueManager>,
}

impl PolliNetSDK {
    fn make_queue_manager(storage_dir: Option<&str>) -> Arc<queue::QueueManager> {
        if let Some(dir) = storage_dir {
            tracing::info!("Using persistent queue storage: {}", dir);
            Arc::new(queue::QueueManager::with_storage(dir).unwrap_or_else(|e| {
                tracing::warn!("Failed to load queues from storage: {}, starting fresh", e);
                queue::QueueManager::new()
            }))
        } else {
            tracing::info!("No persistent storage configured, queues will not persist");
            Arc::new(queue::QueueManager::new())
        }
    }

    /// Initialize a new PolliNet SDK instance without RPC client
    pub async fn new() -> Result<Self, PolliNetError> {
        Ok(Self {
            local_cache: Arc::new(RwLock::new(ble::fragmenter::TransactionCache::new())),
            queue_manager: Self::make_queue_manager(None),
        })
    }

    /// Initialize a new PolliNet SDK instance (RPC URL param reserved for future use)
    pub async fn new_with_rpc(_rpc_url: &str) -> Result<Self, PolliNetError> {
        Ok(Self {
            local_cache: Arc::new(RwLock::new(ble::fragmenter::TransactionCache::new())),
            queue_manager: Self::make_queue_manager(None),
        })
    }

    // =========================================================================
    // Queue Management Methods (Phase 2)
    // =========================================================================

    /// Get queue manager reference
    pub fn queue_manager(&self) -> &Arc<queue::QueueManager> {
        &self.queue_manager
    }

    /// Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
    pub async fn clear_all_queues(&self) -> Result<(), PolliNetError> {
        // Clear queue manager queues
        self.queue_manager.clear_all_queues().await;

        tracing::info!("✅ Cleared all queues via SDK");
        Ok(())
    }

    /// Get queue metrics
    pub async fn get_queue_metrics(&self) -> queue::QueueMetrics {
        self.queue_manager.get_metrics().await
    }

    /// Get queue health status
    pub async fn get_queue_health(&self) -> queue::HealthStatus {
        self.queue_manager.get_health().await
    }

    /// Accept and queue a pre-signed transaction from external partners
    ///
    /// This method is designed for accepting transactions from external partners.
    /// It verifies the transaction is properly signed, compresses it if needed,
    /// fragments it for BLE transmission, and adds it to the outbound queue for relay.
    ///
    /// # Arguments
    /// * `base64_signed_tx` - Base64-encoded pre-signed Solana transaction
    /// * `max_payload` - Optional maximum payload size (typically MTU - 10). If None, uses default.
    ///
    /// # Returns
    /// Transaction ID (SHA-256 hash as hex string) for tracking
    ///
    /// # Errors
    /// Returns error if:
    /// - Transaction is not properly signed
    /// - Transaction verification fails
    /// - Compression fails
    /// - Fragmentation fails
    /// - Queue is full
    pub async fn accept_and_queue_external_transaction(
        &self,
        base64_signed_tx: &str,
        max_payload: Option<usize>,
    ) -> Result<String, PolliNetError> {
        use crate::ble::fragmenter;
        use crate::queue::{OutboundTransaction, Priority};
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        use sha2::{Digest, Sha256};

        tracing::info!("📥 Accepting external pre-signed transaction for relay");

        // Decode from base64
        let tx_bytes = BASE64.decode(base64_signed_tx).map_err(|e| {
            PolliNetError::Serialization(format!("Failed to decode base64: {}", e))
        })?;

        tracing::info!("Decoded transaction: {} bytes", tx_bytes.len());

        // Deserialize and verify transaction
        let tx: solana_sdk::transaction::Transaction =
            bincode1::deserialize(&tx_bytes).map_err(|e| {
                PolliNetError::Serialization(format!("Failed to deserialize transaction: {}", e))
            })?;

        // Verify transaction has valid signatures
        let valid_sigs = tx
            .signatures
            .iter()
            .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
            .count();

        if valid_sigs == 0 {
            return Err(PolliNetError::Serialization(
                "Transaction must be signed before queuing for relay".to_string(),
            ));
        }

        // Verify transaction signatures
        if let Err(err) = tx.verify() {
            tracing::error!("❌ Transaction signature verification failed: {}", err);
            return Err(PolliNetError::Serialization(format!(
                "Transaction signature verification failed: {}",
                err
            )));
        }

        tracing::info!(
            "✅ Transaction verified: {}/{} valid signatures",
            valid_sigs,
            tx.signatures.len()
        );
        tracing::info!("   Instructions: {}", tx.message.instructions.len());

        // Store original bytes for transaction ID calculation
        let original_tx_bytes = tx_bytes.clone();

        // Compress the transaction if it exceeds the threshold
        // Use the transaction service's compression logic via process_and_relay_transaction
        // but we'll do it manually to avoid creating fragments twice
        let compressed_tx = if tx_bytes.len() > crate::COMPRESSION_THRESHOLD {
            tracing::info!(
                "Compressing transaction (threshold: {} bytes)",
                crate::COMPRESSION_THRESHOLD
            );
            // Create a temporary compressor to compress
            let compressor = crate::util::lz::Lz4Compressor::new()
                .map_err(|e| PolliNetError::Serialization(e.to_string()))?;
            let compressed = compressor.compress_with_size(&tx_bytes)
                .map_err(|e| PolliNetError::Serialization(e.to_string()))?;
            tracing::info!(
                "Compressed: {} bytes -> {} bytes",
                tx_bytes.len(),
                compressed.len()
            );
            compressed
        } else {
            tracing::info!("Transaction below compression threshold, keeping uncompressed");
            tx_bytes
        };

        tracing::info!("Final transaction size: {} bytes", compressed_tx.len());

        // Fragment the transaction
        let mesh_fragments = if let Some(max_payload) = max_payload {
            fragmenter::fragment_transaction_with_max_payload(&compressed_tx, max_payload)
        } else {
            fragmenter::fragment_transaction(&compressed_tx)
        };

        tracing::info!(
            "✅ Created {} fragments for BLE transmission",
            mesh_fragments.len()
        );

        // Calculate transaction ID (SHA-256 hash of original uncompressed transaction)
        let mut hasher = Sha256::new();
        hasher.update(&original_tx_bytes);
        let tx_id = hex::encode(hasher.finalize());

        // Create outbound transaction with NORMAL priority (external partner transactions)
        let outbound_tx = OutboundTransaction::new(
            tx_id.clone(),
            original_tx_bytes, // Store original uncompressed bytes
            mesh_fragments,
            Priority::Normal, // External partner transactions use normal priority
        );

        // Add to outbound queue
        let mut queue = self.queue_manager.outbound.write().await;
        queue.push(outbound_tx).map_err(|e| {
            PolliNetError::Serialization(format!("Failed to add transaction to queue: {}", e))
        })?;
        drop(queue);

        tracing::info!("✅ External transaction queued for relay: {}", tx_id);

        Ok(tx_id)
    }
}

/// Error types for PolliNet operations
#[derive(Error, Debug)]
pub enum PolliNetError {
    #[error("Solana RPC error: {0}")]
    SolanaRpc(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Configuration error: {0}")]
    Configuration(String),
}

/// BLE MTU size for packet fragmentation
pub const BLE_MTU_SIZE: usize = 360;

/// Compression threshold in bytes
pub const COMPRESSION_THRESHOLD: usize = 100;
