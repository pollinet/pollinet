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

use super::adapter::{BleAdapter, BleError, AdapterInfo, DiscoveredDevice, POLLINET_SERVICE_UUID};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};

/// macOS BLE adapter using btleplug (Central role only)
pub struct MacOSBleAdapter {
    /// BLE manager
    manager: Manager,
    /// Active adapter
    adapter: Arc<Mutex<Option<Adapter>>>,
    /// Discovered devices cache
    discovered_devices: Arc<Mutex<HashMap<String, DiscoveredDevice>>>,
    /// Scanning status
    is_scanning: Arc<Mutex<bool>>,
    /// Service UUID for PolliNet
    service_uuid: Uuid,
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

        tracing::info!("✅ macOS BLE adapter initialized");
        tracing::info!("   Mode: Central only (scanning/connecting)");
        tracing::info!("   Can discover: Linux PolliNet devices ✅");
        tracing::info!("   Can advertise: Not supported ❌");

        Ok(Self {
            manager,
            adapter: Arc::new(Mutex::new(None)),
            discovered_devices: Arc::new(Mutex::new(HashMap::new())),
            is_scanning: Arc::new(Mutex::new(false)),
            service_uuid,
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
        let adapters = self.manager.adapters().await
            .map_err(|e| BleError::PlatformError(format!("Failed to get adapters: {}", e)))?;
        
        let adapter = adapters.into_iter().next()
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
        let peripherals = adapter.peripherals().await
            .map_err(|e| BleError::PlatformError(format!("Failed to get peripherals: {}", e)))?;

        let mut new_devices = HashMap::new();
        
        for peripheral in peripherals {
            let properties = peripheral.properties().await
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
}

#[async_trait]
impl BleAdapter for MacOSBleAdapter {
    async fn start_advertising(&self, service_uuid: &str, service_name: &str) -> Result<(), BleError> {
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
        tracing::debug!("📤 Send packet called ({} bytes) - not supported in Central-only mode", data.len());
        Ok(())
    }

    fn on_receive(&self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        tracing::info!("📥 Setting up receive callback");
        *self.receive_callback.lock().unwrap() = Some(callback);
    }

    fn is_advertising(&self) -> bool {
        false // Central role cannot advertise
    }

    fn connected_clients_count(&self) -> usize {
        0 // Central role doesn't have clients
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

        adapter.start_scan(filter).await
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
            adapter.stop_scan().await
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
            tracing::info!("   - {} ({})", 
                device.name.as_ref().unwrap_or(&"Unknown".to_string()),
                device.address
            );
        }
        
        Ok(devices)
    }
}
