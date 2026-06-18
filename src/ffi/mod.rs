//! FFI module for Android integration
//!
//! This module provides a C-compatible interface for Kotlin/Java to interact with
//! the PolliNet Rust core. It handles:
//! - Host-driven BLE transport (push_inbound, next_outbound, tick)
//! - Transaction building and fragmentation
//! - Signature operations
//! - Metrics and diagnostics

pub mod android;
pub mod host_transport;
pub mod runtime;
pub mod transport;
pub mod types;
pub mod wifi_direct_transport;

pub use android::*;
pub use host_transport::HostTransport;
pub use types::*;
pub use wifi_direct_transport::HostWifiDirectTransport;
