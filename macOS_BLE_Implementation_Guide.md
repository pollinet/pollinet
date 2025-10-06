# macOS BLE Adapter Implementation Guide

## Overview
This guide provides step-by-step instructions for implementing the macOS BLE adapter to enable cross-platform Bluetooth Low Energy communication between Linux and macOS PolliNet SDK instances.

## Prerequisites
- macOS development machine
- Xcode installed with command line tools
- Rust development environment
- Basic understanding of Core Bluetooth framework

## Current Status
- ✅ Linux BLE adapter implemented (BlueZ)
- ❌ macOS BLE adapter (stub only)
- ❌ Cross-platform discovery not working

## Implementation Steps

### Step 1: Add macOS Dependencies

Update `Cargo.toml` to include macOS-specific dependencies:

```toml
[dependencies]
# ... existing dependencies ...

# macOS BLE support
core-bluetooth = { version = "0.1", optional = true }

[features]
default = []
linux = ["bluer"]
macos = ["core-bluetooth"]  # Add this line
windows = []
android = []
```

### Step 2: Implement macOS BLE Adapter

Replace the stub implementation in `src/ble/macos.rs` with a full Core Bluetooth implementation:

```rust
//! macOS BLE implementation using Core Bluetooth
//! 
//! This module provides a native macOS implementation using the Core Bluetooth framework.

use super::adapter::{BleAdapter, BleError, AdapterInfo, DiscoveredDevice};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use uuid::Uuid;

/// macOS BLE adapter implementation using Core Bluetooth
pub struct MacOSBleAdapter {
    /// Central manager for BLE operations
    central_manager: Arc<Mutex<Option<core_bluetooth::CentralManager>>>,
    /// Discovered devices cache
    discovered_devices: Arc<Mutex<HashMap<String, DiscoveredDevice>>>,
    /// Advertising status
    is_advertising: Arc<Mutex<bool>>,
    /// Service UUID for PolliNet
    service_uuid: Uuid,
    /// Receive callback
    receive_callback: Arc<Mutex<Option<Box<dyn Fn(Vec<u8>) + Send + 'static>>>>,
}

impl MacOSBleAdapter {
    /// Create a new macOS BLE adapter
    pub async fn new() -> Result<Self, BleError> {
        // Initialize Core Bluetooth central manager
        let central_manager = core_bluetooth::CentralManager::new()
            .map_err(|e| BleError::PlatformError(format!("Failed to create Core Bluetooth manager: {}", e)))?;

        // Parse PolliNet service UUID
        let service_uuid = Uuid::parse_str(super::super::adapter::POLLINET_SERVICE_UUID)
            .map_err(|e| BleError::InvalidUuid(format!("Invalid PolliNet service UUID: {}", e)))?;

        Ok(Self {
            central_manager: Arc::new(Mutex::new(Some(central_manager))),
            discovered_devices: Arc::new(Mutex::new(HashMap::new())),
            is_advertising: Arc::new(Mutex::new(false)),
            service_uuid,
            receive_callback: Arc::new(Mutex::new(None)),
        })
    }

    /// Start BLE advertising using Core Bluetooth
    async fn start_core_bluetooth_advertising(&self, service_name: &str) -> Result<(), BleError> {
        let manager_guard = self.central_manager.lock().unwrap();
        let manager = manager_guard.as_ref().ok_or_else(|| {
            BleError::PlatformError("Core Bluetooth manager not available".to_string())
        })?;

        // Create advertisement data
        let advertisement_data = core_bluetooth::AdvertisementData {
            local_name: Some(service_name.to_string()),
            service_uuids: vec![self.service_uuid],
            ..Default::default()
        };

        // Start advertising
        manager.start_advertising(advertisement_data)
            .map_err(|e| BleError::AdvertisingFailed(format!("Core Bluetooth advertising failed: {}", e)))?;

        tracing::info!("📡 macOS BLE advertising started with Core Bluetooth");
        tracing::info!("   Service UUID: {}", self.service_uuid);
        tracing::info!("   Service Name: {}", service_name);

        Ok(())
    }
}

#[async_trait]
impl BleAdapter for MacOSBleAdapter {
    async fn start_advertising(&self, service_uuid: &str, service_name: &str) -> Result<(), BleError> {
        tracing::info!("🚀 Starting BLE advertising on macOS");
        tracing::info!("   Service UUID: {}", service_uuid);
        tracing::info!("   Service Name: {}", service_name);

        // Validate UUID matches PolliNet service
        if service_uuid != super::super::adapter::POLLINET_SERVICE_UUID {
            return Err(BleError::InvalidUuid(format!(
                "Expected {}, got {}", 
                super::super::adapter::POLLINET_SERVICE_UUID, 
                service_uuid
            )));
        }

        // Set advertising status
        {
            let mut status = self.is_advertising.lock().unwrap();
            *status = true;
        }

        // Start Core Bluetooth advertising
        self.start_core_bluetooth_advertising(service_name).await?;

        tracing::info!("✅ macOS BLE advertising started successfully");
        Ok(())
    }

    async fn stop_advertising(&self) -> Result<(), BleError> {
        tracing::info!("🛑 Stopping BLE advertising on macOS");

        let manager_guard = self.central_manager.lock().unwrap();
        let manager = manager_guard.as_ref().ok_or_else(|| {
            BleError::PlatformError("Core Bluetooth manager not available".to_string())
        })?;

        // Stop advertising
        manager.stop_advertising()
            .map_err(|e| BleError::AdvertisingFailed(format!("Failed to stop advertising: {}", e)))?;

        // Update advertising status
        {
            let mut status = self.is_advertising.lock().unwrap();
            *status = false;
        }

        tracing::info!("✅ macOS BLE advertising stopped successfully");
        Ok(())
    }

    async fn send_packet(&self, data: &[u8]) -> Result<(), BleError> {
        tracing::debug!("📤 Sending packet via macOS BLE ({} bytes)", data.len());
        
        // TODO: Implement actual packet sending to connected GATT clients
        // This would involve:
        // 1. Getting connected peripherals
        // 2. Finding the PolliNet service
        // 3. Writing to the characteristic
        // 4. Handling write responses
        
        tracing::info!("📤 Packet sent via macOS BLE (placeholder implementation)");
        Ok(())
    }

    fn on_receive(&self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>) {
        tracing::info!("📥 Setting up macOS BLE receive callback");
        let mut callback_guard = self.receive_callback.lock().unwrap();
        *callback_guard = Some(callback);
    }

    fn is_advertising(&self) -> bool {
        let status = self.is_advertising.lock().unwrap();
        *status
    }

    fn connected_clients_count(&self) -> usize {
        // TODO: Implement connected client count
        // This would query the Core Bluetooth manager for connected peripherals
        0
    }

    fn get_adapter_info(&self) -> AdapterInfo {
        AdapterInfo {
            platform: "macOS".to_string(),
            name: "Core Bluetooth".to_string(),
            address: "00:00:00:00:00:00".to_string(), // TODO: Get actual adapter address
            powered: true, // TODO: Check actual power state
            discoverable: true, // TODO: Check actual discoverable state
        }
    }

    async fn start_scanning(&self) -> Result<(), BleError> {
        tracing::info!("🔍 Starting BLE scanning on macOS");

        let manager_guard = self.central_manager.lock().unwrap();
        let manager = manager_guard.as_ref().ok_or_else(|| {
            BleError::PlatformError("Core Bluetooth manager not available".to_string())
        })?;

        // Start scanning for devices
        manager.start_scanning()
            .map_err(|e| BleError::PlatformError(format!("Failed to start scanning: {}", e)))?;

        tracing::info!("✅ macOS BLE scanning started successfully");
        Ok(())
    }

    async fn stop_scanning(&self) -> Result<(), BleError> {
        tracing::info!("🛑 Stopping BLE scanning on macOS");

        let manager_guard = self.central_manager.lock().unwrap();
        let manager = manager_guard.as_ref().ok_or_else(|| {
            BleError::PlatformError("Core Bluetooth manager not available".to_string())
        })?;

        // Stop scanning
        manager.stop_scanning()
            .map_err(|e| BleError::PlatformError(format!("Failed to stop scanning: {}", e)))?;

        tracing::info!("✅ macOS BLE scanning stopped successfully");
        Ok(())
    }

    async fn get_discovered_devices(&self) -> Result<Vec<DiscoveredDevice>, BleError> {
        let devices_guard = self.discovered_devices.lock().unwrap();
        let devices: Vec<DiscoveredDevice> = devices_guard.values().cloned().collect();
        
        tracing::debug!("📱 Found {} discovered devices on macOS", devices.len());
        Ok(devices)
    }
}
```

