//! Confirmation Relay Queue
//!
//! Queue for transaction confirmations to be relayed back to origin devices.
//! Implements FIFO ordering with hop count tracking and TTL management.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// Confirmation status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfirmationStatus {
    /// Transaction successfully submitted
    Success { signature: String },
    /// Transaction submission failed
    Failed { error: String },
}

/// Transaction confirmation for relay back to origin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Confirmation {
    /// Original transaction ID (SHA-256 hash)
    pub original_tx_id: [u8; 32],
    /// Confirmation status (success or failure)
    pub status: ConfirmationStatus,
    /// Unix timestamp when confirmation was created
    pub timestamp: u64,
    /// Number of relay hops (for mesh routing)
    pub relay_count: u8,
    /// Maximum hops allowed (TTL)
    pub max_hops: u8,
}

impl Confirmation {
    /// Create new success confirmation
    pub fn success(original_tx_id: [u8; 32], signature: String) -> Self {
        Self::new(
            original_tx_id,
            ConfirmationStatus::Success { signature },
        )
    }
    
    /// Create new failure confirmation
    pub fn failure(original_tx_id: [u8; 32], error: String) -> Self {
        Self::new(
            original_tx_id,
            ConfirmationStatus::Failed { error },
        )
    }
    
    /// Create new confirmation with status
    pub fn new(original_tx_id: [u8; 32], status: ConfirmationStatus) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            original_tx_id,
            status,
            timestamp: now,
            relay_count: 0,
            max_hops: 5, // Default max 5 hops
        }
    }
    
    /// Check if confirmation has exceeded max hops
    pub fn has_exceeded_hops(&self) -> bool {
        self.relay_count >= self.max_hops
    }
    
    /// Increment relay count (returns false if exceeded)
    pub fn increment_relay(&mut self) -> bool {
        if self.has_exceeded_hops() {
            return false;
        }
        self.relay_count += 1;
        true
    }
    
    /// Get age in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.timestamp)
    }
    
    /// Check if confirmation is expired (older than TTL)
    pub fn is_expired(&self, ttl_seconds: u64) -> bool {
        self.age_seconds() > ttl_seconds
    }
    
    /// Get transaction ID as hex string
    pub fn tx_id_hex(&self) -> String {
        hex::encode(&self.original_tx_id)
    }
}

/// Confirmation queue (FIFO with TTL management)
pub struct ConfirmationQueue {
    /// Pending confirmations (FIFO order)
    pending: VecDeque<Confirmation>,
    /// Maximum queue size
    max_size: usize,
    /// Default TTL in seconds (1 hour)
    default_ttl: u64,
}

impl ConfirmationQueue {
    /// Create new confirmation queue with default capacity (500)
    pub fn new() -> Self {
        Self::with_capacity(500)
    }
    
