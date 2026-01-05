//! Host-driven BLE transport layer
//! 
//! This module provides a transport mechanism where the host platform (Android)
//! drives BLE operations, and Rust only handles packetization, reassembly, and
//! protocol state.

use crate::transaction::TransactionService;
use crate::ble::MeshHealthMonitor;
use crate::ble::mesh::TransactionFragment;
use crate::storage::SecureStorage;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use super::types::{Fragment, MetricsSnapshot, FragmentReassemblyInfo};

/// Maximum MTU size for BLE
const MAX_MTU: usize = 512;

/// Host-driven BLE transport bridge
pub struct HostBleTransport {
    /// Queue of outbound frames ready to send
    outbound_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    
    /// Inbound reassembly buffers keyed by transaction ID
    inbound_buffers: Arc<Mutex<HashMap<String, Vec<TransactionFragment>>>>,
    
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
    
    /// PolliNet SDK instance (Phase 2 - for queue access)
    pub sdk: Arc<crate::PolliNetSDK>,
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
        
        let sdk = crate::PolliNetSDK::new()
            .await
            .map_err(|e| format!("Failed to create SDK: {}", e))?;
        
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
            sdk: Arc::new(sdk),
        })
    }
    
    /// Create with an RPC client and optional secure storage
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, String> {
        let transaction_service = TransactionService::new_with_rpc(rpc_url)
            .await
            .map_err(|e| format!("Failed to create transaction service: {}", e))?;
        
        let sdk = crate::PolliNetSDK::new_with_rpc(rpc_url)
            .await
            .map_err(|e| format!("Failed to create SDK: {}", e))?;
        
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
            sdk: Arc::new(sdk),
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
        tracing::info!("📥 push_inbound() called with {} bytes", data.len());
        
        // Deserialize the mesh fragment using bincode1 (matching outbound serialization)
        use crate::ble::fragmenter::reconstruct_transaction;
        
        tracing::debug!("🔓 Deserializing fragment from binary data...");
        let fragment: TransactionFragment = bincode1::deserialize(&data)
            .map_err(|e| {
                let error_msg = format!("Failed to deserialize fragment ({} bytes): {}", data.len(), e);
                tracing::error!("❌ {}", error_msg);
                error_msg
            })?;
        
        tracing::debug!("✅ Fragment deserialized successfully");

        // Use transaction_id as tx_id (convert to 64-character hex string to match sender format)
        let tx_id = hex::encode(&fragment.transaction_id);
        
        tracing::info!(
            "📥 Received mesh fragment {}/{} for tx {} ({} bytes)",
            fragment.fragment_index + 1,
            fragment.total_fragments,
            tx_id,
            data.len()
        );
        
        let mut buffers = self.inbound_buffers.lock();
        
        // Store TransactionFragment directly (no conversion needed)
        let buffer = buffers.entry(tx_id.clone()).or_insert_with(Vec::new);
        let buffer_size_before = buffer.len();
        buffer.push(fragment.clone());
        let buffer_size_after = buffer.len();
        
        tracing::debug!("📦 Added fragment to buffer for tx {} (buffer size: {} → {})", 
            tx_id, buffer_size_before, buffer_size_after);
        
        // Check if we have all fragments for this transaction
        let total_fragments = fragment.total_fragments as usize;
        let fragments_received = buffer.len();
        let all_received = fragments_received == total_fragments;
        
        tracing::debug!("🔢 Fragment count check: {}/{} fragments for tx {}", 
            fragments_received, total_fragments, tx_id);
        
        tracing::info!(
            "📊 Fragment status for tx {}: {}/{} fragments received",
            tx_id,
            fragments_received,
            total_fragments
        );
        
        // Clone fragments before releasing lock if needed
        let fragments_to_reassemble = if all_received {
            tracing::info!("🎉 All fragments received for tx {} - ready for reassembly!", tx_id);
            Some(buffer.clone())
        } else {
            tracing::debug!("⏳ Waiting for {} more fragments for tx {}", total_fragments - fragments_received, tx_id);
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
            tracing::info!("🔧 Starting reassembly for tx {} with {} fragments", tx_id, fragments.len());
            
            // Fragments are already TransactionFragment - sort by index and use directly
            let mut mesh_fragments = fragments;
            mesh_fragments.sort_by_key(|f| f.fragment_index);
            
            tracing::debug!("✅ Using {} TransactionFragments directly, calling reconstruct_transaction()...", mesh_fragments.len());
            
            // Try to reassemble using mesh fragmenter
            match reconstruct_transaction(&mesh_fragments) {
                Ok(tx_bytes) => {
                    tracing::info!("✅ Transaction {} reassembled successfully ({} bytes)", tx_id, tx_bytes.len());
                    
                    // Remove from inbound buffers FIRST (before updating metrics)
                    tracing::debug!("🧹 Removing tx {} from inbound buffers...", tx_id);
                    self.inbound_buffers.lock().remove(&tx_id);
                    tracing::debug!("✅ Removed from inbound buffers");
                    
                    // Recalculate fragments_buffered after removal
                    let remaining_fragments = self.inbound_buffers.lock().values().map(|v| v.len() as u32).sum();
                    tracing::debug!("📊 Remaining fragments in buffers: {}", remaining_fragments);
                    
                    // Move to completed queue
                    tracing::debug!("📋 Adding to completed transactions queue...");
                    let mut completed = self.completed_transactions.lock();
                    let completed_size_before = completed.len();
                    completed.push_back((tx_id.clone(), tx_bytes.clone()));
                    let completed_size_after = completed.len();
                    drop(completed);
                    tracing::debug!("✅ Added to completed queue (size: {} → {})", completed_size_before, completed_size_after);
                    
                    // Also add to received transaction queue for auto-submission
                    tracing::info!("📥 Calling push_received_transaction() for tx {}...", tx_id);
                    let was_added = self.push_received_transaction(tx_bytes.clone());
                    let queue_size = self.received_queue_size();
                    
                    if was_added {
                        tracing::info!("📥 Transaction {} added to received queue (queue size: {})", tx_id, queue_size);
                    } else {
                        tracing::warn!("⚠️ Transaction {} was NOT added to received queue (likely duplicate, queue size: {})", tx_id, queue_size);
                    }
                    
                    // Update metrics AFTER removing from buffers
                    let mut metrics = self.metrics.lock();
                    metrics.fragments_buffered = remaining_fragments; // Update to actual count
                    metrics.transactions_complete += 1;
                    metrics.updated_at = Self::current_timestamp();
                    drop(metrics);
                    
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
        
        tracing::debug!("🔍 next_outbound called: queue has {} items, max_len={}", queue_size_before, max_len);
        
        let result = queue.pop_front().and_then(|data| {
            if data.len() <= max_len {
                let queue_size_after = queue.len();
                tracing::info!("✅ Returning fragment of {} bytes (max: {})", data.len(), max_len);
                tracing::info!("📊 Queue: {} → {} fragments remaining", queue_size_before, queue_size_after);
                Some(data)
            } else {
                // Put it back if too large
                tracing::warn!("⚠️ Fragment too large: {} bytes (max: {}), putting back in queue", data.len(), max_len);
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
    fn convert_mesh_fragment_to_ffi(&self, mesh_fragment: &crate::ble::mesh::TransactionFragment) -> Fragment {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        
        Fragment {
            id: format!("{:x}", &mesh_fragment.transaction_id[0..8].iter().fold(0u64, |acc, &b| (acc << 8) | b as u64)),
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
    /// 
    /// # Arguments
    /// * `tx_bytes` - Complete signed transaction bytes
    /// * `max_payload` - Optional maximum payload size (typically MTU - 10). If None, uses default MAX_FRAGMENT_DATA
    pub fn queue_transaction(&self, tx_bytes: Vec<u8>, max_payload: Option<usize>) -> Result<Vec<Fragment>, String> {
        tracing::info!("📦 queue_transaction() called with {} bytes, max_payload: {:?}", 
            tx_bytes.len(), max_payload);
        
        // Use BLE mesh fragmenter with MTU-aware payload size
        use crate::ble::fragmenter;
        tracing::debug!("🔧 Fragmenting transaction with max_payload: {:?}...", max_payload);
        let mesh_fragments = if let Some(max_payload) = max_payload {
            fragmenter::fragment_transaction_with_max_payload(&tx_bytes, max_payload)
        } else {
            fragmenter::fragment_transaction(&tx_bytes)
        };
        
        tracing::debug!("✅ Fragmenter created {} fragments", mesh_fragments.len());
        
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
        tracing::info!("📥 push_received_transaction() called with {} bytes", tx_bytes.len());
        
        use sha2::{Sha256, Digest};
        
        // Calculate transaction hash for logging/identification
        tracing::debug!("🔐 Calculating SHA-256 hash for transaction...");
        let mut hasher = Sha256::new();
        hasher.update(&tx_bytes);
        let tx_hash = hasher.finalize().to_vec();
        let tx_hash_hex = hex::encode(&tx_hash);
        tracing::debug!("✅ Transaction hash calculated: {} ({} bytes)", 
            tx_hash_hex.chars().take(32).collect::<String>(), 
            tx_hash.len());
        
        // No duplicate check - all reassembled transactions are queued
        let now = Self::current_timestamp();
        tracing::debug!("📥 Processing transaction {} (no duplicate check)", 
            tx_hash_hex.chars().take(16).collect::<String>());
        
        // Generate transaction ID
        let tx_id = uuid::Uuid::new_v4().to_string();
        #[cfg(feature = "android")]
        log::debug!("🆔 Generated transaction ID: {}", tx_id);
        #[cfg(not(feature = "android"))]
        tracing::debug!("🆔 Generated transaction ID: {}", tx_id);
        
        // Add to received queue
        let mut queue = self.received_tx_queue.lock();
        let queue_size_before = queue.len();
        tracing::debug!("📋 Received queue size before adding: {}", queue_size_before);
        
        queue.push_back((tx_id.clone(), tx_bytes.clone(), now));
        let queue_size = queue.len();
        drop(queue);
        
        tracing::info!("📥 Queued received transaction {} for auto-submission (queue size: {} -> {})", 
            tx_id, queue_size_before, queue_size);
        tracing::info!("   Transaction hash: {} ({} bytes)", 
            tx_hash_hex.chars().take(32).collect::<String>(), 
            tx_bytes.len());
        tracing::info!("   Timestamp: {}", now);
        tracing::info!("✅ push_received_transaction() returning true (successfully queued)");
        true
    }
    
    /// Get next received transaction for auto-submission
    /// Returns (tx_id, tx_bytes, received_at_timestamp)
    pub fn next_received_transaction(&self) -> Option<(String, Vec<u8>, u64)> {
        tracing::info!("📤 next_received_transaction() called");
        
        let mut queue = self.received_tx_queue.lock();
        let queue_size_before = queue.len();
        tracing::debug!("📋 Received queue size before pop: {}", queue_size_before);
        
        let result = queue.pop_front();
        
        if let Some((tx_id, tx_bytes, timestamp)) = &result {
            let queue_size_after = queue.len();
            tracing::info!("✅ Retrieved transaction {} from received queue ({} bytes, timestamp: {})", 
                tx_id, tx_bytes.len(), timestamp);
            tracing::info!("📊 Received queue: {} → {} transactions remaining", 
                queue_size_before, queue_size_after);
        } else {
            tracing::debug!("📭 Received queue is empty, returning None");
        }
        
        drop(queue);
        result
    }
    
    /// Get count of transactions waiting for auto-submission
    pub fn received_queue_size(&self) -> usize {
        self.received_tx_queue.lock().len()
    }
    
    /// Get fragment reassembly progress for all incomplete transactions
    pub fn get_fragment_reassembly_info(&self) -> Vec<FragmentReassemblyInfo> {
        let buffers = self.inbound_buffers.lock();
        let mut info_list = Vec::new();
        
        for (tx_id, fragments) in buffers.iter() {
            if fragments.is_empty() {
                continue;
            }
            
            // Get total fragments from first fragment
            let total_fragments = fragments.first().map(|f| f.total_fragments as usize).unwrap_or(0);
            let received_count = fragments.len();
            
            // Get received fragment indices
            let received_indices: Vec<usize> = fragments.iter()
                .map(|f| f.fragment_index as usize)
                .collect();
            
            // Get fragment sizes
            let fragment_sizes: Vec<usize> = fragments.iter()
                .map(|f| f.data.len())
                .collect();
            
            // Calculate total bytes received so far
            let total_bytes: usize = fragment_sizes.iter().sum();
            
            info_list.push(FragmentReassemblyInfo {
                transaction_id: tx_id.clone(),
                total_fragments,
                received_fragments: received_count,
                received_indices,
                fragment_sizes,
                total_bytes_received: total_bytes,
            });
        }
        
        info_list
    }
    
    /// Get outbound queue size without removing items (for debugging)
    pub fn outbound_queue_size(&self) -> usize {
        self.outbound_queue.lock().len()
    }
    
    /// Get outbound queue debug info without removing items
    pub fn outbound_queue_debug(&self) -> Vec<(usize, usize)> {
        let queue = self.outbound_queue.lock();
        queue.iter()
            .enumerate()
            .map(|(idx, data)| (idx, data.len()))
            .collect()
    }
    
    /// Mark a transaction as successfully submitted (for deduplication)
    pub fn mark_transaction_submitted(&self, tx_bytes: &[u8]) {
        use sha2::{Sha256, Digest};
        
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

