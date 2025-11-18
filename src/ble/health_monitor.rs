use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Health monitor for BLE mesh network
///
/// Tracks:
/// - Network topology (peers and connections)
/// - Connection quality and latency
/// - Dead/stale peers
/// - Overall network health metrics
#[derive(Clone)]
pub struct MeshHealthMonitor {
    peers: Arc<RwLock<HashMap<String, PeerHealth>>>,
    topology: Arc<RwLock<NetworkTopology>>,
    metrics: Arc<RwLock<HealthMetrics>>,
    config: HealthConfig,
}

/// Health configuration
#[derive(Clone, Debug)]
pub struct HealthConfig {
    /// Maximum time without heartbeat before marking peer as stale
    pub stale_threshold: Duration,

    /// Maximum time without heartbeat before marking peer as dead
    pub dead_threshold: Duration,

    /// Number of latency samples to keep for averaging
    pub latency_sample_size: usize,

    /// Minimum signal strength (RSSI) for good connection
    pub min_good_rssi: i8,

    /// Minimum signal strength (RSSI) for acceptable connection
    pub min_acceptable_rssi: i8,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            stale_threshold: Duration::from_secs(30),
            dead_threshold: Duration::from_secs(120),
            latency_sample_size: 10,
            min_good_rssi: -70,
            min_acceptable_rssi: -85,
        }
    }
}

/// Health status of a single peer
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerHealth {
    /// Peer ID
    pub peer_id: String,

    /// Current connection state
    pub state: PeerState,

    /// Last heartbeat time (not serialized)
    #[serde(skip_serializing, skip_deserializing, default = "default_instant")]
    pub last_seen: Instant,

    /// Seconds since last heartbeat (for serialization)
    pub seconds_since_last_seen: u64,

    /// Recent latency samples (ms)
    pub latency_samples: Vec<u32>,

    /// Average latency (ms)
    pub avg_latency_ms: u32,

    /// Signal strength (RSSI)
    pub rssi: Option<i8>,

    /// Connection quality score (0-100)
    pub quality_score: u8,

    /// Number of packets sent
    pub packets_sent: u64,

    /// Number of packets received
    pub packets_received: u64,

    /// Number of transmission failures
    pub tx_failures: u64,

    /// Packet loss rate (0.0-1.0)
    pub packet_loss_rate: f32,
}

/// Peer connection state
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PeerState {
    Connected,
    Stale,
    Dead,
}

/// Network topology
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkTopology {
    /// Direct connections from this node
    pub direct_connections: Vec<String>,

    /// All known peers (including indirect)
    pub all_peers: Vec<String>,

    /// Connection graph (peer_id -> [connected_peer_ids])
    pub connections: HashMap<String, Vec<String>>,

    /// Hop count to each peer
    pub hop_counts: HashMap<String, u8>,
}

impl Default for NetworkTopology {
    fn default() -> Self {
        Self {
            direct_connections: Vec::new(),
            all_peers: Vec::new(),
            connections: HashMap::new(),
            hop_counts: HashMap::new(),
        }
    }
}

/// Overall network health metrics
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthMetrics {
    /// Total number of peers
    pub total_peers: usize,

    /// Number of connected peers
    pub connected_peers: usize,

    /// Number of stale peers
    pub stale_peers: usize,

    /// Number of dead peers
    pub dead_peers: usize,

    /// Average latency across all peers (ms)
    pub avg_latency_ms: u32,

    /// Maximum latency (ms)
    pub max_latency_ms: u32,

    /// Minimum latency (ms)
    pub min_latency_ms: u32,

    /// Average packet loss rate (0.0-1.0)
    pub avg_packet_loss: f32,

    /// Network health score (0-100)
    pub health_score: u8,

    /// Maximum hop count in network
    pub max_hops: u8,

    /// Timestamp of metrics
    pub timestamp: String,
}

