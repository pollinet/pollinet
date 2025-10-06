//! Platform-agnostic GATT traits for PolliNet
//! 
//! These traits define the interface that must be implemented for each platform

use async_trait::async_trait;
use uuid::Uuid;
use std::sync::Arc;
use thiserror::Error;

/// Result type for GATT operations
pub type GattResult<T> = Result<T, GattError>;

/// Platform-agnostic GATT Server trait
#[async_trait]
pub trait GattServer: Send + Sync {
    /// Start the GATT server with the given service
    async fn start(&mut self, service: GattServiceDefinition) -> GattResult<()>;
    
    /// Stop the GATT server
    async fn stop(&mut self) -> GattResult<()>;
    
    /// Update a characteristic value
    async fn update_characteristic(
        &mut self,
        char_uuid: Uuid,
        value: Vec<u8>,
    ) -> GattResult<()>;
    
    /// Send notification to subscribed clients
    async fn notify_characteristic(
        &mut self,
        char_uuid: Uuid,
        value: Vec<u8>,
    ) -> GattResult<()>;
    
    /// Send indication to subscribed clients
    async fn indicate_characteristic(
        &mut self,
        char_uuid: Uuid,
        value: Vec<u8>,
    ) -> GattResult<()>;
    
    /// Get current characteristic value
    async fn get_characteristic_value(&self, char_uuid: Uuid) -> GattResult<Vec<u8>>;
    
    /// Check if server is running
    fn is_running(&self) -> bool;
    
    /// Get connected clients count
    fn get_connected_clients_count(&self) -> usize;
}

/// Platform-agnostic GATT Client trait
#[async_trait]
pub trait GattClient: Send + Sync {
    /// Connect to a remote GATT server
    async fn connect(&mut self, device_id: &str) -> GattResult<()>;
    
    /// Disconnect from the remote GATT server
    async fn disconnect(&mut self) -> GattResult<()>;
    
    /// Discover services on the connected device
    async fn discover_services(&mut self) -> GattResult<Vec<Uuid>>;
    
    /// Discover characteristics for a service
    async fn discover_characteristics(
        &mut self,
        service_uuid: Uuid,
    ) -> GattResult<Vec<CharacteristicInfo>>;
    
    /// Read a characteristic value
    async fn read_characteristic(&mut self, char_uuid: Uuid) -> GattResult<Vec<u8>>;
    
    /// Write a characteristic value with response
    async fn write_characteristic(
        &mut self,
        char_uuid: Uuid,
        value: Vec<u8>,
    ) -> GattResult<()>;
    
    /// Write a characteristic value without response
    async fn write_characteristic_without_response(
        &mut self,
        char_uuid: Uuid,
        value: Vec<u8>,
    ) -> GattResult<()>;
    
    /// Subscribe to characteristic notifications
    async fn subscribe_to_notifications(
        &mut self,
        char_uuid: Uuid,
        callback: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> GattResult<()>;
    
    /// Unsubscribe from characteristic notifications
    async fn unsubscribe_from_notifications(&mut self, char_uuid: Uuid) -> GattResult<()>;
    
    /// Check if connected
    fn is_connected(&self) -> bool;
    
    /// Get connected device ID
    fn get_device_id(&self) -> Option<String>;
}

/// Platform-agnostic BLE Advertiser trait
#[async_trait]
pub trait BleAdvertiser: Send + Sync {
    /// Start advertising with service UUID and data
    async fn start_advertising(
        &mut self,
        service_uuid: Uuid,
        service_data: Vec<u8>,
        device_name: Option<String>,
    ) -> GattResult<()>;
    
    /// Stop advertising
    async fn stop_advertising(&mut self) -> GattResult<()>;
    
    /// Update advertising data
    async fn update_advertising_data(&mut self, service_data: Vec<u8>) -> GattResult<()>;
    
    /// Check if advertising
    fn is_advertising(&self) -> bool;
}

/// Platform-agnostic BLE Scanner trait
#[async_trait]
pub trait BleScanner: Send + Sync {
    /// Start scanning for devices with optional service filter
    async fn start_scan(&mut self, service_filter: Option<Uuid>) -> GattResult<()>;
    
    /// Stop scanning
    async fn stop_scan(&mut self) -> GattResult<()>;
    
    /// Get discovered devices
    async fn get_discovered_devices(&self) -> GattResult<Vec<DiscoveredDevice>>;
    
