//! Host-driven BLE transport layer
//!
//! This module provides a transport mechanism where the host platform (Android)
//! drives BLE operations, and Rust only handles packetization, reassembly, and
//! protocol state.

use super::types::{Fragment, MetricsSnapshot};
use crate::ble::MeshHealthMonitor;
use crate::storage::SecureStorage;
use crate::transaction::{Fragment as TxFragment, FragmentType, TransactionService};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum MTU size for BLE
const MAX_MTU: usize = 512;

/// Host-driven BLE transport bridge
pub struct HostBleTransport {
    /// Queue of outbound frames ready to send
    outbound_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,

    /// Inbound reassembly buffers keyed by transaction ID
    inbound_buffers: Arc<Mutex<HashMap<String, Vec<TxFragment>>>>,

    /// Completed transactions ready for processing
    completed_transactions: Arc<Mutex<VecDeque<(String, Vec<u8>)>>>,

    /// Queue of received transactions ready for auto-submission
    /// (tx_id, tx_bytes, received_at_timestamp)
    received_tx_queue: Arc<Mutex<VecDeque<(String, Vec<u8>, u64)>>>,

    /// Set of transaction hashes that have been submitted (for deduplication)
    submitted_tx_hashes: Arc<Mutex<HashMap<Vec<u8>, u64>>>,

    /// Metrics
    metrics: Arc<Mutex<TransportMetrics>>,

    /// Transaction service for fragmentation and building
    transaction_service: Arc<TransactionService>,

    /// Secure storage for nonce bundles (optional)
    secure_storage: Option<Arc<SecureStorage>>,

    /// Mesh health monitor for tracking peer/network quality
    health_monitor: Arc<MeshHealthMonitor>,
}

impl HostBleTransport {
    /// Get reference to transaction service
    pub fn transaction_service(&self) -> &TransactionService {
        &self.transaction_service
    }
}

#[derive(Debug, Clone, Default)]
struct TransportMetrics {
    fragments_buffered: u32,
    transactions_complete: u32,
    reassembly_failures: u32,
    last_error: String,
    updated_at: u64,
}

impl HostBleTransport {
    /// Create a new host-driven transport
    pub async fn new() -> Result<Self, String> {
        let transaction_service = TransactionService::new()
            .await
            .map_err(|e| format!("Failed to create transaction service: {}", e))?;

        Ok(Self {
            outbound_queue: Arc::new(Mutex::new(VecDeque::new())),
            inbound_buffers: Arc::new(Mutex::new(HashMap::new())),
            completed_transactions: Arc::new(Mutex::new(VecDeque::new())),
            received_tx_queue: Arc::new(Mutex::new(VecDeque::new())),
            submitted_tx_hashes: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(Mutex::new(TransportMetrics::default())),
            transaction_service: Arc::new(transaction_service),
            secure_storage: None,
            health_monitor: Arc::new(MeshHealthMonitor::default()),
        })
    }

    /// Create with an RPC client and optional secure storage
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, String> {
        let transaction_service = TransactionService::new_with_rpc(rpc_url)
            .await
            .map_err(|e| format!("Failed to create transaction service: {}", e))?;

        Ok(Self {
            outbound_queue: Arc::new(Mutex::new(VecDeque::new())),
            inbound_buffers: Arc::new(Mutex::new(HashMap::new())),
            completed_transactions: Arc::new(Mutex::new(VecDeque::new())),
            received_tx_queue: Arc::new(Mutex::new(VecDeque::new())),
            submitted_tx_hashes: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(Mutex::new(TransportMetrics::default())),
            transaction_service: Arc::new(transaction_service),
            secure_storage: None,
            health_monitor: Arc::new(MeshHealthMonitor::default()),
        })
    }

    /// Set secure storage directory for nonce bundle persistence
    pub fn set_secure_storage(&mut self, storage_dir: &str) -> Result<(), String> {
        let storage = SecureStorage::new(storage_dir)
            .map_err(|e| format!("Failed to create secure storage: {}", e))?;
        self.secure_storage = Some(Arc::new(storage));
        tracing::info!("🔒 Secure storage enabled for nonce bundles");
        Ok(())
    }

    /// Get secure storage if available
    pub fn secure_storage(&self) -> Option<&Arc<SecureStorage>> {
        self.secure_storage.as_ref()
    }

    /// Get health monitor
    pub fn health_monitor(&self) -> Arc<MeshHealthMonitor> {
        self.health_monitor.clone()
    }

