//! macOS BLE implementation using btleplug
//!
//! This module provides BLE Central role support on macOS using btleplug.
//!
//! **Current Status:**
//! - ✅ Scanning/Discovery: FULLY WORKING - can discover Linux PolliNet devices
//! - ✅ Connecting: FULLY WORKING - can connect to discovered devices  
//! - ❌ Advertising: NOT SUPPORTED - btleplug doesn't support Peripheral role
//!
//! **Limitation:** macOS devices can act as clients (discover Linux servers) but cannot
//! advertise themselves as GATT servers. For full bidirectional support, native CoreBluetooth
//! FFI would be needed (see macOS_Implementation_Notes.md for details).

use super::adapter::{AdapterInfo, BleAdapter, BleError, DiscoveredDevice, POLLINET_SERVICE_UUID};
use async_trait::async_trait;
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

/// macOS BLE adapter using btleplug (Central role only)
pub struct MacOSBleAdapter {
    /// BLE manager
    manager: Manager,
    /// Active adapter
    adapter: Arc<Mutex<Option<Adapter>>>,
    /// Discovered devices cache
    discovered_devices: Arc<Mutex<HashMap<String, DiscoveredDevice>>>,
    /// Connected peripherals
    connected_peripherals: Arc<Mutex<HashMap<String, Peripheral>>>,
    /// Scanning status
    is_scanning: Arc<Mutex<bool>>,
    /// Service UUID for PolliNet
    service_uuid: Uuid,
    /// Characteristic UUID for data transmission
    characteristic_uuid: Uuid,
    /// Receive callback
    receive_callback: Arc<Mutex<Option<Box<dyn Fn(Vec<u8>) + Send + 'static>>>>,
}

impl MacOSBleAdapter {
    /// Create a new macOS BLE adapter
    pub async fn new() -> Result<Self, BleError> {
        tracing::info!("🍎 Initializing macOS BLE adapter (btleplug - Central role only)");

        let manager = Manager::new()
            .await
            .map_err(|e| BleError::PlatformError(format!("Failed to create BLE manager: {}", e)))?;

        let service_uuid = Uuid::parse_str(POLLINET_SERVICE_UUID)
            .map_err(|e| BleError::InvalidUuid(format!("Invalid PolliNet service UUID: {}", e)))?;

        // Characteristic UUID for data transmission (same as guide suggested)
        let characteristic_uuid = Uuid::parse_str("7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7")
            .map_err(|e| BleError::InvalidUuid(format!("Invalid characteristic UUID: {}", e)))?;

        tracing::info!("✅ macOS BLE adapter initialized");
        tracing::info!("   Mode: Central only (scanning/connecting)");
        tracing::info!("   Can discover: Linux PolliNet devices ✅");
        tracing::info!("   Can advertise: Not supported ❌");

        Ok(Self {
            manager,
            adapter: Arc::new(Mutex::new(None)),
            discovered_devices: Arc::new(Mutex::new(HashMap::new())),
            connected_peripherals: Arc::new(Mutex::new(HashMap::new())),
            is_scanning: Arc::new(Mutex::new(false)),
            service_uuid,
            characteristic_uuid,
            receive_callback: Arc::new(Mutex::new(None)),
        })
    }

    /// Get or initialize the BLE adapter
    async fn get_adapter(&self) -> Result<Adapter, BleError> {
        // Check if adapter exists
        let adapter_opt = {
            let guard = self.adapter.lock().unwrap();
            guard.clone()
        };

        if let Some(adapter) = adapter_opt {
            return Ok(adapter);
        }

        // Initialize adapter
        let adapters = self
            .manager
            .adapters()
            .await
            .map_err(|e| BleError::PlatformError(format!("Failed to get adapters: {}", e)))?;

        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| BleError::AdapterNotAvailable)?;

        // Store and return
        {
            let mut guard = self.adapter.lock().unwrap();
            *guard = Some(adapter.clone());
        }

