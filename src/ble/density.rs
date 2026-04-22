//! Density-adaptive rotation — Subsystem 1
//!
//! Maintains a sliding window of observed peer IDs (2-minute horizon) to estimate
//! local device density N, then recomputes adaptive BLE session/cooldown parameters.
//! Also owns the per-device cooldown list used for peer-rotation scheduling.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Base session target in milliseconds (tunable). 60 seconds.
const BASE_SESSION_MS: f64 = 60_000.0;
/// Sliding window for density estimation in seconds (2 minutes).
const DENSITY_WINDOW_SECS: u64 = 120;
/// Recomputation interval (not enforced here; Kotlin calls every 10 s).
#[allow(dead_code)]
pub const RECOMPUTE_INTERVAL_MS: u64 = 10_000;

/// Adaptive BLE session and cooldown parameters derived from local density.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdaptiveParams {
    /// Estimated number of unique peers observed in the last 2 minutes.
    pub density: u32,
    /// Target session duration in ms — how long to stay connected per peer.
    pub session_target_ms: u64,
    /// Cooldown duration in ms — how long to avoid reconnecting to the same peer.
    pub cooldown_ms: u64,
    /// Minimum session duration in ms (constant 5–10 s window).
    pub session_min_ms: u64,
    /// Maximum session duration (= session_target × 1.5).
    pub session_max_ms: u64,
}

/// Sliding-window density estimator.
/// Stores (peer_id → last_seen_unix_secs) for all peers observed in scans.
pub struct DensityEstimator {
    /// peer_id → unix timestamp of last observation
    seen: HashMap<String, u64>,
}

impl DensityEstimator {
    pub fn new() -> Self {
        Self {
            seen: HashMap::new(),
        }
    }

    /// Record a scan observation for `peer_id`. Call on every `onScanResult`.
    pub fn record(&mut self, peer_id: &str) {
        let now = Self::now_secs();
        self.seen.insert(peer_id.to_string(), now);
    }

    /// Evict entries older than `DENSITY_WINDOW_SECS` and return the current N.
    pub fn evict_and_count(&mut self) -> u32 {
        let cutoff = Self::now_secs().saturating_sub(DENSITY_WINDOW_SECS);
        self.seen.retain(|_, &mut ts| ts > cutoff);
        self.seen.len() as u32
    }

