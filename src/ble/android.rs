//! Android BLE implementation stub
//! 
//! This module provides a stub implementation for Android.
//! In the future, this will use Android Bluetooth APIs via JNI or a native crate.

use super::adapter::{BleAdapter, BleError, AdapterInfo};
use async_trait::async_trait;

/// Android BLE adapter implementation (stub)
pub struct AndroidBleAdapter {
    // Future: Android Bluetooth implementation
}

impl AndroidBleAdapter {
    /// Create a new Android BLE adapter (stub)
    pub async fn new() -> Result<Self, BleError> {
        Err(BleError::OperationNotSupported(
            "Android BLE adapter not yet implemented".to_string()
        ))
    }
}

#[async_trait]
impl BleAdapter for AndroidBleAdapter {
    async fn start_advertising(&self, _service_uuid: &str, _service_name: &str) -> Result<(), BleError> {
        unimplemented!("Android BLE adapter not yet implemented")
    }

    async fn stop_advertising(&self) -> Result<(), BleError> {
        unimplemented!("Android BLE adapter not yet implemented")
    }

    async fn send_packet(&self, _data: &[u8]) -> Result<(), BleError> {
        unimplemented!("Android BLE adapter not yet implemented")
    }

    fn on_receive(&self, _callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        unimplemented!("Android BLE adapter not yet implemented")
    }

    fn is_advertising(&self) -> bool {
        false
    }

    fn connected_clients_count(&self) -> usize {
        0
    }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "Android".to_string(),
            name: "Android Bluetooth (Stub)".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: false,
            discoverable: false,
        }
    }
}
