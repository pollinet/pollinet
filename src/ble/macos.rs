//! macOS BLE implementation stub
//! 
//! This module provides a stub implementation for macOS.
//! In the future, this will use Core Bluetooth via FFI or a native crate.

use super::adapter::{BleAdapter, BleError, AdapterInfo};
use async_trait::async_trait;

/// macOS BLE adapter implementation (stub)
pub struct MacOSBleAdapter {
    // Future: Core Bluetooth implementation
}

impl MacOSBleAdapter {
    /// Create a new macOS BLE adapter (stub)
    pub async fn new() -> Result<Self, BleError> {
        Err(BleError::OperationNotSupported(
            "macOS BLE adapter not yet implemented".to_string()
        ))
    }
}

#[async_trait]
impl BleAdapter for MacOSBleAdapter {
    async fn start_advertising(&self, _service_uuid: &str, _service_name: &str) -> Result<(), BleError> {
        unimplemented!("macOS BLE adapter not yet implemented")
    }

    async fn stop_advertising(&self) -> Result<(), BleError> {
        unimplemented!("macOS BLE adapter not yet implemented")
    }

    async fn send_packet(&self, _data: &[u8]) -> Result<(), BleError> {
        unimplemented!("macOS BLE adapter not yet implemented")
    }

    fn on_receive(&self, _callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        unimplemented!("macOS BLE adapter not yet implemented")
    }

    fn is_advertising(&self) -> bool {
        false
    }

    fn connected_clients_count(&self) -> usize {
        0
    }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "macOS".to_string(),
            name: "Core Bluetooth (Stub)".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: false,
            discoverable: false,
        }
    }

    async fn start_scanning(&self) -> Result<(), BleError> {
        unimplemented!("macOS BLE adapter not yet implemented")
    }

    async fn stop_scanning(&self) -> Result<(), BleError> {
        unimplemented!("macOS BLE adapter not yet implemented")
    }

    async fn get_discovered_devices(&self) -> Result<Vec<super::adapter::DiscoveredDevice>, BleError> {
        unimplemented!("macOS BLE adapter not yet implemented")
    }
}
