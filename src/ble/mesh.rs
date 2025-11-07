//! BLE Mesh Networking Module
//!
//! Implements the PolliNet mesh protocol for peer-to-peer transaction broadcasting

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;
use serde::{Deserialize, Serialize};

/// Maximum number of hops a message can traverse
pub const MAX_HOPS: u8 = 10;

/// Default TTL for new messages
pub const DEFAULT_TTL: u8 = 10;

/// Maximum fragments per transaction
pub const MAX_FRAGMENTS: u16 = 100;

/// Maximum payload size per packet (bytes)
pub const MAX_PAYLOAD_SIZE: usize = 512;

/// Mesh packet header size (bytes)
pub const HEADER_SIZE: usize = 42;

/// Maximum usable fragment data size (bytes)
pub const MAX_FRAGMENT_DATA: usize = MAX_PAYLOAD_SIZE - HEADER_SIZE - 6;

/// Maximum incomplete transactions in buffer
pub const MAX_INCOMPLETE_TRANSACTIONS: usize = 50;

/// Timeout for incomplete transactions (seconds)
pub const REASSEMBLY_TIMEOUT: u64 = 60;

/// Seen message cache size
pub const SEEN_CACHE_SIZE: usize = 1000;

/// Seen message TTL (seconds)
pub const SEEN_CACHE_TTL: u64 = 600;

/// Mesh packet types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PacketType {
    Ping = 0x01,
    Pong = 0x02,
    TransactionFragment = 0x03,
    TransactionAck = 0x04,
    TopologyQuery = 0x05,
    TopologyResponse = 0x06,
    TextMessage = 0x07,
}

impl PacketType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(PacketType::Ping),
            0x02 => Some(PacketType::Pong),
            0x03 => Some(PacketType::TransactionFragment),
            0x04 => Some(PacketType::TransactionAck),
            0x05 => Some(PacketType::TopologyQuery),
            0x06 => Some(PacketType::TopologyResponse),
            0x07 => Some(PacketType::TextMessage),
            _ => None,
        }
    }
}

/// Mesh packet header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshHeader {
    /// Packet type
    pub packet_type: PacketType,
    /// Protocol version
    pub version: u8,
    /// Time-to-live (hops remaining)
    pub ttl: u8,
    /// Number of hops traversed
    pub hop_count: u8,
    /// Unique message ID
    pub message_id: Uuid,
    /// Original sender device ID
    pub sender_id: Uuid,
}

