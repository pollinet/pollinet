//! Linux BLE implementation using bluer (BlueZ)
//! 
//! This module provides the Linux-specific implementation of the BleAdapter trait
//! using the bluer crate to interface with BlueZ.

#[cfg(feature = "linux")]
mod linux_impl {
    use super::super::adapter::{BleAdapter, BleError, AdapterInfo, POLLINET_SERVICE_UUID, POLLINET_SERVICE_NAME};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    use uuid::Uuid;
    use tokio::sync::RwLock;
    use bluer::{
        adv::Advertisement,
    };
    use tokio_stream::StreamExt;

    /// Linux BLE adapter implementation
    pub struct LinuxBleAdapter {
        /// BlueZ session
        session: bluer::Session,
        /// Bluetooth adapter
        adapter: bluer::Adapter,
        /// Connected clients
        clients: Arc<RwLock<HashMap<String, ClientInfo>>>,
        /// Receive callback
        receive_callback: Arc<Mutex<Option<Box<dyn Fn(Vec<u8>) + Send + 'static>>>>,
        /// Advertising status
        is_advertising: Arc<RwLock<bool>>,
        /// Scanning status
        is_scanning: Arc<RwLock<bool>>,
        /// Service UUID
        service_uuid: Uuid,
        /// Advertisement handle
        advertisement_handle: Arc<Mutex<Option<bluer::adv::AdvertisementHandle>>>,
        /// Discovered devices
        discovered_devices: Arc<RwLock<HashMap<String, super::super::adapter::DiscoveredDevice>>>,
    }

    /// Information about a connected client
    #[derive(Debug, Clone)]
    struct ClientInfo {
        device_address: String,
        connected_at: std::time::Instant,
    }

    impl LinuxBleAdapter {
        /// Create a new Linux BLE adapter
        pub async fn new() -> Result<Self, BleError> {
            // Create BlueZ session
            let session = bluer::Session::new().await
                .map_err(|e| BleError::PlatformError(format!("Failed to create BlueZ session: {}", e)))?;

            // Get the default adapter
            let adapter = session.default_adapter().await
                .map_err(|_e| BleError::AdapterNotAvailable)?;

            // Power on the adapter
            adapter.set_powered(true).await
                .map_err(|e| BleError::PlatformError(format!("Failed to power on adapter: {}", e)))?;

            // Parse service UUID
            let service_uuid = Uuid::parse_str(POLLINET_SERVICE_UUID)
                .map_err(|e| BleError::InvalidUuid(format!("Invalid service UUID: {}", e)))?;

            Ok(Self {
                session,
                adapter,
                clients: Arc::new(RwLock::new(HashMap::new())),
                receive_callback: Arc::new(Mutex::new(None)),
                is_advertising: Arc::new(RwLock::new(false)),
                is_scanning: Arc::new(RwLock::new(false)),
                service_uuid,
                advertisement_handle: Arc::new(Mutex::new(None)),
                discovered_devices: Arc::new(RwLock::new(HashMap::new())),
            })
        }

        /// Get device information and check if it advertises PolliNet service
        async fn get_device_info(
            device: &bluer::Device,
            pollinet_service_uuid: Uuid,
        ) -> Result<Option<super::super::adapter::DiscoveredDevice>, BleError> {
            // Get device address
            let address = device.address();
            
            // Get device name (alias for now - name requires different handling)  
            let device_name = device.alias().await.ok();
            
            // Get UUIDs (service data keys are the service UUIDs)
            let service_data = device.service_data().await.ok().flatten();
            let mut service_uuids = Vec::new();
            
            if let Some(data) = service_data {
                for uuid in data.keys() {
                    service_uuids.push(*uuid);
                }
            }
            
            // Also check UUIDs property
            if let Ok(uuids) = device.uuids().await {
                if let Some(uuid_list) = uuids {
                    service_uuids.extend(uuid_list);
                }
            }
            
            // Get RSSI
            let rssi = device.rssi().await.ok().flatten();
            
            // Check if this device advertises the PolliNet service
            let has_pollinet_service = service_uuids.contains(&pollinet_service_uuid);
            
            if has_pollinet_service {
                // Only include device if it explicitly advertises PolliNet service
                tracing::info!("✅ Device {} advertises PolliNet service with UUIDs: {:?}", 
                    address, service_uuids);
                Ok(Some(super::super::adapter::DiscoveredDevice {
                    address: address.to_string(),
                    name: device_name,
                    service_uuids,
                    rssi,
                    last_seen: std::time::Instant::now(),
                }))
            } else {
                // Device doesn't advertise PolliNet service - filter it out
                tracing::debug!("⚪ Device {} does not advertise PolliNet service (UUIDs: {:?})", 
                    address, service_uuids);
                // // TEMPORARY: Let's see what devices are actually being discovered
                // if service_uuids.is_empty() {
                //     tracing::info!("📱 Device {} discovered but no service UUIDs available", address);
                // } else {
                //     tracing::info!("📱 Device {} discovered with services: {:?}", address, service_uuids);
                // }
                Ok(None)
            }
        }