impl Default for HealthMetrics {
    fn default() -> Self {
        Self {
            total_peers: 0,
            connected_peers: 0,
            stale_peers: 0,
            dead_peers: 0,
            avg_latency_ms: 0,
            max_latency_ms: 0,
            min_latency_ms: 0,
            avg_packet_loss: 0.0,
            health_score: 100,
            max_hops: 0,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Complete health snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub peers: Vec<PeerHealth>,
    pub topology: NetworkTopology,
    pub metrics: HealthMetrics,
}

impl MeshHealthMonitor {
    /// Create new health monitor
    pub fn new(config: HealthConfig) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            topology: Arc::new(RwLock::new(NetworkTopology::default())),
            metrics: Arc::new(RwLock::new(HealthMetrics::default())),
            config,
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(HealthConfig::default())
    }

    /// Record a peer heartbeat
    pub fn record_heartbeat(&self, peer_id: &str) {
        let mut peers = self.peers.write().unwrap();

        if let Some(peer) = peers.get_mut(peer_id) {
            peer.last_seen = Instant::now();
            peer.state = PeerState::Connected;
        } else {
            // New peer discovered
            peers.insert(
                peer_id.to_string(),
                PeerHealth {
                    peer_id: peer_id.to_string(),
                    state: PeerState::Connected,
                    last_seen: Instant::now(),
                    seconds_since_last_seen: 0,
                    latency_samples: Vec::new(),
                    avg_latency_ms: 0,
                    rssi: None,
                    quality_score: 100,
                    packets_sent: 0,
                    packets_received: 0,
                    tx_failures: 0,
                    packet_loss_rate: 0.0,
                },
            );
        }

        drop(peers);
        self.update_metrics();
    }

    /// Record latency measurement
    pub fn record_latency(&self, peer_id: &str, latency_ms: u32) {
        let mut peers = self.peers.write().unwrap();

        if let Some(peer) = peers.get_mut(peer_id) {
            peer.latency_samples.push(latency_ms);

            // Keep only last N samples
            if peer.latency_samples.len() > self.config.latency_sample_size {
                peer.latency_samples.remove(0);
            }

            // Update average
            peer.avg_latency_ms =
                peer.latency_samples.iter().sum::<u32>() / peer.latency_samples.len() as u32;

            // Update quality score based on latency
            self.update_quality_score(peer);
        }

        drop(peers);
        self.update_metrics();
    }

    /// Record RSSI (signal strength)
    pub fn record_rssi(&self, peer_id: &str, rssi: i8) {
        let mut peers = self.peers.write().unwrap();

        if let Some(peer) = peers.get_mut(peer_id) {
            peer.rssi = Some(rssi);
            self.update_quality_score(peer);
        }

        drop(peers);
        self.update_metrics();
    }

    /// Record packet transmission
    pub fn record_packet_sent(&self, peer_id: &str, success: bool) {
        let mut peers = self.peers.write().unwrap();

        if let Some(peer) = peers.get_mut(peer_id) {
            peer.packets_sent += 1;

            if !success {
                peer.tx_failures += 1;
            }

            // Update packet loss rate
            if peer.packets_sent > 0 {
                peer.packet_loss_rate = peer.tx_failures as f32 / peer.packets_sent as f32;
            }

            self.update_quality_score(peer);
        }

        drop(peers);
        self.update_metrics();
    }

    /// Record packet reception
    pub fn record_packet_received(&self, peer_id: &str) {
        let mut peers = self.peers.write().unwrap();

        if let Some(peer) = peers.get_mut(peer_id) {
            peer.packets_received += 1;
            peer.last_seen = Instant::now();
            peer.state = PeerState::Connected;
        }

        drop(peers);
        self.update_metrics();
    }

    /// Update network topology
    pub fn update_topology(&self, connections: HashMap<String, Vec<String>>) {
        let mut topology = self.topology.write().unwrap();

        topology.connections = connections;

        // Extract all unique peers
        let mut all_peers = std::collections::HashSet::new();
        for (peer, connected) in &topology.connections {
            all_peers.insert(peer.clone());
            for c in connected {
                all_peers.insert(c.clone());
            }
        }
        topology.all_peers = all_peers.into_iter().collect();

        // Calculate hop counts using BFS
        topology.hop_counts = self.calculate_hop_counts(&topology.connections);

        drop(topology);
        self.update_metrics();
    }

    /// Update direct connections
    pub fn update_direct_connections(&self, connections: Vec<String>) {
        let mut topology = self.topology.write().unwrap();
        topology.direct_connections = connections;
        drop(topology);
        self.update_metrics();
    }

    /// Check and update peer states based on last seen time
    pub fn check_stale_peers(&self) {
        let mut peers = self.peers.write().unwrap();
        let now = Instant::now();

        for peer in peers.values_mut() {
            let elapsed = now.duration_since(peer.last_seen);

            if elapsed > self.config.dead_threshold {
                peer.state = PeerState::Dead;
            } else if elapsed > self.config.stale_threshold {
                peer.state = PeerState::Stale;
            }
        }

        drop(peers);
        self.update_metrics();
    }

    /// Remove dead peers
    pub fn remove_dead_peers(&self) -> Vec<String> {
        let mut peers = self.peers.write().unwrap();

        let dead: Vec<String> = peers
            .iter()
            .filter(|(_, p)| p.state == PeerState::Dead)
            .map(|(id, _)| id.clone())
            .collect();

        for peer_id in &dead {
            peers.remove(peer_id);
        }

        drop(peers);
        self.update_metrics();

        dead
    }

    /// Get health snapshot
    pub fn get_snapshot(&self) -> HealthSnapshot {
        self.check_stale_peers();

        let peers = self.peers.read().unwrap();
        let topology = self.topology.read().unwrap();
        let metrics = self.metrics.read().unwrap();

        // Update seconds_since_last_seen for each peer
        let now = Instant::now();
        let peers_snapshot: Vec<PeerHealth> = peers
            .values()
            .map(|p| {
                let mut peer = p.clone();
                peer.seconds_since_last_seen = now.duration_since(p.last_seen).as_secs();
                peer
            })
            .collect();

        HealthSnapshot {
            peers: peers_snapshot,
            topology: topology.clone(),
            metrics: metrics.clone(),
        }
    }

    /// Get peer health
    pub fn get_peer_health(&self, peer_id: &str) -> Option<PeerHealth> {
        let peers = self.peers.read().unwrap();
        peers.get(peer_id).map(|p| {
            let mut peer = p.clone();
            peer.seconds_since_last_seen = Instant::now().duration_since(p.last_seen).as_secs();
            peer
        })
    }

    // Private helper methods

    fn update_quality_score(&self, peer: &mut PeerHealth) {
        let mut score = 100u8;

        // Latency penalty (0-30 points)
        if peer.avg_latency_ms > 0 {
            let latency_penalty = (peer.avg_latency_ms / 10).min(30);
            score = score.saturating_sub(latency_penalty as u8);
        }

        // RSSI penalty (0-30 points)
        if let Some(rssi) = peer.rssi {
            if rssi < self.config.min_acceptable_rssi {
                score = score.saturating_sub(30);
            } else if rssi < self.config.min_good_rssi {
                let rssi_penalty = ((self.config.min_good_rssi - rssi) * 2).min(30);
                score = score.saturating_sub(rssi_penalty as u8);
            }
        }

        // Packet loss penalty (0-40 points)
        let loss_penalty = (peer.packet_loss_rate * 40.0) as u8;
        score = score.saturating_sub(loss_penalty);

        peer.quality_score = score;
    }

    fn update_metrics(&self) {
        let peers = self.peers.read().unwrap();
        let topology = self.topology.read().unwrap();
        let mut metrics = self.metrics.write().unwrap();

        let total = peers.len();
        let connected = peers
            .values()
            .filter(|p| p.state == PeerState::Connected)
            .count();
        let stale = peers
            .values()
            .filter(|p| p.state == PeerState::Stale)
            .count();
        let dead = peers
            .values()
            .filter(|p| p.state == PeerState::Dead)
            .count();

        let latencies: Vec<u32> = peers
            .values()
            .filter(|p| p.avg_latency_ms > 0)
            .map(|p| p.avg_latency_ms)
            .collect();

        metrics.total_peers = total;
        metrics.connected_peers = connected;
        metrics.stale_peers = stale;
        metrics.dead_peers = dead;

        if !latencies.is_empty() {
            metrics.avg_latency_ms = latencies.iter().sum::<u32>() / latencies.len() as u32;
            metrics.max_latency_ms = *latencies.iter().max().unwrap_or(&0);
            metrics.min_latency_ms = *latencies.iter().min().unwrap_or(&0);
        }

        let total_packets: u64 = peers.values().map(|p| p.packets_sent).sum();
        let total_failures: u64 = peers.values().map(|p| p.tx_failures).sum();

        if total_packets > 0 {
            metrics.avg_packet_loss = total_failures as f32 / total_packets as f32;
        }

        metrics.max_hops = topology.hop_counts.values().max().copied().unwrap_or(0);

        // Calculate overall health score
        metrics.health_score = self.calculate_health_score(&peers, &metrics);

        metrics.timestamp = chrono::Utc::now().to_rfc3339();
    }

    fn calculate_health_score(
        &self,
        peers: &HashMap<String, PeerHealth>,
        metrics: &HealthMetrics,
    ) -> u8 {
        if peers.is_empty() {
            return 0;
        }

        let mut score = 100u8;

        // Penalty for dead/stale peers (0-30 points)
        let unhealthy_ratio =
            (metrics.stale_peers + metrics.dead_peers) as f32 / metrics.total_peers as f32;
        score = score.saturating_sub((unhealthy_ratio * 30.0) as u8);

        // Penalty for high latency (0-20 points)
        if metrics.avg_latency_ms > 100 {
            let latency_penalty = ((metrics.avg_latency_ms - 100) / 10).min(20);
            score = score.saturating_sub(latency_penalty as u8);
        }

        // Penalty for packet loss (0-30 points)
        score = score.saturating_sub((metrics.avg_packet_loss * 30.0) as u8);

        // Penalty for poor average peer quality (0-20 points)
        let avg_quality: u8 =
            peers.values().map(|p| p.quality_score).sum::<u8>() / peers.len() as u8;

        if avg_quality < 80 {
            score = score.saturating_sub((80 - avg_quality) / 4);
        }

        score
    }

    fn calculate_hop_counts(
        &self,
        connections: &HashMap<String, Vec<String>>,
    ) -> HashMap<String, u8> {
        let mut hop_counts = HashMap::new();
        let mut queue = std::collections::VecDeque::new();

        // Start from self (hop 0)
        let self_id = "self"; // This should be the actual node ID
        queue.push_back((self_id.to_string(), 0u8));
        hop_counts.insert(self_id.to_string(), 0);

        // BFS to find shortest path to all peers
        while let Some((node, hops)) = queue.pop_front() {
            if let Some(neighbors) = connections.get(&node) {
                for neighbor in neighbors {
                    if !hop_counts.contains_key(neighbor) {
                        hop_counts.insert(neighbor.clone(), hops + 1);
                        queue.push_back((neighbor.clone(), hops + 1));
                    }
                }
            }
        }

        hop_counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_monitor() {
        let monitor = MeshHealthMonitor::default();

        // Record heartbeats
        monitor.record_heartbeat("peer1");
        monitor.record_heartbeat("peer2");

        // Record latency
        monitor.record_latency("peer1", 50);
        monitor.record_latency("peer1", 55);
        monitor.record_latency("peer2", 100);

        // Record RSSI
        monitor.record_rssi("peer1", -60);
        monitor.record_rssi("peer2", -80);

        // Get snapshot
        let snapshot = monitor.get_snapshot();

        assert_eq!(snapshot.metrics.total_peers, 2);
        assert_eq!(snapshot.metrics.connected_peers, 2);
        assert!(snapshot.metrics.health_score > 70);
    }
}

fn default_instant() -> Instant {
    Instant::now()
}