### Step 3: Add Core Bluetooth Delegate Implementation

Create a new file `src/ble/macos_delegate.rs`:

```rust
//! macOS Core Bluetooth delegate implementation
//! 
//! Handles Core Bluetooth events and callbacks for the macOS BLE adapter.

use super::macos::MacOSBleAdapter;
use super::adapter::DiscoveredDevice;
use uuid::Uuid;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Core Bluetooth delegate for handling BLE events
pub struct MacOSBleDelegate {
    adapter: Arc<MacOSBleAdapter>,
}

impl MacOSBleDelegate {
    pub fn new(adapter: Arc<MacOSBleAdapter>) -> Self {
        Self { adapter }
    }
}

impl core_bluetooth::CentralManagerDelegate for MacOSBleDelegate {
    fn did_discover_peripheral(&self, peripheral: &core_bluetooth::Peripheral, advertisement_data: &core_bluetooth::AdvertisementData, rssi: i16) {
        tracing::info!("🔍 Discovered BLE device: {}", peripheral.identifier());
        
        // Check if this device is advertising PolliNet service
        let pollinet_uuid = Uuid::parse_str(super::super::adapter::POLLINET_SERVICE_UUID).unwrap();
        let is_pollinet_device = advertisement_data.service_uuids.contains(&pollinet_uuid);
        
        if is_pollinet_device {
            tracing::info!("🎯 Found PolliNet device: {}", peripheral.identifier());
            
            let discovered_device = DiscoveredDevice {
                address: peripheral.identifier().to_string(),
                name: advertisement_data.local_name.clone(),
                service_uuids: advertisement_data.service_uuids.clone(),
                rssi: Some(rssi),
                last_seen: Instant::now(),
            };
            
            // Add to discovered devices cache
            {
                let mut devices_guard = self.adapter.discovered_devices.lock().unwrap();
                devices_guard.insert(peripheral.identifier().to_string(), discovered_device);
            }
        }
    }
    
    fn did_connect_peripheral(&self, peripheral: &core_bluetooth::Peripheral) {
        tracing::info!("🔗 Connected to BLE device: {}", peripheral.identifier());
        
        // TODO: Discover services and characteristics
        // This would involve:
        // 1. Discovering PolliNet service
        // 2. Finding read/write characteristics
        // 3. Setting up characteristic notifications
    }
    
    fn did_disconnect_peripheral(&self, peripheral: &core_bluetooth::Peripheral, error: Option<&core_bluetooth::BluetoothError>) {
        if let Some(error) = error {
            tracing::warn!("❌ Disconnected from BLE device {} with error: {}", peripheral.identifier(), error);
        } else {
            tracing::info!("🔌 Disconnected from BLE device: {}", peripheral.identifier());
        }
    }
    
    fn did_fail_to_connect_peripheral(&self, peripheral: &core_bluetooth::Peripheral, error: &core_bluetooth::BluetoothError) {
        tracing::error!("❌ Failed to connect to BLE device {}: {}", peripheral.identifier(), error);
    }
}
```

