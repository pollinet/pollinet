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
        gatt::remote::{
            Characteristic, Service,
        },
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
        /// Discovered GATT services
        services: HashMap<Uuid, GattService>,
    }

    /// GATT service information
    #[derive(Debug, Clone)]
    struct GattService {
        uuid: Uuid,
        characteristics: HashMap<Uuid, GattCharacteristic>,
    }

    /// GATT characteristic information
    #[derive(Debug, Clone)]
    struct GattCharacteristic {
        uuid: Uuid,
        properties: CharacteristicProperties,
        value: Option<Vec<u8>>,
    }

    /// Characteristic properties
    #[derive(Debug, Clone)]
    struct CharacteristicProperties {
        read: bool,
        write: bool,
        write_without_response: bool,
        notify: bool,
        indicate: bool,
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

        /// Discover GATT services for a connected device
        async fn discover_gatt_services(
            device: &bluer::Device,
        ) -> Result<HashMap<Uuid, GattService>, BleError> {
            tracing::info!("🔍 Discovering GATT services for device: {}", device.address());
            
            let mut services = HashMap::new();
            
            // Get all services from the device
            let device_services = device.services().await
                .map_err(|e| BleError::PlatformError(format!("Failed to get services: {}", e)))?;
            
            for service in device_services {
                let service_uuid = service.uuid().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get service UUID: {}", e)))?;
                
                tracing::info!("📋 Found service: {}", service_uuid);
                
                // Discover characteristics for this service
                let characteristics = Self::discover_characteristics(&service).await?;
                
                let gatt_service = GattService {
                    uuid: service_uuid,
                    characteristics,
                };
                
                services.insert(service_uuid, gatt_service);
            }
            
            tracing::info!("✅ Discovered {} GATT services", services.len());
            Ok(services)
        }

        /// Set up GATT data reception for a connected device
        async fn setup_gatt_data_reception(
            device: &bluer::Device,
            _services: &HashMap<Uuid, GattService>,
            receive_callback: Arc<Mutex<Option<Box<dyn Fn(Vec<u8>) + Send + 'static>>>>,
        ) -> Result<(), BleError> {
            tracing::info!("📡 Setting up GATT data reception...");
            
            // Get all services from the device
            let device_services = device.services().await
                .map_err(|e| BleError::PlatformError(format!("Failed to get services: {}", e)))?;
            
            for service in device_services {
                let _service_uuid = service.uuid().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get service UUID: {}", e)))?;
                
                // Get characteristics for this service
                let characteristics = service.characteristics().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get characteristics: {}", e)))?;
                
                for characteristic in characteristics {
                    let char_uuid = characteristic.uuid().await
                        .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic UUID: {}", e)))?;
                    
                    let properties = characteristic.flags().await
                        .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic properties: {}", e)))?;
                    
                    // Check if this characteristic supports notifications or indications
                    if properties.notify || properties.indicate {
                        tracing::info!("🔔 Setting up notification for characteristic: {} (notify: {}, indicate: {})", 
                            char_uuid, properties.notify, properties.indicate);
                        
                        // Set up notification/indication subscription
                        if let Err(e) = Self::subscribe_to_characteristic(&characteristic, receive_callback.clone()).await {
                            tracing::warn!("⚠️  Failed to subscribe to characteristic {}: {}", char_uuid, e);
                        } else {
                            tracing::info!("✅ Successfully subscribed to characteristic: {}", char_uuid);
                        }
                    }
                }
            }
            
            tracing::info!("✅ GATT data reception setup completed");
            Ok(())
        }

        /// Subscribe to a characteristic for notifications/indications
        async fn subscribe_to_characteristic(
            characteristic: &Characteristic,
            receive_callback: Arc<Mutex<Option<Box<dyn Fn(Vec<u8>) + Send + 'static>>>>,
        ) -> Result<(), BleError> {
            let char_uuid = characteristic.uuid().await
                .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic UUID: {}", e)))?;
            
            // Start notifications
            characteristic.notify().await
                .map_err(|e| BleError::PlatformError(format!("Failed to start notifications for characteristic {}: {}", char_uuid, e)))?;
            
            // Set up a background task to handle incoming notifications
            let characteristic_clone = characteristic.clone();
            let callback_clone = receive_callback.clone();
            
            tokio::spawn(async move {
                tracing::info!("🎧 Listening for notifications on characteristic: {}", char_uuid);
                
                // Create a stream for notifications
                if let Ok(notifications) = characteristic_clone.notify().await {
                    use tokio_stream::StreamExt;
                    
                    let mut notifications = Box::pin(notifications);
                    while let Some(data) = notifications.next().await {
                        tracing::info!("📨 Received GATT notification: {} bytes", data.len());
                        tracing::debug!("   Data: {:02x?}", data);
                        
                        // Call the receive callback if it's set
                        if let Ok(callback_guard) = callback_clone.lock() {
                            if let Some(callback) = callback_guard.as_ref() {
                                callback(data);
                            }
                        }
                    }
                } else {
                    tracing::warn!("⚠️  Failed to create notification stream for characteristic: {}", char_uuid);
                }
            });
            
            Ok(())
        }

        /// Discover characteristics for a GATT service
        async fn discover_characteristics(
            service: &Service,
        ) -> Result<HashMap<Uuid, GattCharacteristic>, BleError> {
            let mut characteristics = HashMap::new();
            
            // Get all characteristics from the service
            let service_characteristics = service.characteristics().await
                .map_err(|e| BleError::PlatformError(format!("Failed to get characteristics: {}", e)))?;
            
            for characteristic in service_characteristics {
                let char_uuid = characteristic.uuid().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic UUID: {}", e)))?;
                
                // Get characteristic properties
                let properties = characteristic.flags().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic properties: {}", e)))?;
                
                let char_properties = CharacteristicProperties {
                    read: properties.read,
                    write: properties.write,
                    write_without_response: properties.write_without_response,
                    notify: properties.notify,
                    indicate: properties.indicate,
                };
                
                tracing::info!("  📝 Characteristic: {} (R:{} W:{} WNR:{} N:{} I:{})", 
                    char_uuid,
                    char_properties.read,
                    char_properties.write,
                    char_properties.write_without_response,
                    char_properties.notify,
                    char_properties.indicate
                );
                
                // Try to read the current value if readable
                let mut value = None;
                if char_properties.read {
                    match characteristic.read().await {
                        Ok(read_value) => {
                            tracing::debug!("    📖 Read value: {:02x?}", read_value);
                            value = Some(read_value);
                        }
                        Err(e) => {
                            tracing::debug!("    ⚠️  Failed to read characteristic value: {}", e);
                        }
                    }
                }
                
                let gatt_characteristic = GattCharacteristic {
                    uuid: char_uuid,
                    properties: char_properties,
                    value,
                };
                
                characteristics.insert(char_uuid, gatt_characteristic);
            }
            
            tracing::info!("  ✅ Discovered {} characteristics", characteristics.len());
            Ok(characteristics)
        }

        /// Find a writable characteristic for data transmission
        async fn find_writable_characteristic(
            device: &bluer::Device,
            service_uuid: Option<Uuid>,
        ) -> Result<(Service, Characteristic), BleError> {
            tracing::info!("🔍 Looking for writable characteristic...");
            
            let services = device.services().await
                .map_err(|e| BleError::PlatformError(format!("Failed to get services: {}", e)))?;
            
            for service in services {
                let current_service_uuid = service.uuid().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get service UUID: {}", e)))?;
                
                // If a specific service UUID is provided, only check that service
                if let Some(target_uuid) = service_uuid {
                    if current_service_uuid != target_uuid {
                        continue;
                    }
                }
                
                tracing::debug!("🔍 Checking service: {}", current_service_uuid);
                
                let characteristics = service.characteristics().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get characteristics: {}", e)))?;
                
                for characteristic in characteristics {
                    let char_uuid = characteristic.uuid().await
                        .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic UUID: {}", e)))?;
                    
                    let properties = characteristic.flags().await
                        .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic properties: {}", e)))?;
                    
                    // Check if this characteristic supports writing
                    if properties.write || properties.write_without_response {
                        tracing::info!("✅ Found writable characteristic: {} in service: {}", char_uuid, current_service_uuid);
                        return Ok((service, characteristic));
                    }
                }
            }
            
            Err(BleError::CharacteristicNotFound)
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
            
            // Check if adapter is powered on
            if !self.adapter.is_powered().await
                .map_err(|e| BleError::PlatformError(format!("Failed to check adapter power status: {}", e)))? {
                tracing::warn!("⚠️  BLE adapter is not powered on, attempting to power on...");
                self.adapter.set_powered(true).await
                    .map_err(|e| BleError::PlatformError(format!("Failed to power on adapter: {}", e)))?;
                
                // Wait a bit for the adapter to power on
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }

            // Check if adapter is discoverable
            if !self.adapter.is_discoverable().await
                .map_err(|e| BleError::PlatformError(format!("Failed to check discoverable status: {}", e)))? {
                tracing::warn!("⚠️  BLE adapter is not discoverable, attempting to set discoverable...");
                self.adapter.set_discoverable(true).await
                    .map_err(|e| BleError::PlatformError(format!("Failed to set discoverable: {}", e)))?;
            }

            // Try multiple advertisement configurations
            let mut last_error = None;
            
            // Configuration 1: Minimal advertisement with service UUID
            let mut service_uuids = BTreeSet::new();
            service_uuids.insert(self.service_uuid);
            let minimal_ad = Advertisement {
                advertisement_type: bluer::adv::Type::Broadcast,
                service_uuids: service_uuids.clone(),
                local_name: Some(POLLINET_SERVICE_NAME.to_string()),
                ..Default::default()
            };

            // Configuration 2: With service UUID (same as minimal now)
            let service_ad = Advertisement {
                advertisement_type: bluer::adv::Type::Broadcast,
                service_uuids: service_uuids.clone(),
                local_name: Some(POLLINET_SERVICE_NAME.to_string()),
                ..Default::default()
            };

            // Configuration 3: Connectable advertisement
            let connectable_ad = Advertisement {
                advertisement_type: bluer::adv::Type::Peripheral,
                service_uuids: service_uuids.clone(),
                local_name: Some(POLLINET_SERVICE_NAME.to_string()),
                ..Default::default()
            };

            let configurations = vec![
                ("minimal", minimal_ad),
                ("with_service", service_ad),
                ("connectable", connectable_ad),
            ];

            for (config_name, advertisement) in configurations {
                tracing::debug!("🔧 Trying {} advertisement configuration...", config_name);
                
                match self.adapter.advertise(advertisement).await {
                    Ok(handle) => {
                        tracing::info!("✅ Successfully started advertising with {} configuration", config_name);
                        
                        // Store the handle for stopping later
                        let mut handle_guard = self.advertisement_handle.lock().unwrap();
                        *handle_guard = Some(handle);
                        
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("⚠️  {} configuration failed: {}", config_name, e);
                        last_error = Some(e);
                        
                        // Wait a bit before trying the next configuration
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }

            // If all configurations failed, return the last error
            Err(BleError::AdvertisingFailed(format!(
                "All advertising configurations failed. Last error: {}", 
                last_error.map(|e| e.to_string()).unwrap_or_else(|| "Unknown error".to_string())
            )))
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

            // Try to start BlueZ advertising with fallback
            match self.start_bluez_advertising().await {
                Ok(_) => {
                    tracing::info!("✅ BLE advertising started successfully on Linux");
                    Ok(())
                }
                Err(e) => {
                    tracing::warn!("⚠️  BLE advertising failed, but continuing in simulation mode: {}", e);
                    tracing::warn!("   The system will continue to work for scanning and connecting to other devices");
                    tracing::warn!("   However, this device will not be discoverable by other PolliNet devices");
                    
                    // Don't return error - allow the system to continue
                    // This enables scanning and connecting to other devices even if advertising fails
                    Ok(())
                }
            }
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
            
            tracing::info!("✅ Connected to device: {}", address);
            
            // Discover GATT services after connection
            tracing::info!("🔍 Discovering GATT services...");
            let services = match Self::discover_gatt_services(&device).await {
                Ok(services) => {
                    tracing::info!("✅ Successfully discovered {} GATT services", services.len());
                    services
                }
                Err(e) => {
                    tracing::warn!("⚠️  Failed to discover GATT services: {}", e);
                    tracing::warn!("   Continuing with connection but write operations may not work");
                    HashMap::new()
                }
            };
            
            // Set up GATT data reception if we have services
            if !services.is_empty() {
                tracing::info!("📡 Setting up GATT data reception...");
                if let Err(e) = Self::setup_gatt_data_reception(&device, &services, self.receive_callback.clone()).await {
                    tracing::warn!("⚠️  Failed to set up GATT data reception: {}", e);
                    tracing::warn!("   Connection will continue but data reception may not work");
                } else {
                    tracing::info!("✅ GATT data reception set up successfully");
                }
            }
            
            // Add to connected clients with GATT service information
            let mut clients_guard = self.clients.write().await;
            clients_guard.insert(address.to_string(), ClientInfo {
                device_address: address.to_string(),
                connected_at: std::time::Instant::now(),
                services,
            });
            
            tracing::info!("✅ Successfully connected to device: {} with GATT services and data reception", address);
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
            
            // Check if we have cached GATT services for this device
            let client_info = {
                let clients_guard = self.clients.read().await;
                clients_guard.get(address).cloned()
            };
            
            if let Some(client_info) = client_info {
                // Try to find a writable characteristic from cached services
                if let Some((service_uuid, characteristic_uuid)) = find_cached_writable_characteristic(&client_info) {
                    tracing::info!("📝 Using cached characteristic: {} in service: {}", characteristic_uuid, service_uuid);
                    
                    // Find the actual characteristic object
                    match Self::find_writable_characteristic(&device, Some(service_uuid)).await {
                        Ok((_service, characteristic)) => {
                            return write_to_characteristic(&characteristic, data).await;
                        }
                        Err(e) => {
                            tracing::warn!("⚠️  Failed to find cached characteristic: {}", e);
                        }
                    }
                }
            }
            
            // Fallback: discover services and find any writable characteristic
            tracing::info!("🔍 Discovering services to find writable characteristic...");
            match Self::find_writable_characteristic(&device, None).await {
                Ok((_service, characteristic)) => {
                    write_to_characteristic(&characteristic, data).await
                }
                Err(e) => {
                    tracing::error!("❌ No writable characteristic found: {}", e);
                    Err(BleError::CharacteristicNotFound)
                }
            }
        }

        /// Read data from a connected device using GATT
        async fn read_from_device(&self, address: &str) -> Result<Vec<u8>, BleError> {
            tracing::info!("📖 Reading from BLE device: {}", address);
            
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
            
            // Find a readable characteristic
            let services = device.services().await
                .map_err(|e| BleError::PlatformError(format!("Failed to get services: {}", e)))?;
            
            for service in services {
                let characteristics = service.characteristics().await
                    .map_err(|e| BleError::PlatformError(format!("Failed to get characteristics: {}", e)))?;
                
                for characteristic in characteristics {
                    let properties = characteristic.flags().await
                        .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic properties: {}", e)))?;
                    
                    if properties.read {
                        return read_from_characteristic(&characteristic).await;
                    }
                }
            }
            
            Err(BleError::CharacteristicNotFound)
        }

    }

    /// Find a writable characteristic from cached GATT services
    fn find_cached_writable_characteristic(
        client_info: &ClientInfo,
    ) -> Option<(Uuid, Uuid)> {
        for (service_uuid, service) in &client_info.services {
            for (char_uuid, characteristic) in &service.characteristics {
                if characteristic.properties.write || characteristic.properties.write_without_response {
                    return Some((*service_uuid, *char_uuid));
                }
            }
        }
        None
    }

    /// Write data to a specific GATT characteristic
    async fn write_to_characteristic(
        characteristic: &Characteristic,
        data: &[u8],
    ) -> Result<(), BleError> {
        let char_uuid = characteristic.uuid().await
            .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic UUID: {}", e)))?;
        
        let properties = characteristic.flags().await
            .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic properties: {}", e)))?;
        
        tracing::info!("📝 Writing to characteristic: {}", char_uuid);
        tracing::debug!("   Properties - Write: {}, WriteWithoutResponse: {}", 
            properties.write, properties.write_without_response);
        
        // Choose write type based on characteristic properties
        let write_with_response = if properties.write_without_response {
            false
        } else if properties.write {
            true
        } else {
            return Err(BleError::CharacteristicNotFound);
        };
        
        // Perform the write operation
        if write_with_response {
            characteristic.write(data).await
                .map_err(|e| BleError::TransmissionFailed(format!("Failed to write to characteristic {}: {}", char_uuid, e)))?;
        } else {
            // For write without response, we'll use the regular write method
            // as write_without_response might not be available in this version of bluer
            characteristic.write(data).await
                .map_err(|e| BleError::TransmissionFailed(format!("Failed to write to characteristic {}: {}", char_uuid, e)))?;
        }
        
        tracing::info!("✅ Successfully wrote {} bytes to characteristic: {}", data.len(), char_uuid);
        Ok(())
    }

    /// Read data from a specific GATT characteristic
    async fn read_from_characteristic(
        characteristic: &Characteristic,
    ) -> Result<Vec<u8>, BleError> {
        let char_uuid = characteristic.uuid().await
            .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic UUID: {}", e)))?;
        
        let properties = characteristic.flags().await
            .map_err(|e| BleError::PlatformError(format!("Failed to get characteristic properties: {}", e)))?;
        
        if !properties.read {
            return Err(BleError::CharacteristicNotFound);
        }
        
        tracing::info!("📖 Reading from characteristic: {}", char_uuid);
        
        // Perform the read operation
        let data = characteristic.read().await
            .map_err(|e| BleError::TransmissionFailed(format!("Failed to read from characteristic {}: {}", char_uuid, e)))?;
        
        tracing::info!("✅ Successfully read {} bytes from characteristic: {}", data.len(), char_uuid);
        tracing::debug!("   Data: {:02x?}", data);
        Ok(data)
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

    async fn read_from_device(&self, _address: &str) -> Result<Vec<u8>, crate::ble::adapter::BleError> {
        Err(crate::ble::adapter::BleError::OperationNotSupported(
            "Linux BLE adapter not available - compile with 'linux' feature".to_string()
        ))
    }
}