    /// Create new confirmation queue with specified capacity
    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_size,
            default_ttl: 3600, // 1 hour
        }
    }
    
    /// Create queue with custom TTL
    pub fn with_ttl(max_size: usize, ttl_seconds: u64) -> Self {
        Self {
            pending: VecDeque::new(),
            max_size,
            default_ttl: ttl_seconds,
        }
    }
    
    /// Push confirmation to queue
    pub fn push(&mut self, confirmation: Confirmation) -> Result<(), ConfirmationError> {
        // Check queue size
        if self.pending.len() >= self.max_size {
            // Try to make room by removing oldest confirmation
            if let Some(dropped) = self.pending.pop_front() {
                tracing::warn!(
                    "Confirmation queue full ({}), dropped oldest confirmation for tx {}",
                    self.max_size,
                    dropped.tx_id_hex().chars().take(8).collect::<String>()
                );
            } else {
                return Err(ConfirmationError::QueueFull(self.max_size));
            }
        }
        
        // Check hop count
        if confirmation.has_exceeded_hops() {
            return Err(ConfirmationError::MaxHopsExceeded {
                tx_id: confirmation.tx_id_hex(),
                hops: confirmation.relay_count,
                max_hops: confirmation.max_hops,
            });
        }
        
        tracing::debug!(
            "Queued confirmation for tx {} (hops: {}/{}, queue size: {})",
            confirmation.tx_id_hex().chars().take(8).collect::<String>(),
            confirmation.relay_count,
            confirmation.max_hops,
            self.pending.len() + 1
        );
        
        self.pending.push_back(confirmation);
        Ok(())
    }
    
    /// Pop next confirmation (FIFO)
    pub fn pop(&mut self) -> Option<Confirmation> {
        let confirmation = self.pending.pop_front();
        
        if let Some(ref conf) = confirmation {
            tracing::debug!(
                "Popped confirmation for tx {} (remaining: {})",
                conf.tx_id_hex().chars().take(8).collect::<String>(),
                self.pending.len()
            );
        }
        
        confirmation
    }
    
    /// Peek at next confirmation without removing it
    pub fn peek(&self) -> Option<&Confirmation> {
        self.pending.front()
    }
    
    /// Get queue length
    pub fn len(&self) -> usize {
        self.pending.len()
    }
    
    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    
    /// Clear all confirmations
    pub fn clear(&mut self) {
        self.pending.clear();
        tracing::info!("Cleared confirmation queue");
    }
    
    /// Cleanup expired confirmations (older than TTL)
    pub fn cleanup_expired(&mut self) -> usize {
        let original_len = self.pending.len();
        
        self.pending.retain(|conf| {
            let expired = conf.is_expired(self.default_ttl);
            if expired {
                tracing::info!(
                    "Removed expired confirmation for tx {} (age: {}s)",
                    conf.tx_id_hex().chars().take(8).collect::<String>(),
                    conf.age_seconds()
                );
            }
            !expired
        });
        
        original_len - self.pending.len()
    }
    
    /// Get statistics about queue contents
    pub fn stats(&self) -> ConfirmationStats {
        let mut success_count = 0;
        let mut failed_count = 0;
        let mut avg_age = 0u64;
        let mut max_hops = 0u8;
        
        for conf in &self.pending {
            match conf.status {
                ConfirmationStatus::Success { .. } => success_count += 1,
                ConfirmationStatus::Failed { .. } => failed_count += 1,
            }
            avg_age += conf.age_seconds();
            max_hops = max_hops.max(conf.relay_count);
        }
        
        if !self.pending.is_empty() {
            avg_age /= self.pending.len() as u64;
        }
        
        ConfirmationStats {
            total: self.pending.len(),
            success_count,
            failed_count,
            avg_age_seconds: avg_age,
            max_relay_hops: max_hops,
        }
    }
}

impl Default for ConfirmationQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Confirmation queue statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationStats {
    pub total: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub avg_age_seconds: u64,
    pub max_relay_hops: u8,
}

