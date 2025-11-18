//! Transaction Broadcasting
//!
//! Handles broadcasting signed Solana transactions across the BLE mesh network.
//! Fragments are sent to all connected peers with flood prevention and tracking.

use crate::ble::mesh::{MeshPacket, MeshRouter, PacketType, TransactionFragment};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Maximum age for a broadcast before it's considered expired
const BROADCAST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// Maximum retries for fragment transmission
const MAX_RETRIES: u32 = 3;

/// Retry interval for failed fragments
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// Broadcast status for a transaction
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastStatus {
    /// Broadcast is in progress
    InProgress,
    /// Broadcast completed successfully
    Completed,
    /// Broadcast failed
    Failed,
    /// Broadcast timed out
    TimedOut,
}

/// Information about fragment propagation to a specific peer
#[derive(Debug, Clone)]
struct PeerFragmentStatus {
    /// Peer ID
    peer_id: String,
    /// Fragments successfully sent to this peer
    sent_fragments: HashSet<u16>,
    /// Fragments pending transmission
    pending_fragments: HashSet<u16>,
    /// Transmission attempts per fragment
    retry_counts: HashMap<u16, u32>,
    /// Last transmission time per fragment
    last_retry: HashMap<u16, Instant>,
}

impl PeerFragmentStatus {
    fn new(peer_id: String, total_fragments: u16) -> Self {
        let pending_fragments: HashSet<u16> = (0..total_fragments).collect();

        Self {
            peer_id,
            sent_fragments: HashSet::new(),
            pending_fragments,
            retry_counts: HashMap::new(),
            last_retry: HashMap::new(),
        }
    }

    /// Mark a fragment as successfully sent
    fn mark_sent(&mut self, fragment_index: u16) {
        self.pending_fragments.remove(&fragment_index);
        self.sent_fragments.insert(fragment_index);
        self.retry_counts.remove(&fragment_index);
        self.last_retry.remove(&fragment_index);
    }

    /// Check if a fragment needs retry
    fn needs_retry(&self, fragment_index: u16) -> bool {
        if !self.pending_fragments.contains(&fragment_index) {
            return false;
        }

        let retry_count = self.retry_counts.get(&fragment_index).unwrap_or(&0);
        if *retry_count >= MAX_RETRIES {
            return false;
        }

        if let Some(last_retry) = self.last_retry.get(&fragment_index) {
            last_retry.elapsed() >= RETRY_INTERVAL
        } else {
            true
        }
    }

    /// Record a retry attempt
    fn record_retry(&mut self, fragment_index: u16) {
        *self.retry_counts.entry(fragment_index).or_insert(0) += 1;
        self.last_retry.insert(fragment_index, Instant::now());
    }

    /// Check if all fragments have been sent
    fn is_complete(&self) -> bool {
        self.pending_fragments.is_empty()
    }

    /// Get completion percentage
    fn completion_percentage(&self) -> f32 {
        let total = self.sent_fragments.len() + self.pending_fragments.len();
        if total == 0 {
            return 100.0;
        }
        (self.sent_fragments.len() as f32 / total as f32) * 100.0
    }
}

/// Broadcast tracking information
#[derive(Debug, Clone)]
pub struct BroadcastInfo {
    /// Transaction ID (SHA256 hash)
    pub transaction_id: [u8; 32],
    /// Transaction fragments
    pub fragments: Vec<TransactionFragment>,
    /// Per-peer transmission status
    pub peer_status: HashMap<String, PeerFragmentStatus>,
    /// Overall broadcast status
    pub status: BroadcastStatus,
    /// Broadcast start time
    pub started_at: Instant,
    /// Total peers participating
    pub total_peers: usize,
}

impl BroadcastInfo {
    fn new(fragments: Vec<TransactionFragment>, peer_ids: Vec<String>) -> Self {
        let transaction_id = fragments[0].transaction_id;
        let total_fragments = fragments.len() as u16;

        let peer_status: HashMap<String, PeerFragmentStatus> = peer_ids
            .iter()
            .map(|peer_id| {
                (
                    peer_id.clone(),
                    PeerFragmentStatus::new(peer_id.clone(), total_fragments),
                )
            })
            .collect();

        Self {
            transaction_id,
            fragments,
            peer_status,
            status: BroadcastStatus::InProgress,
            started_at: Instant::now(),
            total_peers: peer_ids.len(),
        }
    }

