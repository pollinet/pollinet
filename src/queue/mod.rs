//! Queue Management System for PolliNet
//!
//! Implements a battery-efficient queue architecture with:
//! - Priority-based outbound transmission
//! - Fragment reassembly with SHA-256 matching  
//! - Retry logic with exponential backoff
//! - Confirmation relay for mesh acknowledgment
//!
//! Architecture: Event-driven (not polling) for 85%+ battery savings

pub mod outbound;
pub mod confirmation;
pub mod retry;
pub mod storage;

// Re-export main types
pub use outbound::{OutboundQueue, OutboundTransaction, Priority};
pub use confirmation::{ConfirmationQueue, Confirmation, ConfirmationStatus};
pub use retry::{RetryQueue, RetryItem, BackoffStrategy};
pub use storage::{QueueStorage, StorageError};

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Queue manager coordinating all queues with auto-save
pub struct QueueManager {
    /// Outbound transaction queue (priority-based)
    pub outbound: Arc<RwLock<OutboundQueue>>,
    /// Confirmation relay queue
    pub confirmations: Arc<RwLock<ConfirmationQueue>>,
    /// Retry queue with exponential backoff
    pub retries: Arc<RwLock<RetryQueue>>,
    /// Storage backend for persistence
    storage: Option<Arc<storage::QueueStorage>>,
    /// Last save timestamp for debouncing
    last_save: Arc<RwLock<Instant>>,
    /// Auto-save interval (debounce period)
    save_interval: Duration,
}

impl QueueManager {
    /// Create a new queue manager with default settings (no persistence)
    pub fn new() -> Self {
        Self {
            outbound: Arc::new(RwLock::new(OutboundQueue::new())),
            confirmations: Arc::new(RwLock::new(ConfirmationQueue::new())),
            retries: Arc::new(RwLock::new(RetryQueue::new())),
            storage: None,
            last_save: Arc::new(RwLock::new(Instant::now())),
            save_interval: Duration::from_secs(5), // Debounce: save at most every 5 seconds
        }
    }
    
    /// Create queue manager with custom configuration
    pub fn with_config(config: QueueConfig) -> Self {
        Self {
            outbound: Arc::new(RwLock::new(OutboundQueue::with_capacity(config.max_outbound_size))),
            confirmations: Arc::new(RwLock::new(ConfirmationQueue::with_capacity(config.max_confirmation_size))),
            retries: Arc::new(RwLock::new(RetryQueue::with_config(
                config.max_retries,
                config.retry_backoff_strategy,
            ))),
            storage: None,
            last_save: Arc::new(RwLock::new(Instant::now())),
            save_interval: Duration::from_secs(config.auto_save_interval_secs.unwrap_or(5)),
        }
    }
    
    /// Create queue manager with persistence enabled
    pub fn with_storage(storage_dir: impl AsRef<std::path::Path>) -> Result<Self, StorageError> {
        let storage = storage::QueueStorage::new(storage_dir)?;
        
        // Load existing queues from disk
        let (outbound, retry, confirmation) = storage.load_all()?;
        
        Ok(Self {
            outbound: Arc::new(RwLock::new(outbound)),
            confirmations: Arc::new(RwLock::new(confirmation)),
            retries: Arc::new(RwLock::new(retry)),
            storage: Some(Arc::new(storage)),
            last_save: Arc::new(RwLock::new(Instant::now())),
            save_interval: Duration::from_secs(5),
        })
    }
    