impl MeshHeader {
    pub fn new(packet_type: PacketType, sender_id: Uuid) -> Self {
        Self {
            packet_type,
            version: 1,
            ttl: DEFAULT_TTL,
            hop_count: 0,
            message_id: Uuid::new_v4(),
            sender_id,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_SIZE);
        bytes.push(self.packet_type as u8);
        bytes.push(self.version);
        bytes.push(self.ttl);
        bytes.push(self.hop_count);
        bytes.extend_from_slice(&[0u8; 6]); // Reserved
        bytes.extend_from_slice(self.message_id.as_bytes());
        bytes.extend_from_slice(self.sender_id.as_bytes());
        bytes
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, MeshError> {
        if bytes.len() < HEADER_SIZE {
            return Err(MeshError::InvalidPacket("Header too short".into()));
        }

        let packet_type = PacketType::from_u8(bytes[0])
            .ok_or_else(|| MeshError::InvalidPacket("Unknown packet type".into()))?;
        
        let version = bytes[1];
        let ttl = bytes[2];
        let hop_count = bytes[3];
        
        let message_id = Uuid::from_slice(&bytes[10..26])
            .map_err(|e| MeshError::InvalidPacket(format!("Invalid message ID: {}", e)))?;
        
        let sender_id = Uuid::from_slice(&bytes[26..42])
            .map_err(|e| MeshError::InvalidPacket(format!("Invalid sender ID: {}", e)))?;

        Ok(Self {
            packet_type,
            version,
            ttl,
            hop_count,
            message_id,
            sender_id,
        })
    }

    /// Decrement TTL and increment hop count for forwarding
    pub fn prepare_for_forward(&mut self) {
        if self.ttl > 0 {
            self.ttl -= 1;
        }
        self.hop_count += 1;
    }
}

/// Complete mesh packet
#[derive(Debug, Clone)]
pub struct MeshPacket {
    pub header: MeshHeader,
    pub payload: Vec<u8>,
}

impl MeshPacket {
    pub fn new(packet_type: PacketType, sender_id: Uuid, payload: Vec<u8>) -> Self {
        Self {
            header: MeshHeader::new(packet_type, sender_id),
            payload,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = self.header.serialize();
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, MeshError> {
        let header = MeshHeader::deserialize(bytes)?;
        let payload = bytes[HEADER_SIZE..].to_vec();
        Ok(Self { header, payload })
    }
}

/// Transaction fragment payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionFragment {
    /// SHA256 hash of complete transaction
    pub transaction_id: [u8; 32],
    /// Fragment index (0-based)
    pub fragment_index: u16,
    /// Total number of fragments
    pub total_fragments: u16,
    /// Fragment data
    pub data: Vec<u8>,
}

impl TransactionFragment {
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.transaction_id);
        bytes.extend_from_slice(&self.fragment_index.to_be_bytes());
        bytes.extend_from_slice(&self.total_fragments.to_be_bytes());
        bytes.extend_from_slice(&(self.data.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, MeshError> {
        if bytes.len() < 38 {
            return Err(MeshError::InvalidPacket("Fragment payload too short".into()));
        }

        let mut transaction_id = [0u8; 32];
        transaction_id.copy_from_slice(&bytes[0..32]);
        
        let fragment_index = u16::from_be_bytes([bytes[32], bytes[33]]);
        let total_fragments = u16::from_be_bytes([bytes[34], bytes[35]]);
        let data_len = u16::from_be_bytes([bytes[36], bytes[37]]) as usize;
        
        if bytes.len() < 38 + data_len {
            return Err(MeshError::InvalidPacket("Fragment data truncated".into()));
        }
        
        let data = bytes[38..38 + data_len].to_vec();

        Ok(Self {
            transaction_id,
            fragment_index,
            total_fragments,
            data,
        })
    }
}

/// Incomplete transaction being reassembled
#[derive(Debug, Clone)]
struct IncompleteTransaction {
    transaction_id: [u8; 32],
    total_fragments: u16,
    received_fragments: HashSet<u16>,
    fragments: HashMap<u16, Vec<u8>>,
    first_seen: Instant,
    last_fragment: Instant,
}

impl IncompleteTransaction {
    fn new(transaction_id: [u8; 32], total_fragments: u16) -> Self {
        Self {
            transaction_id,
            total_fragments,
            received_fragments: HashSet::new(),
            fragments: HashMap::new(),
            first_seen: Instant::now(),
            last_fragment: Instant::now(),
        }
    }

    fn add_fragment(&mut self, index: u16, data: Vec<u8>) {
        if !self.received_fragments.contains(&index) {
            self.received_fragments.insert(index);
            self.fragments.insert(index, data);
            self.last_fragment = Instant::now();
        }
    }

    fn is_complete(&self) -> bool {
        self.received_fragments.len() == self.total_fragments as usize
    }

    fn is_expired(&self) -> bool {
        self.first_seen.elapsed() > Duration::from_secs(REASSEMBLY_TIMEOUT)
    }

    fn reconstruct(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }

        let mut result = Vec::new();
        for i in 0..self.total_fragments {
            if let Some(data) = self.fragments.get(&i) {
                result.extend_from_slice(data);
            } else {
                return None; // Missing fragment
            }
        }

        Some(result)
    }
}

/// Seen message cache entry
#[derive(Debug, Clone)]
struct SeenMessage {
    seen_at: Instant,
    hop_count: u8,
}

/// BLE Mesh Router
pub struct MeshRouter {
    device_id: Uuid,
    seen_cache: Arc<RwLock<HashMap<Uuid, SeenMessage>>>,
    incomplete_transactions: Arc<RwLock<HashMap<[u8; 32], IncompleteTransaction>>>,
    completed_transactions: Arc<RwLock<Vec<Vec<u8>>>>,
}

impl MeshRouter {
    pub fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            seen_cache: Arc::new(RwLock::new(HashMap::new())),
            incomplete_transactions: Arc::new(RwLock::new(HashMap::new())),
            completed_transactions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Check if a message should be forwarded
    pub async fn should_forward(&self, header: &MeshHeader, sender_peer_id: &str) -> bool {
        // Check seen cache
        {
            let cache = self.seen_cache.read().await;
            if cache.contains_key(&header.message_id) {
                tracing::debug!("Message {} already seen, dropping", header.message_id);
                return false;
            }
        }

        // Check TTL
        if header.ttl == 0 {
            tracing::debug!("Message {} TTL exhausted, dropping", header.message_id);
            return false;
        }

        // Check hop count
        if header.hop_count >= MAX_HOPS {
            tracing::debug!("Message {} exceeded max hops, dropping", header.message_id);
            return false;
        }

        tracing::debug!(
            "Message {} should be forwarded (TTL={}, hops={}, from={})",
            header.message_id,
            header.ttl,
            header.hop_count,
            sender_peer_id
        );

        true
    }