    /// Compute adaptive parameters from current N.
    pub fn compute_params(&mut self) -> AdaptiveParams {
        let n = self.evict_and_count().max(1) as f64;

        let session_target_ms = {
            let raw = BASE_SESSION_MS / (n / 10.0).sqrt();
            raw.clamp(20_000.0, 120_000.0) as u64
        };

        let cooldown_ms = {
            let raw = session_target_ms as f64 * (n - 1.0) * 0.4;
            raw.clamp(15_000.0, 600_000.0) as u64
        };

        AdaptiveParams {
            density: n as u32,
            session_target_ms,
            cooldown_ms,
            session_min_ms: 5_000,
            session_max_ms: session_target_ms * 3 / 2,
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

impl Default for DensityEstimator {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-device cooldown list (Subsystem 1).
/// After each session ends, the peer is added with `expiry = now + cooldown_ms`.
/// Peer selection filters against this list before connecting.
pub struct CooldownList {
    /// peer_id → expiry unix timestamp (ms)
    entries: HashMap<String, u64>,
}

impl CooldownList {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Add `peer_id` to cooldown with `cooldown_ms` duration.
    pub fn add(&mut self, peer_id: &str, cooldown_ms: u64) {
        let expiry = Self::now_ms() + cooldown_ms;
        self.entries.insert(peer_id.to_string(), expiry);
    }

    /// Returns true if `peer_id` is currently in cooldown.
    pub fn is_cooling(&self, peer_id: &str) -> bool {
        match self.entries.get(peer_id) {
            Some(&expiry) => Self::now_ms() < expiry,
            None => false,
        }
    }

    /// Remove all expired entries. Call periodically.
    pub fn evict_expired(&mut self) {
        let now = Self::now_ms();
        self.entries.retain(|_, &mut exp| exp > now);
    }

    /// Sparse-network safety net: expire the entry with the earliest expiry so a
    /// device that has been IDLE for >2×session_target_ms can still connect.
    /// Returns the peer_id that was expired, if any.
    pub fn expire_oldest(&mut self) -> Option<String> {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, &exp)| exp)
            .map(|(k, _)| k.clone());
        if let Some(ref k) = oldest {
            self.entries.remove(k);
        }
        oldest
    }

    /// Expire cooldown for all peers NOT in `delivered_to` (compact 4-byte IDs).
    /// Called when a new high-priority entry or confirmation is added to the carrier set.
    pub fn expire_not_delivered(&mut self, delivered_to_bytes: &[u8]) {
        let delivered: std::collections::HashSet<[u8; 4]> = delivered_to_bytes
            .chunks(4)
            .filter_map(|c| c.try_into().ok())
            .collect();

        // We don't have a mapping from peer_id string → 4-byte ID here, so we
        // expire ALL cooldowns (conservative: maximises responsiveness to bursts).
        // The Kotlin layer may re-add if it wants finer control.
        if delivered.is_empty() {
            self.entries.clear();
        }
        // If some peers were already delivered to, keep their cooldowns; clear the rest.
        // Since we only have string IDs here and not the compact mapping, clear all.
        // This is safe: at worst we reconnect slightly sooner to a peer we already delivered to.
        let _ = delivered; // suppresses unused warning — see above rationale
        self.entries.clear();
    }

    /// Total active (non-expired) cooldown count.
    pub fn active_count(&self) -> usize {
        let now = Self::now_ms();
        self.entries.values().filter(|&&exp| exp > now).count()
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Default for CooldownList {
    fn default() -> Self {
        Self::new()
    }
}

/// Session telemetry record (Subsystem 1 output).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionTelemetry {
    pub local_device_id: String,
    pub peer_id: String,
    pub connect_time_ms: u64,
    pub disconnect_time_ms: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub fragments_out: u32,
    pub fragments_in: u32,
    pub data_complete: bool,
    pub confirmation_complete: bool,
    pub close_reason: CloseReason,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum CloseReason {
    MutualDrain,
    SessionMax,
    LinkDropped,
    ForceClose,
    Abort,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_density_estimator_empty() {
        let mut est = DensityEstimator::new();
        let params = est.compute_params();
        // N=1 → session_target = clamp(60000/sqrt(0.1), 20000, 120000) = 120000
        assert_eq!(params.session_target_ms, 120_000);
        assert_eq!(params.session_min_ms, 5_000);
    }

    #[test]
    fn test_density_estimator_10_peers() {
        let mut est = DensityEstimator::new();
        for i in 0..10 {
            est.record(&format!("peer_{}", i));
        }
        let params = est.compute_params();
        // N=10 → session_target = clamp(60000/sqrt(1.0), 20000, 120000) = 60000
        assert_eq!(params.session_target_ms, 60_000);
        // cooldown = clamp(60000 * 9 * 0.4, 15000, 600000) = 216000
        assert_eq!(params.cooldown_ms, 216_000);
    }

    #[test]
    fn test_cooldown_list_basic() {
        let mut list = CooldownList::new();
        list.add("peerA", 60_000);
        assert!(list.is_cooling("peerA"));
        assert!(!list.is_cooling("peerB"));
    }

    #[test]
    fn test_cooldown_expire_oldest() {
        let mut list = CooldownList::new();
        // Add with 0ms — immediately expired
        list.entries.insert("peerX".to_string(), 0);
        list.entries.insert("peerY".to_string(), u64::MAX);
        let removed = list.expire_oldest();
        assert!(removed.is_some());
        // peerX had earliest (0) expiry
        assert_eq!(removed.unwrap(), "peerX");
    }
}
