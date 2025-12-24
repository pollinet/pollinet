//! Outbound Transaction Queue
//!
//! Priority-based queue for transactions awaiting BLE transmission.
//! Supports HIGH, NORMAL, and LOW priority with deduplication.

use crate::ble::mesh::TransactionFragment;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Transaction priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// High priority - user-initiated transactions (sent first)
    High = 2,
    /// Normal priority - regular transactions (default)
    Normal = 1,
    /// Low priority - relay transactions (sent last)
    Low = 0,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

/// Outbound transaction awaiting BLE transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundTransaction {
    /// Unique transaction ID (SHA-256 hash as hex string)
    pub tx_id: String,
    /// Original signed transaction bytes
    pub original_bytes: Vec<u8>,
    /// Pre-fragmented transaction (based on current MTU)
    pub fragments: Vec<TransactionFragment>,
    /// Transaction priority
    pub priority: Priority,
    /// Unix timestamp when queued
    pub created_at: u64,
    /// Current retry count (for transmission failures)
    pub retry_count: u8,
    /// Maximum retries before giving up
    pub max_retries: u8,
}

impl OutboundTransaction {
    /// Create new outbound transaction
    pub fn new(
        tx_id: String,
        original_bytes: Vec<u8>,
        fragments: Vec<TransactionFragment>,
        priority: Priority,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            tx_id,
            original_bytes,
            fragments,
            priority,
            created_at: now,
            retry_count: 0,
            max_retries: 3,
        }
    }
    
    /// Check if transaction has exceeded max retries
    pub fn has_exceeded_retries(&self) -> bool {
        self.retry_count >= self.max_retries
    }
    
    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
    
    /// Get age in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.created_at)
    }
}

/// Priority-based outbound queue with deduplication
pub struct OutboundQueue {
    /// High priority queue (user-initiated)
    high_priority: VecDeque<OutboundTransaction>,
    /// Normal priority queue (default)
    normal_priority: VecDeque<OutboundTransaction>,
    /// Low priority queue (relayed transactions)
    low_priority: VecDeque<OutboundTransaction>,
    /// Set of transaction IDs for deduplication (O(1) lookup)
    deduplication_set: HashSet<String>,
    /// Maximum queue size (across all priorities)
    max_size: usize,
}

impl OutboundQueue {
    /// Create new outbound queue with default capacity (1000)
    pub fn new() -> Self {
        Self::with_capacity(1000)
    }
    
