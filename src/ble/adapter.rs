//! Platform-agnostic Bluetooth GATT adapter for PolliNet SDK
//! 
//! This module defines the unified interface that allows platform-specific backends
//! to plug in seamlessly.

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

/// Platform-agnostic BLE adapter trait
/// 
/// This trait defines the core functionality that must be implemented
/// for each platform-specific BLE backend.
#[async_trait]
pub trait BleAdapter: Send + Sync {
    /// Start advertising the PolliNet service
    /// 
    /// # Arguments
    /// * `service_uuid` - The UUID of the service to advertise
    /// * `service_name` - Human-readable name for the service
    /// 
    /// # Returns
    /// * `Ok(())` if advertising started successfully
    /// * `Err(BleError)` if advertising failed
    async fn start_advertising(&self, service_uuid: &str, service_name: &str) -> Result<(), BleError>;
    
    /// Stop advertising the service
    /// 
    /// # Returns
    /// * `Ok(())` if advertising stopped successfully
    /// * `Err(BleError)` if stopping advertising failed
    async fn stop_advertising(&self) -> Result<(), BleError>;
    
    /// Send a packet to connected clients
    /// 
    /// # Arguments
    /// * `data` - The data to send
    /// 
    /// # Returns
    /// * `Ok(())` if packet was sent successfully
    /// * `Err(BleError)` if sending failed
    async fn send_packet(&self, data: &[u8]) -> Result<(), BleError>;
    
    /// Set callback for incoming data using a boxed closure
    /// 
    /// # Arguments
    /// * `callback` - Boxed closure to call when data is received
    /// 
    /// The callback will be called with the received data as a `Vec<u8>`.
    fn on_receive(&self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>);
    
    /// Check if the adapter is currently advertising
    fn is_advertising(&self) -> bool;
    
    /// Get the number of connected clients
    fn connected_clients_count(&self) -> usize;
    
    /// Get adapter information (platform-specific details)
    fn get_adapter_info(&self) -> AdapterInfo;
    
    /// Start scanning for nearby BLE devices
    async fn start_scanning(&self) -> Result<(), BleError>;
    
    /// Stop scanning for nearby BLE devices
    async fn stop_scanning(&self) -> Result<(), BleError>;
    
    /// Get list of discovered devices
    async fn get_discovered_devices(&self) -> Result<Vec<DiscoveredDevice>, BleError>;
    
    /// Connect to a discovered device (Central role only)
    async fn connect_to_device(&self, address: &str) -> Result<(), BleError> {
        Err(BleError::OperationNotSupported("Connection not supported on this platform".to_string()))
    }
    
    /// Write data to a connected device (Central role only)
    async fn write_to_device(&self, address: &str, data: &[u8]) -> Result<(), BleError> {
        Err(BleError::OperationNotSupported("Writing to devices not supported on this platform".to_string()))
    }
}

/// Information about the BLE adapter
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    /// Platform name (e.g., "Linux", "macOS", "Windows")
    pub platform: String,
    /// Adapter name or identifier
    pub name: String,
    /// Adapter address
    pub address: String,
    /// Whether the adapter is powered on
    pub powered: bool,
    /// Whether the adapter is discoverable
    pub discoverable: bool,
}

impl std::fmt::Display for AdapterInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}) - {} - Powered: {} - Discoverable: {}", 
               self.name, self.platform, self.address, self.powered, self.discoverable)
    }
}

/// Information about a discovered BLE device
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub address: String,
    pub name: Option<String>,
    pub service_uuids: Vec<Uuid>,
    pub rssi: Option<i16>,
    pub last_seen: std::time::Instant,
}

/// BLE-specific error types
#[derive(Error, Debug)]
pub enum BleError {
    #[error("BLE adapter not available")]
    AdapterNotAvailable,
    
    #[error("BLE adapter not powered")]
    AdapterNotPowered,
    
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
    
    #[error("BLE permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("BLE operation not supported: {0}")]
    OperationNotSupported(String),
    
    #[error("BLE timeout")]
    Timeout,
    
    #[error("BLE invalid state: {0}")]
    InvalidState(String),
    
    #[error("Platform error: {0}")]
    PlatformError(String),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
}

/// Factory function to create a platform-specific BLE adapter
pub async fn create_ble_adapter() -> Result<Box<dyn BleAdapter>, BleError> {
    #[cfg(target_os = "linux")]
    {
        use crate::ble::linux::LinuxBleAdapter;
        Ok(Box::new(LinuxBleAdapter::new().await?))
    }
    
    #[cfg(target_os = "macos")]
    {
        use crate::ble::macos::MacOSBleAdapter;
        Ok(Box::new(MacOSBleAdapter::new().await?))
    }
    
    #[cfg(target_os = "windows")]
    {
        use crate::ble::windows::WindowsBleAdapter;
        Ok(Box::new(WindowsBleAdapter::new().await?))
    }
    
    #[cfg(target_os = "android")]
    {
        use crate::ble::android::AndroidBleAdapter;
        Ok(Box::new(AndroidBleAdapter::new().await?))
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        Err(BleError::OperationNotSupported("Unsupported platform".to_string()))
    }
}

/// PolliNet service UUID constant
pub const POLLINET_SERVICE_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7";

/// PolliNet service name
pub const POLLINET_SERVICE_NAME: &str = "PolliNet";
