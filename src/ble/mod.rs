//! BLE mesh protocol types used by the host-driven transport layer.
//!
//! Actual BLE hardware is driven by the Android host (BleService.kt).
//! This module contains the protocol structs and algorithms for
//! fragment reassembly, broadcast preparation, and network health tracking.

pub mod broadcaster;
pub mod fragmenter;
pub mod health_monitor;
pub mod mesh;

// Fragmenter functions
pub use fragmenter::{fragment_transaction, reconstruct_transaction, FragmentationStats};

// Mesh protocol types
pub use mesh::{
    MeshError, MeshHeader, MeshPacket, MeshRouter, MeshStats, PacketType, TransactionFragment,
    DEFAULT_TTL, MAX_FRAGMENT_DATA, MAX_FRAGMENTS, MAX_HOPS, MAX_PAYLOAD_SIZE,
};

// Broadcaster types
pub use broadcaster::{
    BroadcastInfo, BroadcastStatistics, BroadcastStatus, TransactionBroadcaster,
};

// Health monitor types
pub use health_monitor::{
    HealthConfig, HealthMetrics, HealthSnapshot, MeshHealthMonitor, NetworkTopology, PeerHealth,
    PeerState as HealthPeerState,
};