    /// Create new outbound queue with specified capacity
    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            high_priority: VecDeque::new(),
            normal_priority: VecDeque::new(),
            low_priority: VecDeque::new(),
            deduplication_set: HashSet::new(),
            max_size,
        }
    }
    
    /// Push transaction to queue (returns error if duplicate or queue full)
    pub fn push(&mut self, tx: OutboundTransaction) -> Result<(), QueueError> {
        // Check for duplicates
        if self.deduplication_set.contains(&tx.tx_id) {
            return Err(QueueError::Duplicate(tx.tx_id));
        }
        
        // Check queue size
        if self.len() >= self.max_size {
            // Try to make room by dropping oldest low priority transaction
            if !self.low_priority.is_empty() {
                if let Some(dropped) = self.low_priority.pop_front() {
                    self.deduplication_set.remove(&dropped.tx_id);
                    tracing::warn!(
                        "Queue full ({}), dropped low priority tx: {}",
                        self.max_size,
                        dropped.tx_id
                    );
                }
            } else {
                return Err(QueueError::QueueFull(self.max_size));
            }
        }
        
        // Add to deduplication set
        self.deduplication_set.insert(tx.tx_id.clone());
        
        // Add to appropriate priority queue
        match tx.priority {
            Priority::High => self.high_priority.push_back(tx),
            Priority::Normal => self.normal_priority.push_back(tx),
            Priority::Low => self.low_priority.push_back(tx),
        }
        
        tracing::debug!(
            "Queued transaction {} with priority {:?} (queue size: {})",
            tx.tx_id,
            tx.priority,
            self.len()
        );
        
        Ok(())
    }
    
    /// Pop next transaction (priority-based: HIGH → NORMAL → LOW)
    pub fn pop(&mut self) -> Option<OutboundTransaction> {
        let tx = if let Some(tx) = self.high_priority.pop_front() {
            Some(tx)
        } else if let Some(tx) = self.normal_priority.pop_front() {
            Some(tx)
        } else {
            self.low_priority.pop_front()
        };
        
        // Remove from deduplication set
        if let Some(ref tx) = tx {
            self.deduplication_set.remove(&tx.tx_id);
            tracing::debug!(
                "Popped transaction {} (remaining: {})",
                tx.tx_id,
                self.len()
            );
        }
        
        tx
    }
    
    /// Check if transaction exists in queue
    pub fn contains(&self, tx_id: &str) -> bool {
        self.deduplication_set.contains(tx_id)
    }
    
    /// Get total queue length (all priorities)
    pub fn len(&self) -> usize {
        self.high_priority.len() + self.normal_priority.len() + self.low_priority.len()
    }
    
    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Get length of specific priority queue
    pub fn len_priority(&self, priority: Priority) -> usize {
        match priority {
            Priority::High => self.high_priority.len(),
            Priority::Normal => self.normal_priority.len(),
            Priority::Low => self.low_priority.len(),
        }
    }
    
    /// Peek at next transaction without removing it
    pub fn peek(&self) -> Option<&OutboundTransaction> {
        self.high_priority.front()
            .or_else(|| self.normal_priority.front())
            .or_else(|| self.low_priority.front())
    }
    
    /// Clear all queues
    pub fn clear(&mut self) {
        self.high_priority.clear();
        self.normal_priority.clear();
        self.low_priority.clear();
        self.deduplication_set.clear();
        tracing::info!("Cleared outbound queue");
    }
    
    /// Remove stale transactions (older than max_age_seconds)
    pub fn cleanup_stale(&mut self, max_age_seconds: u64) -> usize {
        let mut removed_count = 0;
        
        // Helper function to filter and count removed items
        let filter_stale = |queue: &mut VecDeque<OutboundTransaction>| -> usize {
            let original_len = queue.len();
            queue.retain(|tx| tx.age_seconds() < max_age_seconds);
            original_len - queue.len()
        };
        
        removed_count += filter_stale(&mut self.high_priority);
        removed_count += filter_stale(&mut self.normal_priority);
        removed_count += filter_stale(&mut self.low_priority);
        
        // Rebuild deduplication set from remaining transactions
        self.deduplication_set.clear();
        for tx in self.high_priority.iter()
            .chain(self.normal_priority.iter())
            .chain(self.low_priority.iter())
        {
            self.deduplication_set.insert(tx.tx_id.clone());
        }
        
        if removed_count > 0 {
            tracing::info!(
                "Cleaned up {} stale transactions (older than {}s)",
                removed_count,
                max_age_seconds
            );
        }
        
        removed_count
    }
    
    /// Get statistics about queue contents
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            total: self.len(),
            high_priority: self.high_priority.len(),
            normal_priority: self.normal_priority.len(),
            low_priority: self.low_priority.len(),
            oldest_age_seconds: self.get_oldest_age_seconds(),
        }
    }
    
    /// Get age of oldest transaction in seconds
    fn get_oldest_age_seconds(&self) -> Option<u64> {
        let oldest = self.high_priority.front()
            .into_iter()
            .chain(self.normal_priority.front())
            .chain(self.low_priority.front())
            .min_by_key(|tx| tx.created_at);
        
        oldest.map(|tx| tx.age_seconds())
    }
}

impl Default for OutboundQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Queue statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub total: usize,
    pub high_priority: usize,
    pub normal_priority: usize,
    pub low_priority: usize,
    pub oldest_age_seconds: Option<u64>,
}