    /// Push inbound data from GATT characteristic
    pub fn push_inbound(&self, data: Vec<u8>) -> Result<(), String> {
        // Deserialize the mesh fragment using bincode1 (matching outbound serialization)
        use crate::ble::fragmenter::reconstruct_transaction;
        use crate::ble::mesh::TransactionFragment;

        let fragment: TransactionFragment = bincode1::deserialize(&data)
            .map_err(|e| format!("Failed to deserialize fragment: {}", e))?;

        // Use transaction_id as tx_id (convert to hex string)
        let tx_id = format!(
            "{:x}",
            &fragment
                .transaction_id
                .iter()
                .fold(0u64, |acc, &b| (acc << 8) | b as u64)
        );

        tracing::info!(
            "📥 Received mesh fragment {}/{} for tx {}",
            fragment.fragment_index + 1,
            fragment.total_fragments,
            tx_id
        );

        let mut buffers = self.inbound_buffers.lock();

        // IMPORTANT: We need to store the mesh TransactionFragment, not TxFragment
        // So we need to change the inbound_buffers type
        // For now, let's convert it to a temporary structure
        let tx_fragment = TxFragment {
            id: tx_id.clone(),
            index: fragment.fragment_index as usize,
            total: fragment.total_fragments as usize,
            data: fragment.data.clone(),
            fragment_type: if fragment.fragment_index == 0 {
                crate::transaction::FragmentType::FragmentStart
            } else if fragment.fragment_index == fragment.total_fragments - 1 {
                crate::transaction::FragmentType::FragmentEnd
            } else {
                crate::transaction::FragmentType::FragmentContinue
            },
            checksum: fragment.transaction_id,
        };

        let buffer = buffers.entry(tx_id.clone()).or_insert_with(Vec::new);
        buffer.push(tx_fragment.clone());

        // Check if we have all fragments for this transaction
        let total_fragments = fragment.total_fragments as usize;
        let all_received = buffer.len() == total_fragments;

        // Clone fragments before releasing lock if needed
        let fragments_to_reassemble = if all_received {
            Some(buffer.clone())
        } else {
            None
        };

        // Calculate metrics count
        let fragments_buffered_count = buffers.values().map(|v| v.len() as u32).sum();
        drop(buffers); // Release buffers lock

        // Update metrics
        let mut metrics = self.metrics.lock();
        metrics.fragments_buffered = fragments_buffered_count;
        metrics.updated_at = Self::current_timestamp();
        drop(metrics); // Release metrics lock

        if let Some(fragments) = fragments_to_reassemble {
            // Convert TxFragments back to mesh TransactionFragments for reassembly
            let mesh_fragments: Vec<TransactionFragment> = fragments
                .iter()
                .map(|f| TransactionFragment {
                    transaction_id: f.checksum,
                    fragment_index: f.index as u16,
                    total_fragments: f.total as u16,
                    data: f.data.clone(),
                })
                .collect();

            // Try to reassemble using mesh fragmenter
            match reconstruct_transaction(&mesh_fragments) {
                Ok(tx_bytes) => {
                    // Move to completed queue
                    let mut completed = self.completed_transactions.lock();
                    completed.push_back((tx_id.clone(), tx_bytes.clone()));

                    // Also add to received transaction queue for auto-submission
                    self.push_received_transaction(tx_bytes);

                    // Remove from inbound buffers
                    self.inbound_buffers.lock().remove(&tx_id);

                    // Update metrics
                    let mut metrics = self.metrics.lock();
                    metrics.transactions_complete += 1;
                    metrics.updated_at = Self::current_timestamp();

                    tracing::info!(
                        "✅ Transaction {} reassembled and queued for auto-submission",
                        tx_id
                    );
                    Ok(())
                }
                Err(e) => {
                    let error_msg = format!("Failed to reassemble transaction {}: {}", tx_id, e);
                    tracing::error!("{}", error_msg);

                    // Update metrics
                    let mut metrics = self.metrics.lock();
                    metrics.reassembly_failures += 1;
                    metrics.last_error = error_msg.clone();
                    metrics.updated_at = Self::current_timestamp();

                    // Remove failed fragments
                    self.inbound_buffers.lock().remove(&tx_id);

                    Err(error_msg)
                }
            }
        } else {
            Ok(())
        }
    }

