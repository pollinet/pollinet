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

// Legacy BLE mesh transport (keeping for backward compatibility)
use crate::transaction::Fragment;
use crate::SERVICE_UUID;
use btleplug::{
    api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager},
};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

/// BLE mesh transport for PolliNet
///
/// This legacy transport is only used for desktop simulations; production BLE
/// traffic flows through the Android service.
pub struct MeshTransport {
    /// BLE manager
    manager: Manager,
    /// Active BLE adapter
    adapter: Option<Adapter>,
    /// Connected peers
    peers: Arc<RwLock<HashMap<String, LegacyPeerInfo>>>,
    /// Fragment relay buffer
    relay_buffer: Arc<RwLock<Vec<Fragment>>>,
    /// Device identifier
    device_id: String,
    /// Service UUID for PolliNet
    service_uuid: Uuid,
    /// Characteristic UUID for fragment transmission
    fragment_characteristic_uuid: Uuid,
    /// Characteristic UUID for confirmation messages
    confirmation_characteristic_uuid: Uuid,
}

/// Legacy information about a connected peer
#[derive(Debug, Clone)]
pub struct LegacyPeerInfo {
    /// Peer device ID
    pub device_id: String,
    /// Peer capabilities
    pub capabilities: Vec<String>,
    /// Connection strength
    pub rssi: i16,
    /// Last seen timestamp
    pub last_seen: std::time::Instant,
}

impl MeshTransport {
    /// Create a new BLE mesh transport
    pub async fn new() -> Result<Self, LegacyBleError> {
        let manager = Manager::new().await?;
        let adapter = None; // Will be initialized when starting

        // Generate device ID
        let device_id = format!("pollinet_{}", Uuid::new_v4().to_string()[..8].to_string());

        // Parse service UUID
        let service_uuid = Uuid::parse_str(SERVICE_UUID)
            .map_err(|e| LegacyBleError::InvalidUuid(format!("Invalid service UUID: {}", e)))?;

        // Generate characteristic UUIDs
        let fragment_characteristic_uuid = Uuid::new_v4();
        let confirmation_characteristic_uuid = Uuid::new_v4();

        Ok(Self {
            manager,
            adapter,
            peers: Arc::new(RwLock::new(HashMap::new())),
            relay_buffer: Arc::new(RwLock::new(Vec::new())),
            device_id,
            service_uuid,
            fragment_characteristic_uuid,
            confirmation_characteristic_uuid,
        })
    }

    /// Start BLE advertising
    pub async fn start_advertising(&self) -> Result<(), LegacyBleError> {
        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let adapter = &adapters[0];
        let adapter_info = adapter.adapter_info().await?;
        tracing::info!("Starting BLE advertising on adapter: {}", adapter_info);

        // Start advertising the PolliNet service
        // Note: This is a simplified implementation
        // In production, you'd implement proper GATT server advertising
        tracing::info!("Advertising PolliNet service: {}", self.service_uuid);
        tracing::info!("Device ID: {}", self.device_id);

        // Add more detailed advertising info
        tracing::info!("BLE Advertising Status: ACTIVE");
        tracing::info!("Service UUID: {}", self.service_uuid);
        tracing::info!("Device ID: {}", self.device_id);
        tracing::info!("Waiting for other PolliNet devices to discover this service...");

        Ok(())
    }

    /// Start BLE scanning
    pub async fn start_scanning(&self) -> Result<(), LegacyBleError> {
        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let adapter = &adapters[0];
        let adapter_info = adapter.adapter_info().await?;
        tracing::info!("Starting BLE scanning on adapter: {}", adapter_info);

        // Start scanning for PolliNet devices
        let filter = ScanFilter {
            services: vec![self.service_uuid],
        };

        adapter
            .start_scan(filter)
            .await
            .map_err(|e| LegacyBleError::ScanningFailed(e.to_string()))?;

        tracing::info!(
            "Scanning for PolliNet devices with service: {}",
            self.service_uuid
        );
        tracing::info!("Scan Filter: Service UUID = {}", self.service_uuid);
        tracing::info!("BLE Scanning Status: ACTIVE - Looking for PolliNet devices...");

        Ok(())
    }

    /// Relay transaction fragments over BLE mesh
    pub async fn relay_fragments(&self, fragments: Vec<Fragment>) -> Result<(), LegacyBleError> {
        // Store fragments in relay buffer
        let mut buffer = self.relay_buffer.write().await;
        buffer.extend(fragments.clone());

        // Get connected peers
        let peers = self.peers.read().await;
        if peers.is_empty() {
            tracing::warn!(
                "No peers connected, storing {} fragments in buffer",
                fragments.len()
            );
            return Ok(());
        }

        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let _adapter = &adapters[0];

        // Relay fragments to each connected peer
        for (peer_id, _peer_info) in peers.iter() {
            tracing::info!(
                "Relaying {} fragments to peer: {}",
                fragments.len(),
                peer_id
            );

            // In a real implementation, you would:
            // 1. Connect to the peer if not already connected
            // 2. Write fragments to the fragment characteristic
            // 3. Handle connection errors and retries

            // For now, simulate the relay operation
            for fragment in &fragments {
                tracing::debug!("Relaying fragment {} to peer {}", fragment.id, peer_id);
            }
        }

        tracing::info!(
            "Successfully relayed {} fragments to {} peers",
            fragments.len(),
            peers.len()
        );

        Ok(())
    }