/// Queue operation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum QueueError {
    #[error("Transaction {0} already in queue (duplicate)")]
    Duplicate(String),
    
    #[error("Queue is full (max size: {0})")]
    QueueFull(usize),
    
    #[error("Transaction not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_tx(id: &str, priority: Priority) -> OutboundTransaction {
        OutboundTransaction::new(
            id.to_string(),
            vec![1, 2, 3],
            vec![],
            priority,
        )
    }
    
    #[test]
    fn test_queue_creation() {
        let queue = OutboundQueue::new();
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }
    
    #[test]
    fn test_push_pop_single() {
        let mut queue = OutboundQueue::new();
        let tx = create_test_tx("tx1", Priority::Normal);
        
        assert!(queue.push(tx.clone()).is_ok());
        assert_eq!(queue.len(), 1);
        
        let popped = queue.pop().unwrap();
        assert_eq!(popped.tx_id, "tx1");
        assert!(queue.is_empty());
    }
    
    #[test]
    fn test_priority_ordering() {
        let mut queue = OutboundQueue::new();
        
        // Add in reverse priority order
        queue.push(create_test_tx("low", Priority::Low)).unwrap();
        queue.push(create_test_tx("normal", Priority::Normal)).unwrap();
        queue.push(create_test_tx("high", Priority::High)).unwrap();
        
        // Should pop in priority order: high, normal, low
        assert_eq!(queue.pop().unwrap().tx_id, "high");
        assert_eq!(queue.pop().unwrap().tx_id, "normal");
        assert_eq!(queue.pop().unwrap().tx_id, "low");
    }
    
    #[test]
    fn test_deduplication() {
        let mut queue = OutboundQueue::new();
        let tx = create_test_tx("tx1", Priority::Normal);
        
        assert!(queue.push(tx.clone()).is_ok());
        assert!(matches!(queue.push(tx), Err(QueueError::Duplicate(_))));
        assert_eq!(queue.len(), 1);
    }
    
    #[test]
    fn test_contains() {
        let mut queue = OutboundQueue::new();
        queue.push(create_test_tx("tx1", Priority::Normal)).unwrap();
        
        assert!(queue.contains("tx1"));
        assert!(!queue.contains("tx2"));
    }
    
    #[test]
    fn test_queue_full_drops_low_priority() {
        let mut queue = OutboundQueue::with_capacity(2);
        
        queue.push(create_test_tx("low1", Priority::Low)).unwrap();
        queue.push(create_test_tx("high1", Priority::High)).unwrap();
        
        // Queue is full (2/2), adding another should drop low priority
        assert!(queue.push(create_test_tx("high2", Priority::High)).is_ok());
        
        // low1 should be dropped
        assert!(!queue.contains("low1"));
        assert!(queue.contains("high1"));
        assert!(queue.contains("high2"));
    }
    
    #[test]
    fn test_queue_full_error_when_no_low_priority() {
        let mut queue = OutboundQueue::with_capacity(2);
        
        queue.push(create_test_tx("high1", Priority::High)).unwrap();
        queue.push(create_test_tx("high2", Priority::High)).unwrap();
        
        // Queue is full with high priority, should error
        assert!(matches!(
            queue.push(create_test_tx("high3", Priority::High)),
            Err(QueueError::QueueFull(2))
        ));
    }
    
    #[test]
    fn test_peek() {
        let mut queue = OutboundQueue::new();
        queue.push(create_test_tx("tx1", Priority::Normal)).unwrap();
        
        // Peek should not remove
        assert_eq!(queue.peek().unwrap().tx_id, "tx1");
        assert_eq!(queue.len(), 1);
        
        // Pop should remove
        queue.pop();
        assert!(queue.peek().is_none());
    }
    
    #[test]
    fn test_clear() {
        let mut queue = OutboundQueue::new();
        queue.push(create_test_tx("tx1", Priority::High)).unwrap();
        queue.push(create_test_tx("tx2", Priority::Normal)).unwrap();
        queue.push(create_test_tx("tx3", Priority::Low)).unwrap();
        
        assert_eq!(queue.len(), 3);
        queue.clear();
        assert_eq!(queue.len(), 0);
        assert!(!queue.contains("tx1"));
    }
    
    #[test]
    fn test_cleanup_stale() {
        let mut queue = OutboundQueue::new();
        
        // Add transactions with different ages (manually set created_at)
        let mut old_tx = create_test_tx("old", Priority::Normal);
        old_tx.created_at = 0; // Very old
        
        let new_tx = create_test_tx("new", Priority::Normal);
        
        queue.push(old_tx).unwrap();
        queue.push(new_tx).unwrap();
        
        // Cleanup transactions older than 60 seconds
        let removed = queue.cleanup_stale(60);
        
        assert_eq!(removed, 1);
        assert!(!queue.contains("old"));
        assert!(queue.contains("new"));
    }
    
    #[test]
    fn test_stats() {
        let mut queue = OutboundQueue::new();
        queue.push(create_test_tx("h1", Priority::High)).unwrap();
        queue.push(create_test_tx("h2", Priority::High)).unwrap();
        queue.push(create_test_tx("n1", Priority::Normal)).unwrap();
        queue.push(create_test_tx("l1", Priority::Low)).unwrap();
        
        let stats = queue.stats();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.high_priority, 2);
        assert_eq!(stats.normal_priority, 1);
        assert_eq!(stats.low_priority, 1);
        assert!(stats.oldest_age_seconds.is_some());
    }
    
    #[test]
    fn test_len_priority() {
        let mut queue = OutboundQueue::new();
        queue.push(create_test_tx("h1", Priority::High)).unwrap();
        queue.push(create_test_tx("n1", Priority::Normal)).unwrap();
        queue.push(create_test_tx("n2", Priority::Normal)).unwrap();
        
        assert_eq!(queue.len_priority(Priority::High), 1);
        assert_eq!(queue.len_priority(Priority::Normal), 2);
        assert_eq!(queue.len_priority(Priority::Low), 0);
    }
    
    #[test]
    fn test_transaction_age() {
        let tx = create_test_tx("tx1", Priority::Normal);
        assert_eq!(tx.age_seconds(), 0); // Just created
        
        let mut old_tx = create_test_tx("tx2", Priority::Normal);
        old_tx.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 100;
        assert!(old_tx.age_seconds() >= 100);
    }
    
    #[test]
    fn test_retry_count() {
        let mut tx = create_test_tx("tx1", Priority::Normal);
        tx.max_retries = 3;
        
        assert!(!tx.has_exceeded_retries());
        
        tx.increment_retry();
        assert_eq!(tx.retry_count, 1);
        assert!(!tx.has_exceeded_retries());
        
        tx.increment_retry();
        tx.increment_retry();
        assert_eq!(tx.retry_count, 3);
        assert!(tx.has_exceeded_retries());
    }
}

