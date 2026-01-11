//! Android BLE implementation stub
//!
//! This module provides a stub implementation for Android.
//! In host-driven mode, the Android BleService handles all BLE operations,
//! so this adapter is a no-op stub that allows SDK initialization to succeed.

use super::adapter::{AdapterInfo, BleAdapter, BleError};
use async_trait::async_trait;
use tracing;

/// Android BLE adapter implementation (stub)
pub struct AndroidBleAdapter {
    // Future: Android Bluetooth implementation
}

impl AndroidBleAdapter {
    /// Create a new Android BLE adapter (stub)
    /// 
    /// This is a no-op stub for host-driven BLE mode on Android.
    /// The actual BLE operations are handled by the Android BleService,
    /// so this adapter doesn't need to do anything.
    pub async fn new() -> Result<Self, BleError> {
        // Return Ok() instead of error - this is a no-op stub for host-driven mode
        // The Android BleService handles all actual BLE operations via JNI
        tracing::info!("🔧 Creating Android BLE adapter stub (host-driven mode)");
        Ok(AndroidBleAdapter {})
    }
}

#[async_trait]
impl BleAdapter for AndroidBleAdapter {
    async fn start_advertising(&self, _service_uuid: &str, _service_name: &str) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 start_advertising called on Android stub (no-op, host-driven mode)");
        Ok(())
    }

    async fn stop_advertising(&self) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 stop_advertising called on Android stub (no-op, host-driven mode)");
        Ok(())
    }

    async fn send_packet(&self, _data: &[u8]) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 send_packet called on Android stub (no-op, host-driven mode)");
        Ok(())
    }

    fn on_receive(&self, _callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 on_receive called on Android stub (no-op, host-driven mode)");
    }

    fn is_advertising(&self) -> bool {
        // Always return false - actual state is tracked by Android BleService
        false
    }

    async fn connected_clients_count(&self) -> usize {
        // Always return 0 - actual count is tracked by Android BleService
        0
    }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "Android".to_string(),
            name: "Android Bluetooth (Host-Driven)".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: true, // Assume powered since BleService manages this
            discoverable: false,
        }
    }

    async fn start_scanning(&self) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 start_scanning called on Android stub (no-op, host-driven mode)");
        Ok(())
    }

    async fn stop_scanning(&self) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 stop_scanning called on Android stub (no-op, host-driven mode)");
        Ok(())
    }

    async fn get_discovered_devices(&self) -> Result<Vec<super::adapter::DiscoveredDevice>, BleError> {
        // Return empty list - actual discovery is handled by Android BleService
        tracing::debug!("🔇 get_discovered_devices called on Android stub (no-op, host-driven mode)");
        Ok(Vec::new())
    }
    
    // Optional trait methods - provide default implementations for host-driven mode
    async fn connect_to_device(&self, _address: &str) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 connect_to_device called on Android stub (no-op, host-driven mode)");
        Ok(())
    }
    
    async fn write_to_device(&self, _address: &str, _data: &[u8]) -> Result<(), BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 write_to_device called on Android stub (no-op, host-driven mode)");
        Ok(())
    }
    
    async fn read_from_device(&self, _address: &str) -> Result<Vec<u8>, BleError> {
        // No-op: BLE operations are handled by Android BleService in host-driven mode
        tracing::debug!("🔇 read_from_device called on Android stub (no-op, host-driven mode)");
        Ok(Vec::new())
    }
}
