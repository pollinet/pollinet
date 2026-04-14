//! FFI module — Android JNI and iOS C interfaces
//!
//! - `android`: JNI bindings for Kotlin/Java
//! - `ios`:     plain `extern "C"` bindings for Swift via a generated C header
//! - `transport`: host-driven BLE transport shared by both platforms
//! - `types`:   JSON-serialisable request/response types shared by both platforms

pub mod android;
pub mod ios;
pub mod runtime;
pub mod transport;
pub mod types;

pub use android::*;
pub use types::*;
