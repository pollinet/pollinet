//! Windows BLE adapter stub — not yet implemented.

use super::adapter::{AdapterInfo, BleAdapter, BleError, DiscoveredDevice};
use async_trait::async_trait;

pub struct WindowsBleAdapter;

impl WindowsBleAdapter {
    pub async fn new() -> Result<Self, BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE not yet implemented".to_string(),
        ))
    }
}

#[async_trait]
impl BleAdapter for WindowsBleAdapter {
    async fn start_advertising(&self, _: &str, _: &str) -> Result<(), BleError> {
        unimplemented!()
    }
    async fn stop_advertising(&self) -> Result<(), BleError> {
        unimplemented!()
    }
    async fn send_packet(&self, _: &[u8]) -> Result<(), BleError> {
        unimplemented!()
    }
    fn on_receive(&self, _: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        unimplemented!()
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
            "Windows BLE not yet implemented".to_string(),
        ))
    }
    async fn stop_scanning(&self) -> Result<(), BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE not yet implemented".to_string(),
        ))
    }
    async fn get_discovered_devices(&self) -> Result<Vec<DiscoveredDevice>, BleError> {
        Err(BleError::OperationNotSupported(
            "Windows BLE not yet implemented".to_string(),
        ))
    }
}