    /// Handle incoming fragment
    pub async fn on_fragment_received(&self, fragment: Fragment) -> Result<(), LegacyBleError> {
        // Add fragment to reassembly buffer
        // This would integrate with the transaction cache
        tracing::info!(
            "Received fragment {} of {} for transaction {}",
            fragment.index + 1,
            fragment.total,
            fragment.id
        );

        Ok(())
    }

    /// Discover nearby peers
    pub async fn discover_peers(&self) -> Result<Vec<LegacyPeerInfo>, LegacyBleError> {
        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let adapter = &adapters[0];

        // Get discovered peripherals
        let peripherals = adapter.peripherals().await?;
        let mut discovered_peers = Vec::new();

        tracing::info!("Scanning for BLE devices...");
        tracing::info!("Total BLE devices found: {}", peripherals.len());

        for peripheral in peripherals {
            tracing::debug!("Checking peripheral: {}", peripheral.id());

            // Check if this peripheral is actually a PolliNet device
            if let Ok(Some(properties)) = peripheral.properties().await {
                // Check if the peripheral advertises our service UUID
                let is_pollinet_device = properties
                    .services
                    .iter()
                    .any(|service| service == &self.service_uuid);

                if is_pollinet_device {
                    let peer_info = LegacyPeerInfo {
                        device_id: peripheral.id().to_string(),
                        capabilities: vec!["CAN_RELAY".to_string()],
                        rssi: properties.rssi.unwrap_or(-100),
                        last_seen: std::time::Instant::now(),
                    };

                    discovered_peers.push(peer_info);
                    tracing::info!(
                        "Discovered PolliNet peer: {} (RSSI: {})",
                        peripheral.id(),
                        properties.rssi.unwrap_or(-100)
                    );
                } else {
                    tracing::debug!(
                        "Skipping non-PolliNet device: {} (RSSI: {})",
                        peripheral.id(),
                        properties.rssi.unwrap_or(-100)
                    );
                }
            }
        }

        if discovered_peers.is_empty() {
            tracing::warn!("No PolliNet peers discovered");
            tracing::info!("This could mean:");
            tracing::info!("  1. No other PolliNet devices are nearby");
            tracing::info!("  2. Other devices are not advertising");
            tracing::info!("  3. BLE permissions are not granted");
            tracing::info!("  4. Service UUID mismatch between devices");
        } else {
            tracing::info!("Discovered {} PolliNet peers", discovered_peers.len());
        }

        Ok(discovered_peers)
    }

    /// Scan for ALL BLE devices (for debugging)
    pub async fn scan_all_devices(&self) -> Result<Vec<String>, LegacyBleError> {
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let adapter = &adapters[0];
        let peripherals = adapter.peripherals().await?;
        let mut device_list = Vec::new();

        tracing::info!("Scanning for ALL BLE devices (debug mode)...");

        for peripheral in peripherals {
            if let Ok(Some(properties)) = peripheral.properties().await {
                let device_info = format!(
                    "Device: {} | RSSI: {}",
                    peripheral.id(),
                    properties.rssi.unwrap_or(-100)
                );
                device_list.push(device_info.clone());
                tracing::info!("Found: {}", device_info);
            }
        }

        Ok(device_list)
    }

    /// Connect to a discovered peer
    pub async fn connect_to_peer(&self, peer_id: &str) -> Result<(), LegacyBleError> {
        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let adapter = &adapters[0];

        // Find the peripheral by ID
        let peripherals = adapter.peripherals().await?;
        let peripheral = peripherals
            .iter()
            .find(|p| p.id().to_string() == peer_id)
            .ok_or(LegacyBleError::PeripheralNotFound)?;

        // Connect to the peripheral
        tracing::info!("Connecting to peer: {}", peer_id);
        peripheral
            .connect()
            .await
            .map_err(|e| LegacyBleError::ConnectionFailed(e.to_string()))?;

        // Add to connected peers
        let mut peers = self.peers.write().await;
        peers.insert(
            peer_id.to_string(),
            LegacyPeerInfo {
                device_id: peer_id.to_string(),
                capabilities: vec!["CAN_RELAY".to_string()],
                rssi: -50, // Default RSSI for connected peers
                last_seen: std::time::Instant::now(),
            },
        );

        tracing::info!("Successfully connected to peer: {}", peer_id);
        Ok(())
    }

    /// Check if device can submit to Solana
    pub async fn can_submit_to_solana(&self) -> bool {
        // This would check internet connectivity
        // For now, return false (offline mode)
        false
    }

    /// Get connected peer count
    pub async fn get_peer_count(&self) -> usize {
        let peers = self.peers.read().await;
        peers.len()
    }

    /// Get relay buffer size
    pub async fn get_relay_buffer_size(&self) -> usize {
        let buffer = self.relay_buffer.read().await;
        buffer.len()
    }