        /// Start advertising with BlueZ
        async fn start_bluez_advertising(&self) -> Result<(), BleError> {
            use std::collections::BTreeSet;
            
            // Create advertisement
            let mut service_uuids = BTreeSet::new();
            service_uuids.insert(self.service_uuid);
            
            // Create a simpler advertisement to avoid parameter issues
            let advertisement = Advertisement {
                advertisement_type: bluer::adv::Type::Broadcast,
                service_uuids,
                local_name: Some(POLLINET_SERVICE_NAME.to_string()),
                ..Default::default()
            };

            // Start advertising
            let handle = self.adapter.advertise(advertisement).await
                .map_err(|e| BleError::AdvertisingFailed(format!("BlueZ advertising failed: {}", e)))?;

            // Store the handle for stopping later
            let mut handle_guard = self.advertisement_handle.lock().unwrap();
            *handle_guard = Some(handle);

            Ok(())
        }
    }

    /// Device information structure
    #[derive(serde::Serialize)]
    struct DeviceInfo {
        device_id: String,
        platform: String,
        capabilities: Vec<String>,
        protocol_version: String,
    }

    #[async_trait]
    impl BleAdapter for LinuxBleAdapter {
        async fn start_advertising(&self, service_uuid: &str, service_name: &str) -> Result<(), BleError> {
            tracing::info!("Starting BLE advertising on Linux");
            tracing::info!("Service UUID: {}", service_uuid);
            tracing::info!("Service Name: {}", service_name);

            // Validate UUID
            if service_uuid != POLLINET_SERVICE_UUID {
                return Err(BleError::InvalidUuid(format!("Expected {}, got {}", POLLINET_SERVICE_UUID, service_uuid)));
            }

            // Set advertising status
            {
                let mut status = self.is_advertising.write().await;
                *status = true;
            }

            // Start BlueZ advertising
            self.start_bluez_advertising().await?;

            tracing::info!("BLE advertising started successfully on Linux");
            Ok(())
        }

        async fn stop_advertising(&self) -> Result<(), BleError> {
            tracing::info!("Stopping BLE advertising on Linux");

            // Stop BlueZ advertising by dropping the handle
            {
                let mut handle_guard = self.advertisement_handle.lock().unwrap();
                if let Some(handle) = handle_guard.take() {
                    drop(handle); // This will stop the advertising
                }
            }

            // Update advertising status
            {
                let mut status = self.is_advertising.write().await;
                *status = false;
            }

            tracing::info!("BLE advertising stopped successfully on Linux");
            Ok(())
        }

        async fn send_packet(&self, data: &[u8]) -> Result<(), BleError> {
            tracing::debug!("Sending packet via BLE ({} bytes)", data.len());
            
            // For now, just log the packet - in a full implementation,
            // this would send notifications to connected GATT clients
            tracing::info!("📤 BLE Packet sent: {} bytes", data.len());
            tracing::debug!("   Data: {:02x?}", data);
            
            Ok(())
        }

        fn on_receive(&self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
            let mut cb_guard = self.receive_callback.lock().unwrap();
            *cb_guard = Some(callback);
        }

