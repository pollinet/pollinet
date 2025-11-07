//! Peer Discovery and Connection Management
//!
//! Manages BLE peer discovery, connection establishment, and connection pooling

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;
use serde::{Deserialize, Serialize};

/// Minimum number of connections to maintain
pub const MIN_CONNECTIONS: usize = 1;

/// Target number of connections (optimal for mesh coverage)
pub const TARGET_CONNECTIONS: usize = 5;

/// Maximum number of simultaneous connections
pub const MAX_CONNECTIONS: usize = 8;

/// Peer discovery interval (seconds)
pub const DISCOVERY_INTERVAL: u64 = 10;

/// Peer timeout (seconds) - remove if not seen
pub const PEER_TIMEOUT: u64 = 30;

/// Connection retry delay (seconds)
pub const RETRY_DELAY: u64 = 5;

/// Maximum connection retries before backoff
pub const MAX_RETRIES: u32 = 3;

/// RSSI threshold for good connection quality
pub const GOOD_RSSI_THRESHOLD: i16 = -70;

/// RSSI threshold for acceptable connection
pub const MIN_RSSI_THRESHOLD: i16 = -90;

/// Peer information
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Unique peer ID
    pub peer_id: String,
    /// Peer device UUID (if known)
    pub device_uuid: Option<Uuid>,
    /// Peer capabilities
    pub capabilities: Vec<String>,
    /// Signal strength (RSSI)
    pub rssi: i16,
    /// First discovered timestamp
    pub first_seen: Instant,
    /// Last seen timestamp
    pub last_seen: Instant,
    /// Connection state
    pub state: PeerState,
    /// Connection attempts
    pub connection_attempts: u32,
    /// Last connection attempt
    pub last_attempt: Option<Instant>,
}

impl PeerInfo {
    pub fn new(peer_id: String, rssi: i16) -> Self {
        Self {
            peer_id,
            device_uuid: None,
            capabilities: vec!["CAN_RELAY".to_string()],
            rssi,
            first_seen: Instant::now(),
            last_seen: Instant::now(),
            state: PeerState::Discovered,
            connection_attempts: 0,
            last_attempt: None,
        }
    }

    /// Check if peer has good signal strength
    pub fn has_good_signal(&self) -> bool {
        self.rssi >= GOOD_RSSI_THRESHOLD
    }

    /// Check if peer has acceptable signal strength
    pub fn has_acceptable_signal(&self) -> bool {
        self.rssi >= MIN_RSSI_THRESHOLD
    }

    /// Check if peer has timed out
    pub fn is_expired(&self) -> bool {
        self.last_seen.elapsed() > Duration::from_secs(PEER_TIMEOUT)
    }

    /// Check if peer can be retried for connection
    pub fn can_retry(&self) -> bool {
        if self.connection_attempts >= MAX_RETRIES {
            return false;
        }

        if let Some(last_attempt) = self.last_attempt {
            last_attempt.elapsed() > Duration::from_secs(RETRY_DELAY)
        } else {
            true
        }
    }

    /// Update last seen timestamp
    pub fn update_seen(&mut self, rssi: i16) {
        self.last_seen = Instant::now();
        self.rssi = rssi;
    }

    /// Mark connection attempt
    pub fn mark_attempt(&mut self) {
        self.connection_attempts += 1;
        self.last_attempt = Some(Instant::now());
    }

    /// Reset connection attempts (on successful connection)
    pub fn reset_attempts(&mut self) {
        self.connection_attempts = 0;
        self.last_attempt = None;
    }
}

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerState {
    /// Peer discovered but not connected
    Discovered,
    /// Connection in progress
    Connecting,
    /// Successfully connected
    Connected,
    /// Connection failed
    Failed,
    /// Peer disconnected
    Disconnected,
}

/// Peer Manager - handles peer discovery and connection management
pub struct PeerManager {
    /// All discovered peers
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    /// Device ID of this node
    device_id: Uuid,
    /// Callbacks for peer events
    callbacks: Arc<RwLock<PeerCallbacks>>,
}

