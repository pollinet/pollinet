//! Windows BLE implementation stub
//! 
//! This module provides a stub implementation for Windows.
//! In the future, this will use Windows Bluetooth APIs.

use super::adapter::{BleAdapter, BleError, AdapterInfo};
use async_trait::async_trait;

/// Windows BLE adapter implementation (stub)
pub struct WindowsBleAdapter {
    // Future: Windows Bluetooth implementation
}

impl WindowsBleAdapter {
    /// Create a new Windows BLE adapter (stub)
    pub async fn new() -> Result<Self, BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE adapter not yet implemented".to_string()
        ))
    }
}

#[async_trait]
impl BleAdapter for WindowsBleAdapter {
    async fn start_advertising(&self, _service_uuid: &str, _service_name: &str) -> Result<(), BleError> {
        unimplemented!("Windows BLE adapter not yet implemented")
    }

    async fn stop_advertising(&self) -> Result<(), BleError> {
        unimplemented!("Windows BLE adapter not yet implemented")
    }

    async fn send_packet(&self, _data: &[u8]) -> Result<(), BleError> {
        unimplemented!("Windows BLE adapter not yet implemented")
    }

    fn on_receive(&self, _callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        unimplemented!("Windows BLE adapter not yet implemented")
    }

    fn is_advertising(&self) -> bool {
        false
    }

    async fn connected_clients_count(&self) -> usize {
        0
    }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "Windows".to_string(),
            name: "Windows Bluetooth (Stub)".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: false,
            discoverable: false,
        }
    }

    async fn start_scanning(&self) -> Result<(), BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE scanning not yet implemented".to_string()
        ))
    }

    async fn stop_scanning(&self) -> Result<(), BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE scanning not yet implemented".to_string()
        ))
    }

    async fn get_discovered_devices(&self) -> Result<Vec<super::adapter::DiscoveredDevice>, BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE discovery not yet implemented".to_string()
        ))
    }
}
