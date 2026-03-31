//! Bluetooth Low Energy mesh networking for PolliNet SDK
//!
//! Handles BLE advertising, scanning, and relay functionality for transaction propagation

// Platform-agnostic BLE adapter interface
pub mod adapter;

// Bridge between new adapter and legacy functionality
pub mod bridge;

// Mesh protocol implementation
pub mod mesh;

// Peer discovery and connection management
pub mod peer_manager;

// Transaction fragmentation and reassembly
pub mod fragmenter;

// Transaction broadcasting across mesh
pub mod broadcaster;

// Mesh health monitoring
pub mod health_monitor;

// Platform-specific implementations
// Linux is kept for desktop simulation only; Android is the production path.
#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "android")]
pub mod android;

// Re-export the main adapter interface
pub use adapter::{
    create_ble_adapter, AdapterInfo, BleAdapter, BleError as AdapterBleError,
    POLLINET_SERVICE_NAME, POLLINET_SERVICE_UUID,
};

// Re-export mesh types
pub use mesh::{
    MeshError, MeshHeader, MeshPacket, MeshRouter, MeshStats, PacketType, TransactionFragment,
    DEFAULT_TTL, MAX_FRAGMENTS, MAX_FRAGMENT_DATA, MAX_HOPS, MAX_PAYLOAD_SIZE,
};

// Re-export peer manager types
pub use peer_manager::{
    PeerCallbacks, PeerInfo, PeerManager, PeerManagerStats, PeerState, MAX_CONNECTIONS,
    MIN_CONNECTIONS, TARGET_CONNECTIONS,
};

// Re-export fragmenter functions
pub use fragmenter::{fragment_transaction, reconstruct_transaction, FragmentationStats};

// Re-export broadcaster types
pub use broadcaster::{
    BroadcastInfo, BroadcastStatistics, BroadcastStatus, TransactionBroadcaster,
};

// Re-export health monitor types
pub use health_monitor::{
    HealthConfig, HealthMetrics, HealthSnapshot, MeshHealthMonitor, NetworkTopology, PeerHealth,
    PeerState as HealthPeerState,
};