        tracing::info!("📡 BLE adapter initialized");
        Ok(adapter)
    }

    /// Update discovered devices from scan
    async fn update_discovered_devices(&self) -> Result<(), BleError> {
        let adapter = self.get_adapter().await?;
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| BleError::PlatformError(format!("Failed to get peripherals: {}", e)))?;

        let mut new_devices = HashMap::new();

        for peripheral in peripherals {
            let properties = peripheral
                .properties()
                .await
                .map_err(|e| BleError::PlatformError(format!("Failed to get properties: {}", e)))?;

            if let Some(props) = properties {
                // Check if device advertises PolliNet service
                let has_pollinet_service = props.services.contains(&self.service_uuid);

                if has_pollinet_service {
                    let address = peripheral.id().to_string();

                    tracing::info!("🎯 Found PolliNet device:");
                    tracing::info!("   Address: {}", address);
                    if let Some(ref name) = props.local_name {
                        tracing::info!("   Name: {}", name);
                    }
                    if let Some(rssi) = props.rssi {
                        tracing::info!("   RSSI: {} dBm", rssi);
                    }

                    let device = DiscoveredDevice {
                        address: address.clone(),
                        name: props.local_name,
                        service_uuids: props.services,
                        rssi: props.rssi,
                        last_seen: Instant::now(),
                    };

                    new_devices.insert(address, device);
                }
            }
        }

        // Update the cache after all awaits are done
        {
            let mut devices_guard = self.discovered_devices.lock().unwrap();
            devices_guard.extend(new_devices);
        }

        Ok(())
    }

    /// Connect to a discovered peripheral by address
    pub async fn connect_to_peripheral(&self, address: &str) -> Result<(), BleError> {
        tracing::info!("🔗 Connecting to peripheral: {}", address);

        let adapter = self.get_adapter().await?;
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| BleError::PlatformError(format!("Failed to get peripherals: {}", e)))?;

        // Find peripheral by address
        let peripheral = peripherals
            .iter()
            .find(|p| p.id().to_string() == address)
            .ok_or_else(|| BleError::PeripheralNotFound)?;

        // Connect to peripheral
        peripheral
            .connect()
            .await
            .map_err(|e| BleError::ConnectionFailed(format!("Failed to connect: {}", e)))?;

        tracing::info!("✅ Connected to {}", address);

        // Discover services and characteristics
        peripheral
            .discover_services()
            .await
            .map_err(|e| BleError::PlatformError(format!("Failed to discover services: {}", e)))?;

        tracing::info!("🔍 Discovering services...");

        // Find PolliNet service
        let services = peripheral.services();
        let pollinet_service = services
            .iter()
            .find(|s| s.uuid == self.service_uuid)
            .ok_or_else(|| BleError::ServiceNotFound)?;

        tracing::info!("✅ Found PolliNet service");

        // Find data characteristic
        let characteristic = pollinet_service
            .characteristics
            .iter()
            .find(|c| c.uuid == self.characteristic_uuid)
            .ok_or_else(|| BleError::CharacteristicNotFound)?;

        tracing::info!("✅ Found data characteristic");

        // Subscribe to notifications
        peripheral
            .subscribe(characteristic)
            .await
            .map_err(|e| BleError::PlatformError(format!("Failed to subscribe: {}", e)))?;

        tracing::info!("📥 Subscribed to notifications");

        // Store connected peripheral
        {
            let mut peripherals = self.connected_peripherals.lock().unwrap();
            peripherals.insert(address.to_string(), peripheral.clone());
        }

        // Start notification handler
        self.start_notification_handler(peripheral.clone()).await;

        tracing::info!("🎉 GATT session established with {}", address);
        Ok(())
    }

    /// Handle incoming notifications from a peripheral
    async fn start_notification_handler(&self, peripheral: Peripheral) {
        let receive_callback = self.receive_callback.clone();
        let characteristic_uuid = self.characteristic_uuid;

        tokio::spawn(async move {
            let mut notification_stream = peripheral.notifications().await.unwrap();

            while let Some(notification) = notification_stream.next().await {
                if notification.uuid == characteristic_uuid {
                    tracing::info!("📥 Received {} bytes via GATT", notification.value.len());

                    if let Some(ref callback) = *receive_callback.lock().unwrap() {
                        callback(notification.value);
                    }
                }
            }
        });
    }

    /// Write data to a connected peripheral
    pub async fn write_to_peripheral(&self, address: &str, data: &[u8]) -> Result<(), BleError> {
        // Get peripheral and characteristic without holding lock across await
        let (peripheral, characteristic) = {
            let peripherals = self.connected_peripherals.lock().unwrap();
            let peripheral = peripherals
                .get(address)
                .ok_or_else(|| BleError::PeripheralNotFound)?
                .clone();

            // Find the characteristic
            let services = peripheral.services();
            let pollinet_service = services
                .iter()
                .find(|s| s.uuid == self.service_uuid)
                .ok_or_else(|| BleError::ServiceNotFound)?;

            let characteristic = pollinet_service
                .characteristics
                .iter()
                .find(|c| c.uuid == self.characteristic_uuid)
                .ok_or_else(|| BleError::CharacteristicNotFound)?
                .clone();

            (peripheral, characteristic)
        };

        // Write data
        peripheral
            .write(&characteristic, data, WriteType::WithResponse)
            .await
            .map_err(|e| BleError::TransmissionFailed(format!("Write failed: {}", e)))?;

        tracing::debug!("📤 Wrote {} bytes to {}", data.len(), address);
        Ok(())
    }
}

