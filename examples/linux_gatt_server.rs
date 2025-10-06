//! Linux GATT Server Example for PolliNet SDK
//! 
//! This example demonstrates how to use the platform-agnostic BLE adapter
//! to create a GATT server on Linux using the bluer crate.

use pollinet::ble::{create_ble_adapter, POLLINET_SERVICE_UUID, POLLINET_SERVICE_NAME};
use tracing::{info, error, warn};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    info!("🚀 Starting PolliNet Linux GATT Server Example...");
    
    // Create platform-specific BLE adapter
    info!("📡 Creating BLE adapter...");
    let adapter = match create_ble_adapter().await {
        Ok(adapter) => {
            info!("✅ BLE adapter created successfully");
            adapter
        },
        Err(e) => {
            error!("❌ Failed to create BLE adapter: {}", e);
            return Err(e.into());
        }
    };
    
    // Display adapter information
    let adapter_info = adapter.get_adapter_info();
    info!("🔧 Adapter Info:");
    info!("   Platform: {}", adapter_info.platform);
    info!("   Name: {}", adapter_info.name);
    info!("   Address: {}", adapter_info.address);
    info!("   Powered: {}", adapter_info.powered);
    info!("   Discoverable: {}", adapter_info.discoverable);
    
    // Set up receive callback
    info!("📥 Setting up receive callback...");
    adapter.on_receive(Box::new(|data| {
        info!("📨 Received data: {} bytes", data.len());
        info!("   Data: {:02x?}", data);
        
        // Echo the data back (mock response)
        let response = format!("Echo: {}", String::from_utf8_lossy(&data));
        info!("📤 Sending response: {}", response);
    }));
    
    // Start advertising
    info!("📡 Starting BLE advertising...");
    info!("   Service UUID: {}", POLLINET_SERVICE_UUID);
    info!("   Service Name: {}", POLLINET_SERVICE_NAME);
    
    match adapter.start_advertising(POLLINET_SERVICE_UUID, POLLINET_SERVICE_NAME).await {
        Ok(_) => {
            info!("✅ BLE advertising started successfully");
        },
        Err(e) => {
            error!("❌ Failed to start BLE advertising: {}", e);
            return Err(e.into());
        }
    }
    
    // Display status
    info!("📊 GATT Server Status:");
    info!("   Advertising: {}", adapter.is_advertising());
    info!("   Connected clients: {}", adapter.connected_clients_count());
    
    // Simulate sending some test data
    info!("🧪 Testing data transmission...");
    let test_data = b"Hello from PolliNet Linux GATT Server!";
    match adapter.send_packet(test_data).await {
        Ok(_) => {
            info!("✅ Test packet sent successfully");
        },
        Err(e) => {
            warn!("⚠️  Failed to send test packet: {}", e);
        }
    }
    
    info!("🎉 PolliNet Linux GATT Server is running!");
    info!("💡 The server is now advertising and ready to receive connections");
    info!("🔄 Press Ctrl+C to stop the server");
    
    // Keep the server running
    let mut counter = 0;
    loop {
        sleep(Duration::from_secs(10)).await;
        counter += 1;
        
        info!("⏰ Server heartbeat #{}", counter);
        info!("   Advertising: {}", adapter.is_advertising());
        info!("   Connected clients: {}", adapter.connected_clients_count());
        
        // Send periodic status updates
        let status_data = format!("Status update #{}", counter).into_bytes();
        match adapter.send_packet(&status_data).await {
            Ok(_) => {
                info!("📤 Status update sent");
            },
            Err(e) => {
                warn!("⚠️  Failed to send status update: {}", e);
            }
        }
    }
}
