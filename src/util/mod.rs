//! Utility functions for PolliNet SDK
//!
//! Includes compression, serialization, and other helper functions

pub mod lz;

/// Common utility functions
pub mod common {
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Generate a unique identifier based on current timestamp
    pub fn generate_id() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("id_{:x}", timestamp)
    }

    /// Check if data should be compressed based on size threshold
    pub fn should_compress(data_size: usize, threshold: usize) -> bool {
        data_size > threshold
    }

    /// Calculate optimal fragment size for BLE transmission
    pub fn calculate_fragment_size(data_size: usize, mtu_size: usize) -> usize {
        (data_size + mtu_size - 1) / mtu_size
    }
}