    /// Mark message as seen
    pub async fn mark_seen(&self, message_id: Uuid, hop_count: u8) {
        let mut cache = self.seen_cache.write().await;
        
        // Evict old entries if cache is full
        if cache.len() >= SEEN_CACHE_SIZE {
            let now = Instant::now();
            cache.retain(|_, v| now.duration_since(v.seen_at) < Duration::from_secs(SEEN_CACHE_TTL));
        }

        cache.insert(message_id, SeenMessage {
            seen_at: Instant::now(),
            hop_count,
        });
    }

    /// Process received transaction fragment
    pub async fn process_fragment(&self, fragment: TransactionFragment) -> Result<Option<Vec<u8>>, MeshError> {
        tracing::info!(
            "Processing fragment {}/{} for transaction {:?}",
            fragment.fragment_index + 1,
            fragment.total_fragments,
            hex::encode(&fragment.transaction_id[..8])
        );

        // Validate fragment
        if fragment.fragment_index >= fragment.total_fragments {
            return Err(MeshError::InvalidFragment("Fragment index out of range".into()));
        }

        if fragment.total_fragments > MAX_FRAGMENTS {
            return Err(MeshError::InvalidFragment("Too many fragments".into()));
        }

        let mut incomplete = self.incomplete_transactions.write().await;

        // Get or create incomplete transaction
        let tx = incomplete.entry(fragment.transaction_id).or_insert_with(|| {
            IncompleteTransaction::new(fragment.transaction_id, fragment.total_fragments)
        });

        // Add fragment
        tx.add_fragment(fragment.fragment_index, fragment.data);

        // Check if complete
        if tx.is_complete() {
            tracing::info!("Transaction {:?} complete! Reconstructing...", hex::encode(&fragment.transaction_id[..8]));
            
            if let Some(reconstructed) = tx.reconstruct() {
                // Move to completed transactions
                let mut completed = self.completed_transactions.write().await;
                completed.push(reconstructed.clone());
                
                // Remove from incomplete
                incomplete.remove(&fragment.transaction_id);
                
                tracing::info!("✅ Transaction reconstructed: {} bytes", reconstructed.len());
                return Ok(Some(reconstructed));
            }
        } else {
            tracing::debug!(
                "Transaction {:?} progress: {}/{}",
                hex::encode(&fragment.transaction_id[..8]),
                tx.received_fragments.len(),
                tx.total_fragments
            );
        }

        Ok(None)
    }