    /// Get next outbound frame to send
    pub fn next_outbound(&self, max_len: usize) -> Option<Vec<u8>> {
        let mut queue = self.outbound_queue.lock();
        let queue_size_before = queue.len();

        tracing::debug!(
            "🔍 next_outbound called: queue has {} items, max_len={}",
            queue_size_before,
            max_len
        );

        let result = queue.pop_front().and_then(|data| {
            if data.len() <= max_len {
                let queue_size_after = queue.len();
                tracing::info!(
                    "✅ Returning fragment of {} bytes (max: {})",
                    data.len(),
                    max_len
                );
                tracing::info!(
                    "📊 Queue: {} → {} fragments remaining",
                    queue_size_before,
                    queue_size_after
                );
                Some(data)
            } else {
                // Put it back if too large
                tracing::warn!(
                    "⚠️ Fragment too large: {} bytes (max: {}), putting back in queue",
                    data.len(),
                    max_len
                );
                queue.push_front(data);
                None
            }
        });

        if result.is_none() && queue_size_before == 0 {
            tracing::debug!("📭 Queue is empty, nothing to send");
        }

        result
    }

    /// Convert a BLE mesh TransactionFragment to FFI Fragment
    fn convert_mesh_fragment_to_ffi(
        &self,
        mesh_fragment: &crate::ble::mesh::TransactionFragment,
    ) -> Fragment {
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        Fragment {
            id: format!(
                "{:x}",
                &mesh_fragment.transaction_id[0..8]
                    .iter()
                    .fold(0u64, |acc, &b| (acc << 8) | b as u64)
            ),
            index: mesh_fragment.fragment_index as u32,
            total: mesh_fragment.total_fragments as u32,
            data: STANDARD.encode(&mesh_fragment.data),
            fragment_type: if mesh_fragment.fragment_index == 0 {
                "FragmentStart".to_string()
            } else if mesh_fragment.fragment_index == mesh_fragment.total_fragments - 1 {
                "FragmentEnd".to_string()
            } else {
                "FragmentContinue".to_string()
            },
            checksum: STANDARD.encode(&mesh_fragment.transaction_id),
        }
    }

    /// Queue transaction fragments for sending
    pub fn queue_transaction(&self, tx_bytes: Vec<u8>) -> Result<Vec<Fragment>, String> {
        // Use BLE mesh fragmenter for optimal fragment size (52 bytes data)
        use crate::ble::fragmenter::fragment_transaction as mesh_fragment;
        let mesh_fragments = mesh_fragment(&tx_bytes);

        tracing::info!(
            "📦 Mesh fragmenter created {} fragments for {} byte transaction",
            mesh_fragments.len(),
            tx_bytes.len()
        );

        // Queue each fragment as compact binary bytes (bincode)
        // We serialize the mesh TransactionFragment which is much more compact
        let mut queue = self.outbound_queue.lock();
        let queue_size_before = queue.len();

        for fragment in &mesh_fragments {
            // Use bincode1 for compact binary serialization
            // TransactionFragment is: transaction_id[32] + fragment_index(u16) + total_fragments(u16) + data(Vec<u8>)
            let binary_bytes = bincode1::serialize(fragment)
                .map_err(|e| format!("Failed to serialize fragment: {}", e))?;

            tracing::info!(
                "📦 Fragment serialized: {} bytes (data: {}B, index: {}/{})",
                binary_bytes.len(),
                fragment.data.len(),
                fragment.fragment_index,
                fragment.total_fragments
            );

            queue.push_back(binary_bytes);
        }

        // Convert mesh fragments to FFI fragments for return value
        let ffi_fragments: Vec<Fragment> = mesh_fragments
            .iter()
            .map(|mf| self.convert_mesh_fragment_to_ffi(mf))
            .collect();

        let queue_size_after = queue.len();
        let total_bytes: usize = queue.iter().map(|data| data.len()).sum();

        tracing::info!(
            "📤 Queued {} fragments for transaction {}",
            ffi_fragments.len(),
            ffi_fragments[0].id
        );
        tracing::info!(
            "📊 Outbound queue: {} → {} fragments ({} total bytes)",
            queue_size_before,
            queue_size_after,
            total_bytes
        );

        // Log each fragment in queue for debugging
        for (idx, data) in queue.iter().enumerate() {
            tracing::debug!("  Fragment [{}]: {} bytes", idx, data.len());
        }

        Ok(ffi_fragments)
    }

    /// Periodic tick for retries and timeouts
    pub fn tick(&self, _now_ms: u64) -> Vec<Vec<u8>> {
        // TODO: Implement retry logic and timeout handling
        // For now, just return empty list
        Vec::new()
    }

    /// Get current metrics snapshot
    pub fn metrics(&self) -> MetricsSnapshot {
        let metrics = self.metrics.lock();
        MetricsSnapshot {
            fragments_buffered: metrics.fragments_buffered,
            transactions_complete: metrics.transactions_complete,
            reassembly_failures: metrics.reassembly_failures,
            last_error: metrics.last_error.clone(),
            updated_at: metrics.updated_at,
        }
    }