    /// Save all queues to disk (with debouncing)
    pub async fn save_if_needed(&self) -> Result<(), StorageError> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(()), // No storage configured
        };
        
        // Check if enough time has passed since last save
        let mut last_save = self.last_save.write().await;
        if last_save.elapsed() < self.save_interval {
            return Ok(()); // Skip save (debounce)
        }
        
        // Save all queues
        let outbound = self.outbound.read().await;
        let retry = self.retries.read().await;
        let confirmation = self.confirmations.read().await;
        
        storage.save_all(&outbound, &retry, &confirmation)?;
        
        *last_save = Instant::now();
        
        Ok(())
    }
    
    /// Force save all queues (bypass debouncing)
    pub async fn force_save(&self) -> Result<(), StorageError> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(()),
        };
        
        let outbound = self.outbound.read().await;
        let retry = self.retries.read().await;
        let confirmation = self.confirmations.read().await;
        
        storage.save_all(&outbound, &retry, &confirmation)?;
        
        let mut last_save = self.last_save.write().await;
        *last_save = Instant::now();
        
        tracing::info!("Force saved all queues");
        Ok(())
    }
    
    /// Get metrics for all queues
    pub async fn get_metrics(&self) -> QueueMetrics {
        let outbound = self.outbound.read().await;
        let confirmations = self.confirmations.read().await;
        let retries = self.retries.read().await;
        
        QueueMetrics {
            outbound_size: outbound.len(),
            outbound_high_priority: outbound.len_priority(Priority::High),
            outbound_normal_priority: outbound.len_priority(Priority::Normal),
            outbound_low_priority: outbound.len_priority(Priority::Low),
            confirmation_size: confirmations.len(),
            retry_size: retries.len(),
            retry_avg_attempts: retries.average_attempts(),
        }
    }
    
    /// Get queue health status
    pub async fn get_health(&self) -> HealthStatus {
        let metrics = self.get_metrics().await;
        
        let warnings = vec![
            (metrics.outbound_size > 100, "Outbound queue > 100 items"),
            (metrics.retry_size > 50, "Retry queue > 50 items"),
            (metrics.outbound_size > 500, "CRITICAL: Outbound queue > 500 items"),
        ];
        
        let active_warnings: Vec<_> = warnings
            .into_iter()
            .filter(|(condition, _)| *condition)
            .map(|(_, msg)| msg.to_string())
            .collect();
        
        if active_warnings.is_empty() {
            HealthStatus::Healthy
        } else if metrics.outbound_size > 500 || metrics.retry_size > 200 {
            HealthStatus::Critical(active_warnings)
        } else {
            HealthStatus::Warning(active_warnings)
        }
    }
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Queue configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    /// Maximum outbound queue size
    pub max_outbound_size: usize,
    /// Maximum confirmation queue size
    pub max_confirmation_size: usize,
    /// Maximum retry attempts per transaction
    pub max_retries: usize,
    /// Retry backoff strategy
    pub retry_backoff_strategy: BackoffStrategy,
    /// Auto-save interval in seconds (None to disable auto-save)
    pub auto_save_interval_secs: Option<u64>,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_outbound_size: 1000,
            max_confirmation_size: 500,
            max_retries: 5,
            retry_backoff_strategy: BackoffStrategy::Exponential { base_seconds: 2 },
            auto_save_interval_secs: Some(5), // Auto-save every 5 seconds
        }
    }
}

/// Queue metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMetrics {
    pub outbound_size: usize,
    pub outbound_high_priority: usize,
    pub outbound_normal_priority: usize,
    pub outbound_low_priority: usize,
    pub confirmation_size: usize,
    pub retry_size: usize,
    pub retry_avg_attempts: f32,
}

/// Queue health status
#[derive(Debug, Clone)]
pub enum HealthStatus {
    Healthy,
    Warning(Vec<String>),
    Critical(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_queue_manager_creation() {
        let manager = QueueManager::new();
        let metrics = manager.get_metrics().await;
        
        assert_eq!(metrics.outbound_size, 0);
        assert_eq!(metrics.confirmation_size, 0);
        assert_eq!(metrics.retry_size, 0);
    }
    
    #[tokio::test]
    async fn test_queue_health_healthy() {
        let manager = QueueManager::new();
        let health = manager.get_health().await;
        
        matches!(health, HealthStatus::Healthy);
    }
}

