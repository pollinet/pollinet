//! FFI module for Android integration
//!
//! This module provides a C-compatible interface for Kotlin/Java to interact with
//! the PolliNet Rust core. It handles:
//! - Host-driven BLE transport (push_inbound, next_outbound, tick)
//! - Transaction building and fragmentation
//! - Signature operations
//! - Metrics and diagnostics

pub mod android;
pub mod runtime;
pub mod transport;
pub mod types;

pub use android::*;
pub use types::*;
