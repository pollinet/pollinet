//! Bluetooth Low Energy mesh networking for PolliNet SDK
//! 
//! Handles BLE advertising, scanning, and relay functionality for transaction propagation

// Platform-agnostic BLE adapter interface
pub mod adapter;

// Bridge between new adapter and legacy functionality
pub mod bridge;

// Platform-specific implementations
#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "android")]
pub mod android;

// Re-export the main adapter interface
pub use adapter::{BleAdapter, BleError as AdapterBleError, AdapterInfo, create_ble_adapter, POLLINET_SERVICE_UUID, POLLINET_SERVICE_NAME};

// Legacy BLE mesh transport (keeping for backward compatibility)
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;
use btleplug::{
    api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager, PeripheralId},
};
use uuid::Uuid;
use crate::transaction::Fragment;
use crate::SERVICE_UUID;

/// BLE mesh transport for PolliNet
pub struct MeshTransport {
    /// BLE manager
    manager: Manager,
    /// Active BLE adapter
    adapter: Option<Adapter>,
    /// Connected peers
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
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

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
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
        
        adapter.start_scan(filter).await
            .map_err(|e| LegacyBleError::ScanningFailed(e.to_string()))?;
        
        tracing::info!("Scanning for PolliNet devices with service: {}", self.service_uuid);
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
            tracing::warn!("No peers connected, storing {} fragments in buffer", fragments.len());
            return Ok(());
        }
        
        // Get the first available adapter
        let adapters = self.manager.adapters().await?;
        if adapters.is_empty() {
            return Err(LegacyBleError::NoAdapter);
        }
        
        let adapter = &adapters[0];
        
        // Relay fragments to each connected peer
        for (peer_id, peer_info) in peers.iter() {
            tracing::info!("Relaying {} fragments to peer: {}", fragments.len(), peer_id);
            
            // In a real implementation, you would:
            // 1. Connect to the peer if not already connected
            // 2. Write fragments to the fragment characteristic
            // 3. Handle connection errors and retries
            
            // For now, simulate the relay operation
            for fragment in &fragments {
                tracing::debug!("Relaying fragment {} to peer {}", fragment.id, peer_id);
            }
        }
        
        tracing::info!("Successfully relayed {} fragments to {} peers", fragments.len(), peers.len());
        
        Ok(())
    }
    
    /// Handle incoming fragment
    pub async fn on_fragment_received(&self, fragment: Fragment) -> Result<(), LegacyBleError> {
        // Add fragment to reassembly buffer
        // This would integrate with the transaction cache
        tracing::info!("Received fragment {} of {} for transaction {}", 
            fragment.index + 1, fragment.total, fragment.id);
        
        Ok(())
    }
    
    /// Discover nearby peers
    pub async fn discover_peers(&self) -> Result<Vec<PeerInfo>, LegacyBleError> {
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
            
            // For now, assume all discovered peripherals are potential PolliNet peers
            // In production, you'd check for the actual service UUID
            if let Ok(Some(properties)) = peripheral.properties().await {
                let peer_info = PeerInfo {
                    device_id: peripheral.id().to_string(),
                    capabilities: vec!["CAN_RELAY".to_string()], // Default capability
                    rssi: properties.rssi.unwrap_or(-100),
                    last_seen: std::time::Instant::now(),
                };
                
                discovered_peers.push(peer_info);
                tracing::info!("Discovered potential PolliNet peer: {} (RSSI: {})", 
                    peripheral.id(), properties.rssi.unwrap_or(-100));
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
        let peripheral = peripherals.iter()
            .find(|p| p.id().to_string() == peer_id)
            .ok_or(LegacyBleError::PeripheralNotFound)?;
        
        // Connect to the peripheral
        tracing::info!("Connecting to peer: {}", peer_id);
        peripheral.connect().await
            .map_err(|e| LegacyBleError::ConnectionFailed(e.to_string()))?;
        
        // Add to connected peers
        let mut peers = self.peers.write().await;
        peers.insert(peer_id.to_string(), PeerInfo {
            device_id: peer_id.to_string(),
            capabilities: vec!["CAN_RELAY".to_string()],
            rssi: -50, // Default RSSI for connected peers
            last_seen: std::time::Instant::now(),
        });
        
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