    /// Clear a specific transaction from buffers
    pub fn clear_transaction(&self, tx_id: &str) {
        self.inbound_buffers.lock().remove(tx_id);
        tracing::info!("🗑️  Cleared transaction {}", tx_id);
    }

    /// Get next completed transaction
    pub fn pop_completed(&self) -> Option<(String, Vec<u8>)> {
        self.completed_transactions.lock().pop_front()
    }

    /// Push a received transaction into the auto-submission queue
    /// Returns true if added, false if it's a duplicate
    pub fn push_received_transaction(&self, tx_bytes: Vec<u8>) -> bool {
        use sha2::{Digest, Sha256};

        // Calculate transaction hash for deduplication
        let mut hasher = Sha256::new();
        hasher.update(&tx_bytes);
        let tx_hash = hasher.finalize().to_vec();

        // Check if already submitted
        let mut submitted = self.submitted_tx_hashes.lock();
        if submitted.contains_key(&tx_hash) {
            tracing::debug!("⏩ Skipping duplicate transaction");
            return false;
        }

        // Add to submitted set with current timestamp
        submitted.insert(tx_hash, Self::current_timestamp());
        drop(submitted);

        // Generate transaction ID
        let tx_id = uuid::Uuid::new_v4().to_string();

        // Add to received queue
        let mut queue = self.received_tx_queue.lock();
        queue.push_back((tx_id.clone(), tx_bytes, Self::current_timestamp()));

        tracing::info!(
            "📥 Queued received transaction {} for auto-submission",
            tx_id
        );
        true
    }

    /// Get next received transaction for auto-submission
    /// Returns (tx_id, tx_bytes, received_at_timestamp)
    pub fn next_received_transaction(&self) -> Option<(String, Vec<u8>, u64)> {
        self.received_tx_queue.lock().pop_front()
    }

    /// Get count of transactions waiting for auto-submission
    pub fn received_queue_size(&self) -> usize {
        self.received_tx_queue.lock().len()
    }

    /// Get outbound queue size without removing items (for debugging)
    pub fn outbound_queue_size(&self) -> usize {
        self.outbound_queue.lock().len()
    }

    /// Get outbound queue debug info without removing items
    pub fn outbound_queue_debug(&self) -> Vec<(usize, usize)> {
        let queue = self.outbound_queue.lock();
        queue
            .iter()
            .enumerate()
            .map(|(idx, data)| (idx, data.len()))
            .collect()
    }

    /// Mark a transaction as successfully submitted (for deduplication)
    pub fn mark_transaction_submitted(&self, tx_bytes: &[u8]) {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(tx_bytes);
        let tx_hash = hasher.finalize().to_vec();

        let mut submitted = self.submitted_tx_hashes.lock();
        submitted.insert(tx_hash, Self::current_timestamp());

        tracing::debug!("✅ Marked transaction as submitted");
    }

    /// Clean up old submitted transaction hashes (older than 24 hours)
    pub fn cleanup_old_submissions(&self) {
        let cutoff = Self::current_timestamp() - (24 * 60 * 60); // 24 hours ago

        let mut submitted = self.submitted_tx_hashes.lock();
        submitted.retain(|_, timestamp| *timestamp > cutoff);

        tracing::debug!("🧹 Cleaned up old submission hashes");
    }

    // Helper functions

    fn convert_fragment_to_ffi(&self, fragment: &TxFragment) -> Fragment {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

        let fragment_type = match &fragment.fragment_type {
            FragmentType::FragmentStart => "FragmentStart",
            FragmentType::FragmentEnd => "FragmentEnd",
            FragmentType::FragmentContinue => "FragmentContinue",
        };

        Fragment {
            id: fragment.id.clone(),
            index: fragment.index as u32,
            total: fragment.total as u32,
            data: BASE64.encode(&fragment.data),
            fragment_type: fragment_type.to_string(),
            checksum: BASE64.encode(&fragment.checksum),
        }
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transport_creation() {
        let transport = HostBleTransport::new().await.unwrap();
        assert!(transport.next_outbound(512).is_none());
    }

    #[tokio::test]
    async fn test_metrics() {
        let transport = HostBleTransport::new().await.unwrap();
        let metrics = transport.metrics();
        assert_eq!(metrics.transactions_complete, 0);
        assert_eq!(metrics.fragments_buffered, 0);
    }
}