#[async_trait]
impl BleAdapter for MacOSBleAdapter {
    async fn start_advertising(
        &self,
        service_uuid: &str,
        service_name: &str,
    ) -> Result<(), BleError> {
        tracing::warn!("⚠️  BLE advertising not supported on macOS (btleplug limitation)");
        tracing::info!("   Service UUID: {}", service_uuid);
        tracing::info!("   Service Name: {}", service_name);
        tracing::info!("   ");
        tracing::info!("   📝 Note: btleplug only supports Central role (scanning/connecting)");
        tracing::info!("   For advertising support, native CoreBluetooth FFI would be needed");
        tracing::info!("   ");
        tracing::info!("   ✅ This macOS device CAN discover Linux PolliNet devices");
        tracing::info!("   ❌ Linux devices CANNOT discover this macOS device");

        Ok(())
    }

    async fn stop_advertising(&self) -> Result<(), BleError> {
        tracing::debug!("Stop advertising called (no-op on macOS)");
        Ok(())
    }

    async fn send_packet(&self, data: &[u8]) -> Result<(), BleError> {
        tracing::debug!(
            "📤 Send packet called ({} bytes) - not supported in Central-only mode",
            data.len()
        );
        Ok(())
    }

    fn on_receive(&self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        tracing::info!("📥 Setting up receive callback");
        *self.receive_callback.lock().unwrap() = Some(callback);
    }

    fn is_advertising(&self) -> bool {
        false // Central role cannot advertise
    }

    async fn connected_clients_count(&self) -> usize {
        self.connected_peripherals.lock().unwrap().len()
    }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "macOS".to_string(),
            name: "btleplug (Central Mode)".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: true,
            discoverable: false, // Cannot advertise in Central mode
        }
    }

    async fn start_scanning(&self) -> Result<(), BleError> {
        tracing::info!("🔍 Starting BLE scanning on macOS");
        tracing::info!("   Looking for PolliNet service: {}", self.service_uuid);

        let adapter = self.get_adapter().await?;

        // Create scan filter for PolliNet service
        let filter = ScanFilter {
            services: vec![self.service_uuid],
        };

        adapter
            .start_scan(filter)
            .await
            .map_err(|e| BleError::ScanningFailed(format!("Failed to start scan: {}", e)))?;

        *self.is_scanning.lock().unwrap() = true;

        tracing::info!("✅ BLE scanning started successfully");
        tracing::info!("   Filtering for PolliNet UUID: {}", self.service_uuid);
        tracing::info!("   Will discover Linux PolliNet devices advertising this service");

        // Give scan a moment to populate
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Update discovered devices
        self.update_discovered_devices().await?;

        Ok(())
    }

    async fn stop_scanning(&self) -> Result<(), BleError> {
        tracing::info!("🛑 Stopping BLE scanning on macOS");

        // Get adapter reference without holding lock across await
        let adapter_opt = {
            let guard = self.adapter.lock().unwrap();
            guard.clone()
        };

        if let Some(adapter) = adapter_opt {
            adapter
                .stop_scan()
                .await
                .map_err(|e| BleError::ScanningFailed(format!("Failed to stop scan: {}", e)))?;
        }

        *self.is_scanning.lock().unwrap() = false;
        tracing::info!("✅ BLE scanning stopped");
        Ok(())
    }

    async fn get_discovered_devices(&self) -> Result<Vec<DiscoveredDevice>, BleError> {
        // Update devices from latest scan
        let is_scanning = *self.is_scanning.lock().unwrap();
        if is_scanning {
            self.update_discovered_devices().await?;
        }

        let devices = {
            let devices_guard = self.discovered_devices.lock().unwrap();
            devices_guard.values().cloned().collect::<Vec<_>>()
        };

        tracing::info!("📱 Discovered {} PolliNet devices on macOS", devices.len());
        for device in &devices {
            tracing::info!(
                "   - {} ({})",
                device.name.as_ref().unwrap_or(&"Unknown".to_string()),
                device.address
            );
        }

        Ok(devices)
    }

    async fn connect_to_device(&self, address: &str) -> Result<(), BleError> {
        self.connect_to_peripheral(address).await
    }

    async fn write_to_device(&self, address: &str, data: &[u8]) -> Result<(), BleError> {
        self.write_to_peripheral(address, data).await
    }
}