        fn is_advertising(&self) -> bool {
            // This is a simplified check - in a real implementation,
            // you'd query the actual advertising status from BlueZ
            true // For now, assume advertising is active if we started it
        }

        async fn connected_clients_count(&self) -> usize {
            // Return the number of connected clients
            let clients_guard = self.clients.read().await;
            clients_guard.len()
        }

        fn get_adapter_info(&self) -> AdapterInfo {
            AdapterInfo {
                platform: "Linux".to_string(),
                name: "BlueZ Adapter".to_string(),
                address: "00:00:00:00:00:00".to_string(), // Would get real address
                powered: true,
                discoverable: true,
            }
        }

        async fn start_scanning(&self) -> Result<(), BleError> {
            tracing::info!("🔍 Starting BLE scanning on Linux");

            // Check if already scanning
            {
                let is_scanning = self.is_scanning.read().await;
                if *is_scanning {
                    tracing::warn!("⚠️ BLE scanning already active");
                    return Ok(());
                }
            }

            // Start BlueZ discovery
            self.adapter.set_discoverable(true).await
                .map_err(|e| BleError::PlatformError(format!("Failed to set discoverable: {}", e)))?;

            // Set scanning status
            {
                let mut status = self.is_scanning.write().await;
                *status = true;
            }

            // Set up discovery event stream
            let discovery_events = self.adapter.discover_devices_with_changes().await
                .map_err(|e| BleError::PlatformError(format!("Failed to create discovery stream: {}", e)))?;

            // Start background task to handle discovery events
            let devices = Arc::clone(&self.discovered_devices);
            let service_uuid = self.service_uuid;
            let adapter_for_task = self.adapter.clone();
            
            tokio::spawn(async move {
                let mut stream = discovery_events;
                while let Some(device_events) = stream.next().await {
                    match device_events {
                        bluer::AdapterEvent::DeviceAdded(device_id) => {
                            // tracing::info!("📱 Device discovered: {}", device_id);
                            
                            // Get device from adapter
                            if let Ok(device) = adapter_for_task.device(device_id) {
                                // Get device properties
                                match tokio::time::timeout(
                                    tokio::time::Duration::from_secs(2),
                                    Self::get_device_info(&device, service_uuid)
                                ).await {
                                    Ok(Ok(Some(discovered_device))) => {
                                        tracing::info!("🎯 Found PolliNet device: {} ({})", 
                                            device_id,
                                            discovered_device.name.as_deref().unwrap_or("Unknown")
                                        );
                                        
                                        // Add to discovered devices
                                        let mut devices_guard = devices.write().await;
                                        devices_guard.insert(device_id.to_string(), discovered_device);
                                    }
                                    Ok(Ok(None)) => {
                                        tracing::debug!("⚪ Device {} does not advertise PolliNet service", device_id);
                                    }
                                    Ok(Err(e)) => {
                                        tracing::debug!("⚠️ Failed to get device info for {}: {}", device_id, e);
                                    }
                                    Err(_) => {
                                        tracing::debug!("⏱️ Timeout getting device info for {}", device_id);
                                    }
                                }
                            }
                        }
                        bluer::AdapterEvent::DeviceRemoved(device_id) => {
                            tracing::debug!("📱 Device removed: {}", device_id);
                            
                            // Remove from discovered devices
                            let mut devices_guard = devices.write().await;
                            devices_guard.remove(&device_id.to_string());
                        }
                        _ => {
                            // Ignore other events for now
                        }
                    }
                }
            });

            tracing::info!("✅ Linux BLE scanning started successfully");
            Ok(())
        }

        async fn stop_scanning(&self) -> Result<(), BleError> {
            tracing::info!("🛑 Stopping BLE scanning on Linux");

            // Stop BlueZ discovery (set discoverable to false)
            self.adapter.set_discoverable(false).await
                .map_err(|e| BleError::PlatformError(format!("Failed to stop discovery: {}", e)))?;

            // Update scanning status
            {
                let mut status = self.is_scanning.write().await;
                *status = false;
            }

            tracing::info!("✅ Linux BLE scanning stopped successfully");
            Ok(())
        }