    /// Set scan callback for real-time discovery
    async fn set_scan_callback(
        &mut self,
        callback: Arc<dyn Fn(DiscoveredDevice) + Send + Sync>,
    ) -> GattResult<()>;
    
    /// Check if scanning
    fn is_scanning(&self) -> bool;
}

/// GATT service definition
#[derive(Debug, Clone)]
pub struct GattServiceDefinition {
    pub uuid: Uuid,
    pub characteristics: Vec<CharacteristicDefinition>,
}

/// Characteristic definition
#[derive(Debug, Clone)]
pub struct CharacteristicDefinition {
    pub uuid: Uuid,
    pub properties: CharacteristicProperties,
    pub permissions: CharacteristicPermissions,
    pub initial_value: Vec<u8>,
    pub descriptors: Vec<DescriptorDefinition>,
    
    /// Callback for read requests
    pub on_read: Option<Arc<dyn Fn() -> Vec<u8> + Send + Sync>>,
    
    /// Callback for write requests
    pub on_write: Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>>,
}

/// Descriptor definition
#[derive(Debug, Clone)]
pub struct DescriptorDefinition {
    pub uuid: Uuid,
    pub value: Vec<u8>,
}

/// Characteristic properties
#[derive(Debug, Clone, Copy)]
pub struct CharacteristicProperties {
    pub read: bool,
    pub write: bool,
    pub write_without_response: bool,
    pub notify: bool,
    pub indicate: bool,
}

/// Characteristic permissions
#[derive(Debug, Clone, Copy)]
pub struct CharacteristicPermissions {
    pub readable: bool,
    pub writable: bool,
    pub encrypted_read: bool,
    pub encrypted_write: bool,
}

/// Information about a characteristic
#[derive(Debug, Clone)]
pub struct CharacteristicInfo {
    pub uuid: Uuid,
    pub properties: CharacteristicProperties,
}

/// Discovered BLE device
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: Option<String>,
    pub rssi: i16,
    pub services: Vec<Uuid>,
    pub service_data: Vec<u8>,
    pub manufacturer_data: Option<Vec<u8>>,
}

/// GATT operation errors
#[derive(Error, Debug)]
pub enum GattError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Disconnection failed: {0}")]
    DisconnectionFailed(String),
    
    #[error("Service not found: {0}")]
    ServiceNotFound(Uuid),
    
    #[error("Characteristic not found: {0}")]
    CharacteristicNotFound(Uuid),
    
    #[error("Read operation failed: {0}")]
    ReadFailed(String),
    
    #[error("Write operation failed: {0}")]
    WriteFailed(String),
    
    #[error("Notification subscription failed: {0}")]
    SubscriptionFailed(String),
    
    #[error("Advertising failed: {0}")]
    AdvertisingFailed(String),
    
    #[error("Scanning failed: {0}")]
    ScanningFailed(String),
    
    #[error("Not connected")]
    NotConnected,
    
    #[error("Already connected")]
    AlreadyConnected,
    
    #[error("Invalid state: {0}")]
    InvalidState(String),
    
    #[error("Platform error: {0}")]
    PlatformError(String),
    
    #[error("Timeout")]
    Timeout,
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl CharacteristicProperties {
    /// Create properties with all flags enabled
    pub fn all() -> Self {
        Self {
            read: true,
            write: true,
            write_without_response: true,
            notify: true,
            indicate: true,
        }
    }
    
    /// Create properties for read-only characteristic
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            write_without_response: false,
            notify: false,
            indicate: false,
        }
    }
    
    /// Create properties for write-only characteristic
    pub fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            write_without_response: true,
            notify: false,
            indicate: false,
        }
    }
    
    /// Create properties for notify characteristic
    pub fn notify() -> Self {
        Self {
            read: true,
            write: false,
            write_without_response: false,
            notify: true,
            indicate: false,
        }
    }
}

impl CharacteristicPermissions {
    /// Create open permissions
    pub fn open() -> Self {
        Self {
            readable: true,
            writable: true,
            encrypted_read: false,
            encrypted_write: false,
        }
    }
    
    /// Create encrypted permissions
    pub fn encrypted() -> Self {
        Self {
            readable: true,
            writable: true,
            encrypted_read: true,
            encrypted_write: true,
        }
    }
    
    /// Create read-only permissions
    pub fn read_only() -> Self {
        Self {
            readable: true,
            writable: false,
            encrypted_read: false,
            encrypted_write: false,
        }
    }
}