/// Callbacks for peer events
#[derive(Clone)]
pub struct PeerCallbacks {
    /// Called when a new peer is discovered
    pub on_peer_discovered: Option<Arc<dyn Fn(PeerInfo) + Send + Sync>>,
    /// Called when a peer connects
    pub on_peer_connected: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Called when a peer disconnects
    pub on_peer_disconnected: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl Default for PeerCallbacks {
    fn default() -> Self {
        Self {
            on_peer_discovered: None,
            on_peer_connected: None,
            on_peer_disconnected: None,
        }
    }
}

impl PeerManager {
    pub fn new(device_id: Uuid) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            device_id,
            callbacks: Arc::new(RwLock::new(PeerCallbacks::default())),
        }
    }

    /// Set callbacks for peer events
    pub async fn set_callbacks(&self, callbacks: PeerCallbacks) {
        let mut cb = self.callbacks.write().await;
        *cb = callbacks;
    }

    /// Add or update a discovered peer
    pub async fn add_peer(&self, peer_id: String, rssi: i16) {
        let mut peers = self.peers.write().await;
        
        let is_new = !peers.contains_key(&peer_id);
        
        let peer = peers.entry(peer_id.clone()).or_insert_with(|| {
            tracing::info!("🔍 Discovered new peer: {} (RSSI: {})", peer_id, rssi);
            PeerInfo::new(peer_id.clone(), rssi)
        });

        peer.update_seen(rssi);

        // Call callback for new peers
        if is_new {
            let callbacks = self.callbacks.read().await;
            if let Some(callback) = &callbacks.on_peer_discovered {
                callback(peer.clone());
            }
        }
    }

    /// Get a peer by ID
    pub async fn get_peer(&self, peer_id: &str) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(peer_id).cloned()
    }

    /// Get all discovered peers
    pub async fn get_all_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    /// Get connected peers
    pub async fn get_connected_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values()
            .filter(|p| p.state == PeerState::Connected)
            .cloned()
            .collect()
    }

    /// Get peers suitable for connection (sorted by priority)
    pub async fn get_connection_candidates(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        let mut candidates: Vec<PeerInfo> = peers.values()
            .filter(|p| {
                // Must be discovered or disconnected
                matches!(p.state, PeerState::Discovered | PeerState::Disconnected | PeerState::Failed) &&
                // Must have acceptable signal
                p.has_acceptable_signal() &&
                // Must not be expired
                !p.is_expired() &&
                // Must be able to retry
                p.can_retry()
            })
            .cloned()
            .collect();

        // Sort by priority:
        // 1. Good RSSI first
        // 2. Fewer connection attempts
        // 3. More recently seen
        candidates.sort_by(|a, b| {
            // First, prioritize good signal
            let a_good = a.has_good_signal();
            let b_good = b.has_good_signal();
            if a_good != b_good {
                return b_good.cmp(&a_good);
            }

            // Then, fewer attempts
            let attempt_cmp = a.connection_attempts.cmp(&b.connection_attempts);
            if attempt_cmp != std::cmp::Ordering::Equal {
                return attempt_cmp;
            }

            // Finally, more recent
            b.last_seen.cmp(&a.last_seen)
        });

        candidates
    }

    /// Get number of connected peers
    pub async fn get_connected_count(&self) -> usize {
        let peers = self.peers.read().await;
        peers.values()
            .filter(|p| p.state == PeerState::Connected)
            .count()
    }

    /// Check if we need more connections
    pub async fn needs_more_connections(&self) -> bool {
        self.get_connected_count().await < TARGET_CONNECTIONS
    }

    /// Mark peer as connecting
    pub async fn mark_connecting(&self, peer_id: &str) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.state = PeerState::Connecting;
            peer.mark_attempt();
            tracing::info!("🔄 Connecting to peer: {} (attempt {})", peer_id, peer.connection_attempts);
        }
    }

    /// Mark peer as connected
    pub async fn mark_connected(&self, peer_id: &str) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.state = PeerState::Connected;
            peer.reset_attempts();
            tracing::info!("✅ Connected to peer: {}", peer_id);

            // Call callback
            drop(peers); // Release lock before callback
            let callbacks = self.callbacks.read().await;
            if let Some(callback) = &callbacks.on_peer_connected {
                callback(peer_id.to_string());
            }
        }
    }

    /// Mark peer as disconnected
    pub async fn mark_disconnected(&self, peer_id: &str) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.state = PeerState::Disconnected;
            tracing::warn!("❌ Peer disconnected: {}", peer_id);

            // Call callback
            drop(peers); // Release lock before callback
            let callbacks = self.callbacks.read().await;
            if let Some(callback) = &callbacks.on_peer_disconnected {
                callback(peer_id.to_string());
            }
        }
    }

    /// Mark peer connection as failed
    pub async fn mark_failed(&self, peer_id: &str) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.state = PeerState::Failed;
            tracing::error!("⚠️  Connection failed: {} (attempt {})", peer_id, peer.connection_attempts);
        }
    }

    /// Clean up expired peers
    pub async fn cleanup_expired(&self) {
        let mut peers = self.peers.write().await;
        let before = peers.len();
        peers.retain(|_, peer| !peer.is_expired());
        let after = peers.len();

        if before != after {
            tracing::info!("🧹 Cleaned up {} expired peers", before - after);
        }
    }

    /// Get peer manager statistics
    pub async fn get_stats(&self) -> PeerManagerStats {
        let peers = self.peers.read().await;
        
        let total_peers = peers.len();
        let connected_peers = peers.values().filter(|p| p.state == PeerState::Connected).count();
        let connecting_peers = peers.values().filter(|p| p.state == PeerState::Connecting).count();
        let failed_peers = peers.values().filter(|p| p.state == PeerState::Failed).count();

        let avg_rssi = if !peers.is_empty() {
            peers.values().map(|p| p.rssi as i32).sum::<i32>() / peers.len() as i32
        } else {
            0
        };

        PeerManagerStats {
            device_id: self.device_id,
            total_peers,
            connected_peers,
            connecting_peers,
            failed_peers,
            avg_rssi: avg_rssi as i16,
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> Uuid {
        self.device_id
    }
}

/// Peer manager statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerManagerStats {
    pub device_id: Uuid,
    pub total_peers: usize,
    pub connected_peers: usize,
    pub connecting_peers: usize,
    pub failed_peers: usize,
    pub avg_rssi: i16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_peer_discovery() {
        let manager = PeerManager::new(Uuid::new_v4());
        
        // Add some peers
        manager.add_peer("peer1".to_string(), -60).await;
        manager.add_peer("peer2".to_string(), -80).await;
        manager.add_peer("peer3".to_string(), -95).await;

        let all_peers = manager.get_all_peers().await;
        assert_eq!(all_peers.len(), 3);

        // Check connection candidates (peer3 should be excluded due to low RSSI)
        let candidates = manager.get_connection_candidates().await;
        assert_eq!(candidates.len(), 2);
        
        // First candidate should be peer1 (best RSSI)
        assert_eq!(candidates[0].peer_id, "peer1");
    }

    #[tokio::test]
    async fn test_connection_state_transitions() {
        let manager = PeerManager::new(Uuid::new_v4());
        
        manager.add_peer("peer1".to_string(), -60).await;
        
        // Mark connecting
        manager.mark_connecting("peer1").await;
        let peer = manager.get_peer("peer1").await.unwrap();
        assert_eq!(peer.state, PeerState::Connecting);
        assert_eq!(peer.connection_attempts, 1);

        // Mark connected
        manager.mark_connected("peer1").await;
        let peer = manager.get_peer("peer1").await.unwrap();
        assert_eq!(peer.state, PeerState::Connected);
        assert_eq!(peer.connection_attempts, 0); // Reset on success

        assert_eq!(manager.get_connected_count().await, 1);
    }

    #[tokio::test]
    async fn test_retry_logic() {
        let manager = PeerManager::new(Uuid::new_v4());
        
        manager.add_peer("peer1".to_string(), -60).await;
        
        // Mark multiple failed attempts
        for _ in 0..MAX_RETRIES {
            manager.mark_connecting("peer1").await;
            manager.mark_failed("peer1").await;
        }

        let peer = manager.get_peer("peer1").await.unwrap();
        assert_eq!(peer.connection_attempts, MAX_RETRIES);
        assert!(!peer.can_retry()); // Should not be able to retry anymore

        // Should not be in connection candidates
        let candidates = manager.get_connection_candidates().await;
        assert_eq!(candidates.len(), 0);
    }
}

