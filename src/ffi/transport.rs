//! Host-driven BLE transport layer
//! 
//! This module provides a transport mechanism where the host platform (Android)
//! drives BLE operations, and Rust only handles packetization, reassembly, and
//! protocol state.

use crate::transaction::{Fragment as TxFragment, FragmentType, TransactionService};
use crate::ble::MeshHealthMonitor;
use crate::storage::SecureStorage;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use super::types::{Fragment, MetricsSnapshot};

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
        // Deserialize the fragment
        let fragment: TxFragment = serde_json::from_slice(&data)
            .map_err(|e| format!("Failed to deserialize fragment: {}", e))?;

        let tx_id = fragment.id.clone();
        
        let mut buffers = self.inbound_buffers.lock();
        let buffer = buffers.entry(tx_id.clone()).or_insert_with(Vec::new);
        
        buffer.push(fragment.clone());
        
        // Check if we have all fragments for this transaction
        let total_fragments = fragment.total;
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
            // Try to reassemble

            match self.transaction_service.reassemble_fragments(&fragments) {
                Ok(tx_bytes) => {
                    // Move to completed queue
                    let mut completed = self.completed_transactions.lock();
                    completed.push_back((tx_id.clone(), tx_bytes));
                    
                    // Remove from inbound buffers
                    self.inbound_buffers.lock().remove(&tx_id);
                    
                    // Update metrics
                    let mut metrics = self.metrics.lock();
                    metrics.transactions_complete += 1;
                    metrics.updated_at = Self::current_timestamp();
                    
                    tracing::info!("✅ Transaction {} reassembled successfully", tx_id);
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
        queue.pop_front().and_then(|data| {
            if data.len() <= max_len {
                Some(data)
            } else {
                // Put it back if too large
                queue.push_front(data);
                None
            }
        })
    }

    /// Queue transaction fragments for sending
    pub fn queue_transaction(&self, tx_bytes: Vec<u8>) -> Result<Vec<Fragment>, String> {
        // Fragment the transaction
        let fragments = self.transaction_service.fragment_transaction(&tx_bytes);

        // Convert to FFI fragments and queue for sending
        let ffi_fragments: Vec<Fragment> = fragments
            .iter()
            .map(|f| self.convert_fragment_to_ffi(f))
            .collect();

        // Queue each fragment as JSON bytes
        let mut queue = self.outbound_queue.lock();
        for fragment in &fragments {
            let json_bytes = serde_json::to_vec(fragment)
                .map_err(|e| format!("Failed to serialize fragment: {}", e))?;
            queue.push_back(json_bytes);
        }

        tracing::info!(
            "📤 Queued {} fragments for transaction {}",
            fragments.len(),
            fragments[0].id
        );

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

