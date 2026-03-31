//! Android BLE adapter stub — all BLE operations are handled by the Android
//! BleService via JNI; this adapter is a no-op that lets SDK initialization succeed.

use super::adapter::{AdapterInfo, BleAdapter, BleError, DiscoveredDevice};
use async_trait::async_trait;

pub struct AndroidBleAdapter;

impl AndroidBleAdapter {
    pub async fn new() -> Result<Self, BleError> {
        tracing::info!("Creating Android BLE adapter stub (host-driven mode)");
        Ok(AndroidBleAdapter)
    }
}

#[async_trait]
impl BleAdapter for AndroidBleAdapter {
    async fn start_advertising(&self, _service_uuid: &str, _service_name: &str) -> Result<(), BleError> { Ok(()) }
    async fn stop_advertising(&self) -> Result<(), BleError> { Ok(()) }
    async fn send_packet(&self, _data: &[u8]) -> Result<(), BleError> { Ok(()) }
    fn on_receive(&self, _callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {}
    fn is_advertising(&self) -> bool { false }
    async fn connected_clients_count(&self) -> usize { 0 }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "Android".to_string(),
            name: "Android Bluetooth (Host-Driven)".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: true,
            discoverable: false,
        }
    }

    async fn start_scanning(&self) -> Result<(), BleError> { Ok(()) }
    async fn stop_scanning(&self) -> Result<(), BleError> { Ok(()) }
    async fn get_discovered_devices(&self) -> Result<Vec<DiscoveredDevice>, BleError> { Ok(Vec::new()) }
    async fn connect_to_device(&self, _address: &str) -> Result<(), BleError> { Ok(()) }
    async fn write_to_device(&self, _address: &str, _data: &[u8]) -> Result<(), BleError> { Ok(()) }
    async fn read_from_device(&self, _address: &str) -> Result<Vec<u8>, BleError> { Ok(Vec::new()) }
}
