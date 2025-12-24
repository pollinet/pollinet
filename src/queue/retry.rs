//! Retry Queue with Exponential Backoff
//!
//! Queue for failed transaction submissions with intelligent retry logic.
//! Uses BTreeMap for efficient time-based scheduling of retries.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Backoff strategy for retries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackoffStrategy {
    /// Exponential backoff: delay = base_seconds * 2^attempt
    Exponential { base_seconds: u64 },
    /// Linear backoff: delay = increment_seconds * attempt
    Linear { increment_seconds: u64 },
    /// Fixed interval between retries
    Fixed { interval_seconds: u64 },
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        BackoffStrategy::Exponential { base_seconds: 2 }
    }
}

impl BackoffStrategy {
    /// Calculate next retry delay based on attempt count
    pub fn calculate_delay(&self, attempt_count: usize) -> Duration {
        let seconds = match self {
            BackoffStrategy::Exponential { base_seconds } => {
                // Exponential: 2s, 4s, 8s, 16s, 32s, 64s (caps at 64s)
                let exp = (attempt_count as u32).min(6);
                base_seconds * 2u64.pow(exp)
            }
            BackoffStrategy::Linear { increment_seconds } => {
                // Linear: increment * (attempt + 1)
                increment_seconds * (attempt_count as u64 + 1)
            }
            BackoffStrategy::Fixed { interval_seconds } => {
                // Fixed interval regardless of attempts
                *interval_seconds
            }
        };
        
        Duration::from_secs(seconds)
    }
}

/// Retry item for failed transactions
#[derive(Debug, Clone)]
pub struct RetryItem {
    /// Transaction bytes (signed transaction)
    pub tx_bytes: Vec<u8>,
    /// Transaction ID (SHA-256 hash as hex string)
    pub tx_id: String,
    /// Number of retry attempts made
    pub attempt_count: usize,
    /// Error message from last failure
    pub last_error: String,
    /// Next scheduled retry time
    pub next_retry_time: Instant,
    /// When this item was first created
    pub created_at: Instant,
    /// Unix timestamp for serialization
    pub created_at_unix: u64,
}

impl RetryItem {
    /// Create new retry item
    pub fn new(tx_bytes: Vec<u8>, tx_id: String, error: String) -> Self {
        let now = Instant::now();
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            tx_bytes,
            tx_id,
            attempt_count: 0,
            last_error: error,
            next_retry_time: now, // Retry immediately on first attempt
            created_at: now,
            created_at_unix: now_unix,
        }
    }
    
    /// Update for next retry attempt
    pub fn prepare_next_retry(&mut self, backoff_strategy: &BackoffStrategy) {
        self.attempt_count += 1;
        let delay = backoff_strategy.calculate_delay(self.attempt_count);
        self.next_retry_time = Instant::now() + delay;
        
        tracing::debug!(
            "Scheduled retry for tx {} (attempt {}, delay: {}s)",
            self.tx_id.chars().take(8).collect::<String>(),
            self.attempt_count,
            delay.as_secs()
        );
    }
    
    /// Check if retry is ready (past next_retry_time)
    pub fn is_ready(&self) -> bool {
        Instant::now() >= self.next_retry_time
    }
    
    /// Get age since creation
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
    
    /// Get time until next retry
    pub fn time_until_retry(&self) -> Duration {
        self.next_retry_time.saturating_duration_since(Instant::now())
    }
}

/// Retry queue with time-based scheduling
pub struct RetryQueue {
    /// Items indexed by next retry time for efficient polling
    items: BTreeMap<Instant, RetryItem>,
    /// Maximum number of retry attempts per transaction
    max_retries: usize,
    /// Maximum age for retry items (24 hours default)
    max_age: Duration,
    /// Backoff strategy for calculating retry delays
    backoff_strategy: BackoffStrategy,
}

impl RetryQueue {
    /// Create new retry queue with default settings
    pub fn new() -> Self {
        Self::with_config(5, BackoffStrategy::default())
    }
    