    /// Get overall completion percentage
    pub fn overall_completion(&self) -> f32 {
        if self.peer_status.is_empty() {
            return 0.0;
        }

        let total_completion: f32 = self
            .peer_status
            .values()
            .map(|ps| ps.completion_percentage())
            .sum();

        total_completion / self.peer_status.len() as f32
    }

    /// Check if broadcast is complete
    pub fn is_complete(&self) -> bool {
        self.peer_status.values().all(|ps| ps.is_complete())
    }

    /// Check if broadcast has timed out
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed() > BROADCAST_TIMEOUT
    }

    /// Update status based on current state
    fn update_status(&mut self) {
        if self.is_timed_out() {
            self.status = BroadcastStatus::TimedOut;
        } else if self.is_complete() {
            self.status = BroadcastStatus::Completed;
        }
    }
}

/// Transaction broadcaster for BLE mesh
pub struct TransactionBroadcaster {
    /// Device ID for packet creation
    device_id: Uuid,
    /// Active broadcasts
    broadcasts: Arc<RwLock<HashMap<[u8; 32], BroadcastInfo>>>,
}

impl TransactionBroadcaster {
    /// Create a new transaction broadcaster
    pub fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            broadcasts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Prepare a broadcast of a signed transaction
    ///
    /// Fragments the transaction and prepares broadcast tracking.
    /// Does not actually send - use `get_pending_fragments()` to get fragments to send.
    ///
    /// # Arguments
    /// * `transaction_bytes` - Signed Solana transaction
    /// * `peer_ids` - List of peer IDs to broadcast to
    ///
    /// # Returns
    /// Transaction ID for tracking broadcast status
    pub async fn prepare_broadcast(
        &self,
        transaction_bytes: &[u8],
        peer_ids: Vec<String>,
    ) -> Result<[u8; 32], String> {
        if peer_ids.is_empty() {
            return Err("No peers available for broadcast".to_string());
        }

        tracing::info!("🚀 Preparing broadcast to {} peers", peer_ids.len());

        // Fragment the transaction
        let fragments = crate::ble::fragment_transaction(transaction_bytes);
        let transaction_id = fragments[0].transaction_id;

        tracing::info!(
            "Transaction {} fragmented into {} pieces",
            hex::encode(&transaction_id[..8]),
            fragments.len()
        );

        // Create broadcast tracking
        let broadcast_info = BroadcastInfo::new(fragments.clone(), peer_ids.clone());

        // Store broadcast info
        {
            let mut broadcasts = self.broadcasts.write().await;
            broadcasts.insert(transaction_id, broadcast_info);
        }

        tracing::info!("✅ Broadcast prepared with {} fragments", fragments.len());

        Ok(transaction_id)
    }

    /// Mark a fragment as successfully sent to a peer
    pub async fn mark_fragment_sent(
        &self,
        transaction_id: &[u8; 32],
        peer_id: &str,
        fragment_index: u16,
    ) -> Result<(), String> {
        let mut broadcasts = self.broadcasts.write().await;

        if let Some(info) = broadcasts.get_mut(transaction_id) {
            if let Some(peer_status) = info.peer_status.get_mut(peer_id) {
                peer_status.mark_sent(fragment_index);
                Ok(())
            } else {
                Err(format!("Peer {} not found in broadcast", peer_id))
            }
        } else {
            Err("Broadcast not found".to_string())
        }
    }

    /// Get all fragments for a broadcast
    pub async fn get_broadcast_fragments(
        &self,
        transaction_id: &[u8; 32],
    ) -> Option<Vec<TransactionFragment>> {
        let broadcasts = self.broadcasts.read().await;
        broadcasts
            .get(transaction_id)
            .map(|info| info.fragments.clone())
    }

    /// Prepare a mesh packet for a fragment
    ///
    /// Returns serialized packet bytes ready for BLE transmission
    pub fn prepare_fragment_packet(
        &self,
        fragment: &TransactionFragment,
    ) -> Result<Vec<u8>, String> {
        // Serialize fragment
        let payload = bincode1::serialize(fragment)
            .map_err(|e| format!("Failed to serialize fragment: {}", e))?;

        // Create mesh packet
        let packet = MeshPacket::new(PacketType::TransactionFragment, self.device_id, payload);

        // Serialize packet
        Ok(packet.serialize())
    }

    /// Get broadcast status for a transaction
    pub async fn get_broadcast_status(&self, transaction_id: &[u8; 32]) -> Option<BroadcastInfo> {
        let broadcasts = self.broadcasts.read().await;
        broadcasts.get(transaction_id).cloned()
    }