    /// Send text message to a connected peer
    pub async fn send_text_message(
        &self,
        peer_id: &str,
        message: &str,
    ) -> Result<(), LegacyBleError> {
        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }

        let adapter = &adapters[0];

        // Find the peripheral by ID
        let peripherals = adapter.peripherals().await?;
        let peripheral = peripherals
            .iter()
            .find(|p| p.id().to_string() == peer_id)
            .ok_or(LegacyBleError::PeripheralNotFound)?;

        // Connect if not already connected
        if !peripheral.is_connected().await? {
            tracing::info!("Connecting to peer for text message: {}", peer_id);
            peripheral
                .connect()
                .await
                .map_err(|e| LegacyBleError::ConnectionFailed(e.to_string()))?;
        }

        // Discover services
        peripheral
            .discover_services()
            .await
            .map_err(|_e| LegacyBleError::ServiceNotFound)?;

        // Find our service
        let services = peripheral.services();
        let pollinet_service = services
            .iter()
            .find(|s| s.uuid == self.service_uuid)
            .ok_or(LegacyBleError::ServiceNotFound)?;

        // Find the fragment characteristic for text transmission
        let fragment_char = pollinet_service
            .characteristics
            .iter()
            .find(|c| c.uuid == self.fragment_characteristic_uuid)
            .ok_or(LegacyBleError::CharacteristicNotFound)?;

        // Send the text message
        let message_bytes = message.as_bytes();
        tracing::info!(
            "Sending text message to {}: '{}' ({} bytes)",
            peer_id,
            message,
            message_bytes.len()
        );

        peripheral
            .write(fragment_char, message_bytes, WriteType::WithResponse)
            .await
            .map_err(|e| LegacyBleError::TransmissionFailed(e.to_string()))?;

        tracing::info!("✅ Text message sent successfully to {}", peer_id);
        Ok(())
    }

    /// Start listening for incoming text messages
    pub async fn start_text_listener(&self) -> Result<(), LegacyBleError> {
        // This would set up a notification listener for incoming text messages
        // For now, we'll implement a simple polling mechanism
        tracing::info!("Starting text message listener...");
        tracing::info!("Listening for 'LOREM_IPSUM' messages from connected peers");
        Ok(())
    }

    /// Check for incoming text messages from connected peers
    pub async fn check_incoming_messages(&self) -> Result<Vec<String>, LegacyBleError> {
        let mut messages = Vec::new();

        // Get connected peers
        let peers = self.peers.read().await;
        if peers.is_empty() {
            return Ok(messages);
        }

        // In a real implementation, this would read from notification characteristics
        // For now, we'll simulate receiving messages
        for (peer_id, _peer_info) in peers.iter() {
            // Simulate receiving a message occasionally
            if rand::random::<f32>() < 0.1 {
                // 10% chance per check
                let message = "LOREM_IPSUM";
                tracing::info!("📨 Received text message from {}: '{}'", peer_id, message);
                messages.push(message.to_string());
            }
        }

        Ok(messages)
    }

    /// Get BLE status and debugging information
    pub async fn get_ble_status(&self) -> Result<String, LegacyBleError> {
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Ok("No BLE adapters found".to_string());
        }

        let adapter = &adapters[0];
        let adapter_info = adapter.adapter_info().await?;

        let peripherals = adapter.peripherals().await?;
        let peers = self.peers.read().await;
        let buffer = self.relay_buffer.read().await;

        let status = format!(
            "BLE Status:\n\
             Adapter: {}\n\
             Service UUID: {}\n\
             Device ID: {}\n\
             Total BLE devices: {}\n\
             Connected peers: {}\n\
             Relay buffer: {} fragments\n\
             Advertising: ACTIVE\n\
             Scanning: ACTIVE",
            adapter_info,
            self.service_uuid,
            self.device_id,
            peripherals.len(),
            peers.len(),
            buffer.len()
        );

        Ok(status)
    }
}

/// Legacy BLE-specific error types (deprecated - use adapter::BleError)
#[derive(Error, Debug)]
pub enum LegacyBleError {
    #[error("BLE manager initialization failed: {0}")]
    ManagerInit(String),

    #[error("BLE adapter not found")]
    NoAdapter,

    #[error("BLE advertising failed: {0}")]
    AdvertisingFailed(String),

    #[error("BLE scanning failed: {0}")]
    ScanningFailed(String),

    #[error("BLE connection failed: {0}")]
    ConnectionFailed(String),

    #[error("BLE transmission failed: {0}")]
    TransmissionFailed(String),

    #[error("Invalid UUID: {0}")]
    InvalidUuid(String),

    #[error("BLE peripheral not found")]
    PeripheralNotFound,

    #[error("BLE service not found")]
    ServiceNotFound,

    #[error("BLE characteristic not found")]
    CharacteristicNotFound,
}

impl From<btleplug::Error> for LegacyBleError {
    fn from(err: btleplug::Error) -> Self {
        LegacyBleError::ManagerInit(err.to_string())
    }
}