    /// Create retry queue with custom configuration
    pub fn with_config(max_retries: usize, backoff_strategy: BackoffStrategy) -> Self {
        Self {
            items: BTreeMap::new(),
            max_retries,
            max_age: Duration::from_secs(24 * 3600), // 24 hours
            backoff_strategy,
        }
    }
    
    /// Push item to retry queue
    pub fn push(&mut self, mut item: RetryItem) -> Result<(), RetryError> {
        // Check if should give up
        if self.should_give_up(&item) {
            return Err(RetryError::MaxRetriesExceeded {
                tx_id: item.tx_id.clone(),
                attempts: item.attempt_count,
                max_retries: self.max_retries,
            });
        }
        
        // Calculate next retry time if not first attempt
        if item.attempt_count > 0 {
            item.prepare_next_retry(&self.backoff_strategy);
        }
        
        // Handle collision: if exact Instant already exists, add 1ns
        let mut retry_time = item.next_retry_time;
        while self.items.contains_key(&retry_time) {
            retry_time += Duration::from_nanos(1);
        }
        item.next_retry_time = retry_time;
        
        tracing::info!(
            "Added retry for tx {} (attempt {}/{}, next retry in {}s)",
            item.tx_id.chars().take(8).collect::<String>(),
            item.attempt_count,
            self.max_retries,
            item.time_until_retry().as_secs()
        );
        
        self.items.insert(retry_time, item);
        Ok(())
    }
    
    /// Pop next ready retry item
    pub fn pop_ready(&mut self) -> Option<RetryItem> {
        let now = Instant::now();
        
        // Find first item where retry time <= now
        let ready_key = self.items
            .keys()
            .find(|&&time| time <= now)
            .copied();
        
        if let Some(key) = ready_key {
            let item = self.items.remove(&key);
            
            if let Some(ref i) = item {
                tracing::debug!(
                    "Popped retry item for tx {} (attempt {}, age: {}s)",
                    i.tx_id.chars().take(8).collect::<String>(),
                    i.attempt_count,
                    i.age().as_secs()
                );
            }
            
            return item;
        }
        
        None
    }
    
    /// Peek at next item without removing (regardless of ready status)
    pub fn peek_next(&self) -> Option<&RetryItem> {
        self.items.values().next()
    }
    
    /// Get next retry time (when next item will be ready)
    pub fn next_retry_time(&self) -> Option<Instant> {
        self.items.keys().next().copied()
    }
    
    /// Get number of items in retry queue
    pub fn len(&self) -> usize {
        self.items.len()
    }
    
    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    
    /// Check if transaction should give up retrying
    pub fn should_give_up(&self, item: &RetryItem) -> bool {
        // Give up if exceeded max retries OR max age
        item.attempt_count >= self.max_retries || item.age() > self.max_age
    }
    
    /// Clear all retry items
    pub fn clear(&mut self) {
        self.items.clear();
        tracing::info!("Cleared retry queue");
    }
    
    /// Cleanup expired items (older than max_age)
    pub fn cleanup_expired(&mut self) -> usize {
        let expired_keys: Vec<Instant> = self.items
            .iter()
            .filter(|(_, item)| item.age() > self.max_age)
            .map(|(k, _)| *k)
            .collect();
        
        let count = expired_keys.len();
        
        for key in expired_keys {
            if let Some(item) = self.items.remove(&key) {
                tracing::info!(
                    "Removed expired retry for tx {} (age: {}h)",
                    item.tx_id.chars().take(8).collect::<String>(),
                    item.age().as_secs() / 3600
                );
            }
        }
        
        count
    }
    
    /// Get average number of attempts across all items
    pub fn average_attempts(&self) -> f32 {
        if self.items.is_empty() {
            return 0.0;
        }
        
        let total: usize = self.items.values().map(|item| item.attempt_count).sum();
        total as f32 / self.items.len() as f32
    }
    