    /// Cancel an ongoing broadcast
    pub async fn cancel_broadcast(&self, transaction_id: &[u8; 32]) -> Result<(), String> {
        let mut broadcasts = self.broadcasts.write().await;

        if let Some(info) = broadcasts.get_mut(transaction_id) {
            info.status = BroadcastStatus::Failed;
            tracing::info!("Broadcast {} cancelled", hex::encode(&transaction_id[..8]));
            Ok(())
        } else {
            Err("Broadcast not found".to_string())
        }
    }

    /// Get statistics for all broadcasts
    pub async fn get_statistics(&self) -> BroadcastStatistics {
        let broadcasts = self.broadcasts.read().await;

        let total_broadcasts = broadcasts.len();
        let active_broadcasts = broadcasts
            .values()
            .filter(|b| b.status == BroadcastStatus::InProgress)
            .count();
        let completed_broadcasts = broadcasts
            .values()
            .filter(|b| b.status == BroadcastStatus::Completed)
            .count();
        let failed_broadcasts = broadcasts
            .values()
            .filter(|b| {
                b.status == BroadcastStatus::Failed || b.status == BroadcastStatus::TimedOut
            })
            .count();

        let avg_completion = if total_broadcasts > 0 {
            broadcasts
                .values()
                .map(|b| b.overall_completion())
                .sum::<f32>()
                / total_broadcasts as f32
        } else {
            0.0
        };

        BroadcastStatistics {
            total_broadcasts,
            active_broadcasts,
            completed_broadcasts,
            failed_broadcasts,
            average_completion: avg_completion,
        }
    }

    /// Clean up expired broadcasts
    pub async fn cleanup_expired(&self) {
        let mut broadcasts = self.broadcasts.write().await;

        let expired: Vec<[u8; 32]> = broadcasts
            .iter()
            .filter(|(_, info)| {
                info.started_at.elapsed() > BROADCAST_TIMEOUT * 2
                    && info.status != BroadcastStatus::InProgress
            })
            .map(|(id, _)| *id)
            .collect();

        for tx_id in expired {
            broadcasts.remove(&tx_id);
            tracing::debug!("Cleaned up expired broadcast {}", hex::encode(&tx_id[..8]));
        }
    }
}

/// Broadcast statistics
#[derive(Debug, Clone)]
pub struct BroadcastStatistics {
    pub total_broadcasts: usize,
    pub active_broadcasts: usize,
    pub completed_broadcasts: usize,
    pub failed_broadcasts: usize,
    pub average_completion: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_fragment_status() {
        let mut status = PeerFragmentStatus::new("peer1".to_string(), 3);

        assert_eq!(status.pending_fragments.len(), 3);
        assert_eq!(status.sent_fragments.len(), 0);
        assert_eq!(status.completion_percentage(), 0.0);

        status.mark_sent(0);
        assert_eq!(status.completion_percentage(), 33.333_332);

        status.mark_sent(1);
        status.mark_sent(2);
        assert!(status.is_complete());
        assert_eq!(status.completion_percentage(), 100.0);
    }

    #[test]
    fn test_retry_logic() {
        let mut status = PeerFragmentStatus::new("peer1".to_string(), 3);

        // First retry should be allowed
        assert!(status.needs_retry(0));

        status.record_retry(0);
        assert_eq!(*status.retry_counts.get(&0).unwrap(), 1);

        // Immediate retry should be blocked (within interval)
        assert!(!status.needs_retry(0));
    }

    #[test]
    fn test_broadcast_info() {
        let fragments = vec![
            TransactionFragment {
                transaction_id: [1u8; 32],
                fragment_index: 0,
                total_fragments: 2,
                data: vec![1, 2, 3],
            },
            TransactionFragment {
                transaction_id: [1u8; 32],
                fragment_index: 1,
                total_fragments: 2,
                data: vec![4, 5, 6],
            },
        ];

        let peers = vec!["peer1".to_string(), "peer2".to_string()];
        let mut info = BroadcastInfo::new(fragments, peers);

        assert_eq!(info.status, BroadcastStatus::InProgress);
        assert_eq!(info.total_peers, 2);
        assert!(!info.is_complete());

        // Mark all fragments sent for all peers
        for peer_status in info.peer_status.values_mut() {
            peer_status.mark_sent(0);
            peer_status.mark_sent(1);
        }

        assert!(info.is_complete());
        assert_eq!(info.overall_completion(), 100.0);
    }
}