    /// Clean up expired incomplete transactions
    pub async fn cleanup_expired(&self) {
        let mut incomplete = self.incomplete_transactions.write().await;
        let before = incomplete.len();
        incomplete.retain(|_, tx| !tx.is_expired());
        let after = incomplete.len();
        
        if before != after {
            tracing::info!("Cleaned up {} expired incomplete transactions", before - after);
        }
    }

    /// Get statistics
    pub async fn get_stats(&self) -> MeshStats {
        let seen_cache = self.seen_cache.read().await;
        let incomplete = self.incomplete_transactions.read().await;
        let completed = self.completed_transactions.read().await;

        MeshStats {
            device_id: self.device_id,
            seen_messages: seen_cache.len(),
            incomplete_transactions: incomplete.len(),
            completed_transactions: completed.len(),
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> Uuid {
        self.device_id
    }

    /// Get completed transactions
    pub async fn get_completed_transactions(&self) -> Vec<Vec<u8>> {
        let completed = self.completed_transactions.read().await;
        completed.clone()
    }

    /// Clear completed transactions
    pub async fn clear_completed_transactions(&self) {
        let mut completed = self.completed_transactions.write().await;
        completed.clear();
    }
}

/// Mesh router statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStats {
    pub device_id: Uuid,
    pub seen_messages: usize,
    pub incomplete_transactions: usize,
    pub completed_transactions: usize,
}

/// Mesh-specific errors
#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),
    
    #[error("Invalid fragment: {0}")]
    InvalidFragment(String),
    
    #[error("Reassembly failed: {0}")]
    ReassemblyFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization() {
        let sender_id = Uuid::new_v4();
        let header = MeshHeader::new(PacketType::Ping, sender_id);
        
        let bytes = header.serialize();
        assert_eq!(bytes.len(), HEADER_SIZE);
        
        let deserialized = MeshHeader::deserialize(&bytes).unwrap();
        assert_eq!(deserialized.packet_type, PacketType::Ping);
        assert_eq!(deserialized.sender_id, sender_id);
    }

    #[test]
    fn test_packet_serialization() {
        let sender_id = Uuid::new_v4();
        let payload = vec![1, 2, 3, 4, 5];
        let packet = MeshPacket::new(PacketType::TextMessage, sender_id, payload.clone());
        
        let bytes = packet.serialize();
        let deserialized = MeshPacket::deserialize(&bytes).unwrap();
        
        assert_eq!(deserialized.header.packet_type, PacketType::TextMessage);
        assert_eq!(deserialized.payload, payload);
    }

    #[test]
    fn test_fragment_serialization() {
        let fragment = TransactionFragment {
            transaction_id: [42u8; 32],
            fragment_index: 0,
            total_fragments: 3,
            data: vec![1, 2, 3],
        };
        
        let bytes = fragment.serialize();
        let deserialized = TransactionFragment::deserialize(&bytes).unwrap();
        
        assert_eq!(deserialized.transaction_id, fragment.transaction_id);
        assert_eq!(deserialized.fragment_index, fragment.fragment_index);
        assert_eq!(deserialized.total_fragments, fragment.total_fragments);
        assert_eq!(deserialized.data, fragment.data);
    }

    #[tokio::test]
    async fn test_fragment_reassembly() {
        let router = MeshRouter::new(Uuid::new_v4());
        let tx_id = [1u8; 32];
        
        // Send fragments
        for i in 0..3 {
            let fragment = TransactionFragment {
                transaction_id: tx_id,
                fragment_index: i,
                total_fragments: 3,
                data: vec![i as u8; 10],
            };
            
            let result = router.process_fragment(fragment).await.unwrap();
            
            if i == 2 {
                assert!(result.is_some());
                let reconstructed = result.unwrap();
                assert_eq!(reconstructed.len(), 30); // 3 * 10 bytes
            } else {
                assert!(result.is_none());
            }
        }
    }
}