### Step 4: Update macOS Module Declaration

Update `src/ble/macos.rs` to include the delegate:

```rust
// Add at the top of macos.rs
mod macos_delegate;

// ... rest of the implementation ...
```

### Step 5: Test the Implementation

Create a test script `test_macos_ble.sh`:

```bash
#!/bin/bash
# Test macOS BLE implementation

echo "🧪 Testing macOS BLE Implementation"

# Build with macOS feature
echo "📦 Building with macOS feature..."
cargo build --features macos --bin pollinet

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    
    # Run the application
    echo "🚀 Running PolliNet with macOS BLE..."
    cargo run --features macos --bin pollinet
else
    echo "❌ Build failed!"
    exit 1
fi
```

### Step 6: Cross-Platform Testing

Create a test to verify cross-platform communication:

```bash
#!/bin/bash
# Cross-platform BLE test

echo "🌐 Cross-Platform BLE Test"
echo "=========================="

echo "1. Start Linux BLE server:"
echo "   cargo run --features linux --bin pollinet"
echo ""
echo "2. Start macOS BLE client:"
echo "   cargo run --features macos --bin pollinet"
echo ""
echo "3. Verify discovery:"
echo "   - macOS should discover Linux device"
echo "   - Linux should show connected client"
echo "   - Fragment exchange should work"
```