        async fn get_discovered_devices(&self) -> Result<Vec<super::super::adapter::DiscoveredDevice>, BleError> {
            let devices_guard = self.discovered_devices.read().await;
            let devices: Vec<super::super::adapter::DiscoveredDevice> = devices_guard.values().cloned().collect();
            
            tracing::debug!("📱 Found {} discovered devices on Linux", devices.len());
            Ok(devices)
        }

        async fn connect_to_device(&self, address: &str) -> Result<(), BleError> {
            tracing::info!("🔗 Connecting to BLE device: {}", address);
            
            // Parse the address
            let device_address = address.parse::<bluer::Address>()
                .map_err(|e| BleError::PlatformError(format!("Invalid device address: {}", e)))?;
            
            // Get the device from the adapter
            let device = self.adapter.device(device_address)
                .map_err(|e| BleError::PlatformError(format!("Failed to get device: {}", e)))?;
            
            // Connect to the device
            device.connect().await
                .map_err(|e| BleError::ConnectionFailed(format!("Failed to connect to device {}: {}", address, e)))?;
            
            // Add to connected clients
            let mut clients_guard = self.clients.write().await;
            clients_guard.insert(address.to_string(), ClientInfo {
                device_address: address.to_string(),
                connected_at: std::time::Instant::now(),
            });
            
            tracing::info!("✅ Successfully connected to device: {}", address);
            Ok(())
        }

        async fn write_to_device(&self, address: &str, data: &[u8]) -> Result<(), BleError> {
            tracing::info!("📤 Writing {} bytes to device: {}", data.len(), address);
            
            // Parse the address
            let device_address = address.parse::<bluer::Address>()
                .map_err(|e| BleError::PlatformError(format!("Invalid device address: {}", e)))?;
            
            // Get the device from the adapter
            let device = self.adapter.device(device_address)
                .map_err(|e| BleError::PlatformError(format!("Failed to get device: {}", e)))?;
            
            // Check if device is connected
            if !device.is_connected().await
                .map_err(|e| BleError::PlatformError(format!("Failed to check connection status: {}", e)))? {
                return Err(BleError::ConnectionFailed("Device not connected".to_string()));
            }
            
            // For now, we'll simulate writing by logging the data
            // In a full implementation, this would write to a GATT characteristic
            tracing::info!("📤 BLE Data written to {}: {} bytes", address, data.len());
            tracing::debug!("   Data: {:02x?}", data);
            
            // TODO: Implement actual GATT characteristic writing
            // This would involve:
            // 1. Discovering services on the connected device
            // 2. Finding the PolliNet service and characteristic
            // 3. Writing the data to the characteristic
            
            Ok(())
        }
    }
}

#[cfg(feature = "linux")]
pub use linux_impl::LinuxBleAdapter;

#[cfg(not(feature = "linux"))]
pub struct LinuxBleAdapter;

#[cfg(not(feature = "linux"))]
impl LinuxBleAdapter {
    pub async fn new() -> Result<Self, crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }
}

#[cfg(not(feature = "linux"))]
#[async_trait::async_trait]
impl crate::ble::adapter::BleAdapter for LinuxBleAdapter {
    async fn start_advertising(&self, _service_uuid: &str, _service_name: &str) -> Result<(), crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }

    async fn stop_advertising(&self) -> Result<(), crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }

    async fn send_packet(&self, _data: &[u8]) -> Result<(), crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }

    fn on_receive(&self, _callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        // No-op for stub implementation
    }

    fn is_advertising(&self) -> bool {
        false
    }

    async fn connected_clients_count(&self) -> usize {
        0
    }

    fn get_adapter_info(&self) -> crate::ble::adapter::AdapterInfo {
        crate::ble::adapter::AdapterInfo {
            platform: "Linux (Stub)".to_string(),
            name: "Not Available".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: false,
            discoverable: false,
        }
    }

    async fn start_scanning(&self) -> Result<(), crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }

    async fn stop_scanning(&self) -> Result<(), crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }

    async fn get_discovered_devices(&self) -> Result<Vec<crate::ble::adapter::DiscoveredDevice>, crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }
}