    /// Get statistics about retry queue
    pub fn stats(&self) -> RetryStats {
        if self.items.is_empty() {
            return RetryStats {
                total: 0,
                ready_now: 0,
                avg_attempts: 0.0,
                oldest_age_secs: 0,
                next_retry_in_secs: None,
            };
        }
        
        let now = Instant::now();
        let ready_count = self.items.keys().filter(|&&time| time <= now).count();
        
        let oldest_age = self.items
            .values()
            .map(|item| item.age().as_secs())
            .max()
            .unwrap_or(0);
        
        let next_retry = self.next_retry_time()
            .map(|time| time.saturating_duration_since(now).as_secs());
        
        RetryStats {
            total: self.items.len(),
            ready_now: ready_count,
            avg_attempts: self.average_attempts(),
            oldest_age_secs: oldest_age,
            next_retry_in_secs: next_retry,
        }
    }
}

impl Default for RetryQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Retry queue statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStats {
    /// Total items in queue
    pub total: usize,
    /// Items ready to retry now
    pub ready_now: usize,
    /// Average retry attempts
    pub avg_attempts: f32,
    /// Age of oldest item in seconds
    pub oldest_age_secs: u64,
    /// Seconds until next retry (None if empty)
    pub next_retry_in_secs: Option<u64>,
}