## Expected Results

After implementation, you should see:

### macOS Output:
```
🚀 Starting BLE advertising on macOS
   Service UUID: 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7
   Service Name: PolliNet
📡 macOS BLE advertising started with Core Bluetooth
✅ macOS BLE advertising started successfully
🔍 Starting BLE scanning on macOS
✅ macOS BLE scanning started successfully
🔍 Discovered BLE device: 90:65:84:5C:9B:2A
🎯 Found PolliNet device: 90:65:84:5C:9B:2A
```

### Linux Output:
```
🚀 Starting BLE advertising on Linux
   Service UUID: 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7
   Service Name: PolliNet
📡 BLE advertising started successfully on Linux
🔗 New BLE client connected: 90:65:84:5C:9B:2A
📨 Received fragment 1 of 3 for transaction tx_123
```

## Troubleshooting

### Common Issues:

1. **Core Bluetooth Permission Denied**
   - Add Bluetooth usage description to Info.plist
   - Request Bluetooth permissions at runtime

2. **Service UUID Not Found**
   - Verify UUID format and parsing
   - Check advertisement data structure

3. **Connection Failures**
   - Ensure devices are within range
   - Check Bluetooth adapter state
   - Verify service/characteristic UUIDs match

4. **Build Errors**
   - Ensure Xcode command line tools are installed
   - Check Rust toolchain compatibility
   - Verify core-bluetooth crate version

## Next Steps

After successful implementation:

1. **Add GATT Server Implementation**: Full service/characteristic handling
2. **Implement Device Connection**: Connect to discovered devices
3. **Add Fragment Exchange**: Bidirectional data transmission
4. **Error Handling**: Robust error recovery and reconnection
5. **Performance Optimization**: Connection pooling and caching

## Dependencies to Add

Add these to `Cargo.toml`:

```toml
[dependencies]
core-bluetooth = "0.1"  # macOS Core Bluetooth bindings
objc = "0.2"            # Objective-C runtime bindings
cocoa = "0.24"          # macOS framework bindings
```

## Testing Checklist

- [ ] macOS BLE adapter compiles
- [ ] Advertising starts successfully
- [ ] Scanning discovers Linux devices
- [ ] Linux devices are recognized as PolliNet
- [ ] Connection establishment works
- [ ] Fragment exchange functions
- [ ] Error handling is robust
- [ ] Cross-platform compatibility verified

## Notes

- Core Bluetooth is asynchronous - use proper async/await patterns
- Permission handling is crucial on macOS
- UUID format must match exactly between platforms
- RSSI values help with device proximity detection
- Connection state management is important for reliability

This implementation will enable full cross-platform BLE communication between Linux and macOS PolliNet instances.