/// Confirmation queue errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfirmationError {
    #[error("Confirmation queue is full (max size: {0})")]
    QueueFull(usize),
    
    #[error("Confirmation for tx {tx_id} exceeded max hops ({hops}/{max_hops})")]
    MaxHopsExceeded {
        tx_id: String,
        hops: u8,
        max_hops: u8,
    },
    
    #[error("Confirmation expired (age: {age}s, TTL: {ttl}s)")]
    Expired {
        age: u64,
        ttl: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_confirmation() -> Confirmation {
        Confirmation::success([1u8; 32], "test_sig".to_string())
    }
    
    #[test]
    fn test_queue_creation() {
        let queue = ConfirmationQueue::new();
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }
    
    #[test]
    fn test_push_pop() {
        let mut queue = ConfirmationQueue::new();
        let conf = create_test_confirmation();
        
        assert!(queue.push(conf.clone()).is_ok());
        assert_eq!(queue.len(), 1);
        
        let popped = queue.pop().unwrap();
        assert_eq!(popped.original_tx_id, [1u8; 32]);
        assert!(queue.is_empty());
    }
    
    #[test]
    fn test_fifo_ordering() {
        let mut queue = ConfirmationQueue::new();
        
        let conf1 = Confirmation::success([1u8; 32], "sig1".to_string());
        let conf2 = Confirmation::success([2u8; 32], "sig2".to_string());
        let conf3 = Confirmation::success([3u8; 32], "sig3".to_string());
        
        queue.push(conf1).unwrap();
        queue.push(conf2).unwrap();
        queue.push(conf3).unwrap();
        
        // Should pop in FIFO order
        assert_eq!(queue.pop().unwrap().original_tx_id, [1u8; 32]);
        assert_eq!(queue.pop().unwrap().original_tx_id, [2u8; 32]);
        assert_eq!(queue.pop().unwrap().original_tx_id, [3u8; 32]);
    }
    
    #[test]
    fn test_hop_count() {
        let mut conf = create_test_confirmation();
        conf.max_hops = 3;
        
        assert!(!conf.has_exceeded_hops());
        assert_eq!(conf.relay_count, 0);
        
        assert!(conf.increment_relay());
        assert_eq!(conf.relay_count, 1);
        
        assert!(conf.increment_relay());
        assert!(conf.increment_relay());
        assert_eq!(conf.relay_count, 3);
        assert!(conf.has_exceeded_hops());
        
        assert!(!conf.increment_relay()); // Should fail
    }
    
    #[test]
    fn test_max_hops_error() {
        let mut queue = ConfirmationQueue::new();
        let mut conf = create_test_confirmation();
        conf.max_hops = 2;
        conf.relay_count = 2;
        
        let result = queue.push(conf);
        assert!(matches!(result, Err(ConfirmationError::MaxHopsExceeded { .. })));
    }
    
    #[test]
    fn test_queue_full_drops_oldest() {
        let mut queue = ConfirmationQueue::with_capacity(2);
        
        queue.push(Confirmation::success([1u8; 32], "sig1".to_string())).unwrap();
        queue.push(Confirmation::success([2u8; 32], "sig2".to_string())).unwrap();
        
        // Queue full, should drop oldest
        assert!(queue.push(Confirmation::success([3u8; 32], "sig3".to_string())).is_ok());
        assert_eq!(queue.len(), 2);
        
        // First should be 2 (1 was dropped)
        assert_eq!(queue.pop().unwrap().original_tx_id, [2u8; 32]);
    }
    
    #[test]
    fn test_peek() {
        let mut queue = ConfirmationQueue::new();
        let conf = create_test_confirmation();
        
        queue.push(conf.clone()).unwrap();
        
        // Peek should not remove
        assert_eq!(queue.peek().unwrap().original_tx_id, [1u8; 32]);
        assert_eq!(queue.len(), 1);
        
        // Pop should remove
        queue.pop();
        assert!(queue.peek().is_none());
    }
    
    #[test]
    fn test_clear() {
        let mut queue = ConfirmationQueue::new();
        queue.push(create_test_confirmation()).unwrap();
        queue.push(create_test_confirmation()).unwrap();
        
        assert_eq!(queue.len(), 2);
        queue.clear();
        assert_eq!(queue.len(), 0);
    }
    
    #[test]
    fn test_stats() {
        let mut queue = ConfirmationQueue::new();
        
        queue.push(Confirmation::success([1u8; 32], "sig1".to_string())).unwrap();
        queue.push(Confirmation::failure([2u8; 32], "error".to_string())).unwrap();
        
        let mut conf3 = Confirmation::success([3u8; 32], "sig3".to_string());
        conf3.relay_count = 2;
        queue.push(conf3).unwrap();
        
        let stats = queue.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.failed_count, 1);
        assert_eq!(stats.max_relay_hops, 2);
    }
    
    #[test]
    fn test_confirmation_age() {
        let mut conf = create_test_confirmation();
        assert_eq!(conf.age_seconds(), 0);
        
        // Manually set old timestamp
        conf.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 100;
        
        assert!(conf.age_seconds() >= 100);
        assert!(conf.is_expired(50));
        assert!(!conf.is_expired(200));
    }
    
    #[test]
    fn test_cleanup_expired() {
        let mut queue = ConfirmationQueue::with_ttl(100, 60); // 60 second TTL
        
        let mut old_conf = create_test_confirmation();
        old_conf.timestamp = 0; // Very old
        
        let new_conf = create_test_confirmation();
        
        queue.push(old_conf).unwrap();
        queue.push(new_conf).unwrap();
        
        assert_eq!(queue.len(), 2);
        
        let removed = queue.cleanup_expired();
        assert_eq!(removed, 1);
        assert_eq!(queue.len(), 1);
    }
    
    #[test]
    fn test_tx_id_hex() {
        let conf = Confirmation::success([0xAB, 0xCD, 0xEF], "sig".to_string());
        let hex = conf.tx_id_hex();
        assert!(hex.starts_with("abcdef"));
    }
}