/// Retry queue errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum RetryError {
    #[error("Transaction {tx_id} exceeded max retries ({attempts}/{max_retries})")]
    MaxRetriesExceeded {
        tx_id: String,
        attempts: usize,
        max_retries: usize,
    },
    
    #[error("Transaction {tx_id} exceeded max age ({age_hours}h)")]
    MaxAgeExceeded {
        tx_id: String,
        age_hours: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    
    fn create_test_item(id: &str) -> RetryItem {
        RetryItem::new(
            vec![1, 2, 3],
            id.to_string(),
            "test error".to_string(),
        )
    }
    
    #[test]
    fn test_backoff_exponential() {
        let strategy = BackoffStrategy::Exponential { base_seconds: 2 };
        
        assert_eq!(strategy.calculate_delay(0).as_secs(), 2);  // 2^0 * 2 = 2
        assert_eq!(strategy.calculate_delay(1).as_secs(), 4);  // 2^1 * 2 = 4
        assert_eq!(strategy.calculate_delay(2).as_secs(), 8);  // 2^2 * 2 = 8
        assert_eq!(strategy.calculate_delay(3).as_secs(), 16); // 2^3 * 2 = 16
        assert_eq!(strategy.calculate_delay(6).as_secs(), 128); // 2^6 * 2 = 128 (capped)
    }
    
    #[test]
    fn test_backoff_linear() {
        let strategy = BackoffStrategy::Linear { increment_seconds: 5 };
        
        assert_eq!(strategy.calculate_delay(0).as_secs(), 5);  // 5 * 1
        assert_eq!(strategy.calculate_delay(1).as_secs(), 10); // 5 * 2
        assert_eq!(strategy.calculate_delay(2).as_secs(), 15); // 5 * 3
    }
    
    #[test]
    fn test_backoff_fixed() {
        let strategy = BackoffStrategy::Fixed { interval_seconds: 10 };
        
        assert_eq!(strategy.calculate_delay(0).as_secs(), 10);
        assert_eq!(strategy.calculate_delay(5).as_secs(), 10);
        assert_eq!(strategy.calculate_delay(100).as_secs(), 10);
    }
    
    #[test]
    fn test_queue_creation() {
        let queue = RetryQueue::new();
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }
    
    #[test]
    fn test_push_pop() {
        let mut queue = RetryQueue::new();
        let item = create_test_item("tx1");
        
        assert!(queue.push(item).is_ok());
        assert_eq!(queue.len(), 1);
        
        // Item should be ready immediately (first attempt)
        let popped = queue.pop_ready().unwrap();
        assert_eq!(popped.tx_id, "tx1");
        assert!(queue.is_empty());
    }
    
    #[test]
    fn test_retry_scheduling() {
        let mut queue = RetryQueue::with_config(
            3,
            BackoffStrategy::Fixed { interval_seconds: 1 }
        );
        
        let mut item = create_test_item("tx1");
        item.attempt_count = 1; // Not first attempt
        
        queue.push(item).unwrap();
        
        // Should not be ready immediately
        assert!(queue.pop_ready().is_none());
        
        // Wait for retry time
        thread::sleep(Duration::from_secs(2));
        
        // Should be ready now
        assert!(queue.pop_ready().is_some());
    }
    
    #[test]
    fn test_max_retries() {
        let mut queue = RetryQueue::with_config(3, BackoffStrategy::default());
        let mut item = create_test_item("tx1");
        
        // Set attempts to max
        item.attempt_count = 3;
        
        let result = queue.push(item);
        assert!(matches!(result, Err(RetryError::MaxRetriesExceeded { .. })));
    }
    
    #[test]
    fn test_average_attempts() {
        let mut queue = RetryQueue::new();
        
        let mut item1 = create_test_item("tx1");
        item1.attempt_count = 1;
        
        let mut item2 = create_test_item("tx2");
        item2.attempt_count = 3;
        
        queue.push(item1).unwrap();
        queue.push(item2).unwrap();
        
        assert_eq!(queue.average_attempts(), 2.0); // (1 + 3) / 2
    }
    
    #[test]
    fn test_time_ordering() {
        let mut queue = RetryQueue::with_config(
            5,
            BackoffStrategy::Fixed { interval_seconds: 1 }
        );
        
        // Add items with different attempt counts (different retry times)
        let mut item1 = create_test_item("tx1");
        item1.attempt_count = 2;
        
        let mut item2 = create_test_item("tx2");
        item2.attempt_count = 1;
        
        queue.push(item1).unwrap();
        queue.push(item2).unwrap();
        
        // Both should have scheduled retry times
        assert!(queue.next_retry_time().is_some());
    }
    
    #[test]
    fn test_peek_next() {
        let mut queue = RetryQueue::new();
        let item = create_test_item("tx1");
        
        queue.push(item).unwrap();
        
        // Peek should not remove
        assert_eq!(queue.peek_next().unwrap().tx_id, "tx1");
        assert_eq!(queue.len(), 1);
    }
    
    #[test]
    fn test_clear() {
        let mut queue = RetryQueue::new();
        queue.push(create_test_item("tx1")).unwrap();
        queue.push(create_test_item("tx2")).unwrap();
        
        assert_eq!(queue.len(), 2);
        queue.clear();
        assert_eq!(queue.len(), 0);
    }
    
    #[test]
    fn test_stats() {
        let mut queue = RetryQueue::new();
        
        let item1 = create_test_item("tx1");
        let mut item2 = create_test_item("tx2");
        item2.attempt_count = 2;
        
        queue.push(item1).unwrap();
        queue.push(item2).unwrap();
        
        let stats = queue.stats();
        assert_eq!(stats.total, 2);
        assert!(stats.avg_attempts > 0.0);
    }
    
    #[test]
    fn test_cleanup_expired() {
        let mut queue = RetryQueue::new();
        queue.max_age = Duration::from_secs(10); // 10 second max age
        
        let mut old_item = create_test_item("old");
        old_item.created_at = Instant::now() - Duration::from_secs(20);
        
        let new_item = create_test_item("new");
        
        queue.push(old_item).unwrap();
        queue.push(new_item).unwrap();
        
        assert_eq!(queue.len(), 2);
        
        let removed = queue.cleanup_expired();
        assert_eq!(removed, 1);
        assert_eq!(queue.len(), 1);
    }
    
    #[test]
    fn test_retry_item_is_ready() {
        let item = create_test_item("tx1");
        assert!(item.is_ready()); // Ready immediately on first attempt
        
        let mut future_item = create_test_item("tx2");
        future_item.next_retry_time = Instant::now() + Duration::from_secs(100);
        assert!(!future_item.is_ready());
    }
    
    #[test]
    fn test_prepare_next_retry() {
        let strategy = BackoffStrategy::Exponential { base_seconds: 2 };
        let mut item = create_test_item("tx1");
        
        assert_eq!(item.attempt_count, 0);
        
        item.prepare_next_retry(&strategy);
        assert_eq!(item.attempt_count, 1);
        assert!(!item.is_ready()); // Should schedule future retry
    }
}
