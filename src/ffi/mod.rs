//! FFI module for platform integration
//! 
//! This module provides C-compatible interfaces for platform-specific code to interact with
//! the PolliNet Rust core. It handles:
//! - Host-driven BLE transport (push_inbound, next_outbound, tick)
//! - Transaction building and fragmentation
//! - Signature operations
//! - Metrics and diagnostics

#[cfg(feature = "android")]
pub mod android;
#[cfg(feature = "ios")]
pub mod ios;
pub mod types;
pub mod runtime;
#[cfg(feature = "android")]
pub mod transport;

#[cfg(feature = "android")]
pub use android::*;
#[cfg(feature = "ios")]
pub use ios::*;
pub use types::*;

