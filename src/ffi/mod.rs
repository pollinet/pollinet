//! FFI module for host-platform integration (Android JNI + iOS C ABI)
//!
//! This module provides the platform bindings that Kotlin (via JNI) and Swift
//! (via a C ABI) use to interact with the PolliNet Rust core. It handles:
//! - Host-driven BLE transport (push_inbound, next_outbound, tick)
//! - Transaction building and fragmentation
//! - Signature operations
//! - Metrics and diagnostics
//!
//! The shared operation bodies live in `common` (feature-gated); `android` and
//! `ios` are thin marshalling shims over it.

pub mod android;
#[cfg(any(feature = "android", feature = "ios"))]
pub(crate) mod common;
pub mod host_transport;
#[cfg(feature = "ios")]
pub mod ios;
pub mod runtime;
pub mod transport;
pub mod types;
pub mod wifi_direct_transport;

#[cfg(feature = "android")]
pub use android::*;
pub use host_transport::HostTransport;
pub use types::*;
pub use wifi_direct_transport::HostWifiDirectTransport;
