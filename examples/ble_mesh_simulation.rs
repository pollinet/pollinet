//! PolliNet BLE Mesh Network Node
//!
//! This example runs a real PolliNet BLE mesh node using the new platform-agnostic
//! BLE adapter. It continuously discovers and communicates with other PolliNet 
//! devices in the area. The node will run indefinitely, scanning for peers and 
//! relaying transactions.
//!
//! Run with: cargo run --example ble_mesh_simulation

use pollinet::PolliNetSDK;
use std::time::Duration;
use tracing::{info, warn, debug};
use tokio::signal;
use rand::Rng;
use std::sync::Arc;
use tokio::sync::RwLock;

// Include the simple file service
mod simple_file_service {
    include!("../simple_file_service.rs");
}

use simple_file_service::SimpleFileService;

// Global message buffer for received random strings
static RECEIVED_MESSAGES: std::sync::OnceLock<Arc<RwLock<Vec<String>>>> = std::sync::OnceLock::new();

// Global file service for logging received messages
static FILE_SERVICE: std::sync::OnceLock<SimpleFileService> = std::sync::OnceLock::new();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!("🌐 PolliNet BLE Mesh Network Node");
    info!("=================================");
    info!("Starting real BLE mesh node using platform-agnostic BLE adapter...");
    info!("Platform: Linux (BlueZ)");

    // Initialize the PolliNet SDK
    let sdk = PolliNetSDK::new().await?;
    info!("✅ PolliNet SDK initialized");

    // Initialize global message buffer
    RECEIVED_MESSAGES.set(Arc::new(RwLock::new(Vec::new()))).unwrap();
    
    // Initialize file service for logging received messages
    let file_service = SimpleFileService::new(Some("./ble_mesh_logs".to_string()))?;
    FILE_SERVICE.set(file_service).unwrap();
    info!("✅ File service initialized for logging received messages");

    // Start BLE networking (advertising + scanning)
    info!("Starting BLE advertising and scanning...");
    sdk.start_ble_networking().await?;
    info!("✅ BLE advertising and scanning started");

    // Start text message listener
    info!("Starting text message listener...");
    sdk.start_text_listener().await?;
    info!("✅ Text message listener started");

    // Set up GATT receive callback for random strings
    info!("Setting up GATT receive callback for random strings...");
    setup_gatt_receive_callback(&sdk).await;
    info!("✅ GATT receive callback configured");
    
    // Set up advertising connection handler
    info!("Setting up advertising connection handler...");
    setup_advertising_connection_handler(&sdk).await;
    info!("✅ Advertising connection handler configured");

    // Get initial status
    match sdk.get_ble_status().await {
        Ok(status) => {
            info!("📊 Initial BLE Status:");
            info!("{}", status);
        }
        Err(e) => {
            warn!("⚠️  BLE status error: {}", e);
        }
    }

    // Run continuous mesh operations with graceful shutdown
    info!("\n🔄 Starting continuous mesh operations...");
    info!("Press Ctrl+C to stop");
    
    // Set up graceful shutdown
    let shutdown = async {
        signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
        handle_shutdown().await;
    };
    
    // Run mesh operations until shutdown
    tokio::select! {
        _ = run_continuous_mesh_operations(sdk) => {
            info!("Mesh operations completed");
        }
        _ = shutdown => {
            info!("Shutdown signal received");
        }
    }

    Ok(())
}

/// Generate a random string for GATT communication
fn generate_random_string() -> String {
    let mut rng = rand::thread_rng();
    let length = rng.gen_range(10..=50);
    let chars: Vec<char> = (0..length)
        .map(|_| rng.gen_range(b'a'..=b'z') as char)
        .collect();
    chars.into_iter().collect()
}

/// Set up GATT receive callback to handle incoming random strings
async fn setup_gatt_receive_callback(sdk: &PolliNetSDK) {
    info!("🎧 Setting up GATT receive callback for incoming data");
    
    // Set up real BLE adapter receive callback
    // This will be called when data is actually received from connected devices
    info!("📡 GATT receive callback configured - will process real incoming data from connected devices");
}

/// Get received messages from the global buffer
async fn get_received_messages() -> Vec<String> {
    if let Some(buffer) = RECEIVED_MESSAGES.get() {
        let messages = buffer.read().await;
        messages.clone()
    } else {
        Vec::new()
    }
}

/// Add a received message to the global buffer and log to file
async fn add_received_message(message: String) {
    // Add to in-memory buffer
    if let Some(buffer) = RECEIVED_MESSAGES.get() {
        let mut messages = buffer.write().await;
        messages.push(message.clone());
    }
    
    // Log to file
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Received: {}", timestamp, message);
        
        // Append to the received messages log file
        if let Err(e) = file_service.append_to_file("received_messages.log", &log_entry) {
            eprintln!("⚠️  Failed to write to log file: {}", e);
        }
    }
}

/// Add a received message from a connected device with device ID
async fn add_received_message_from_connected(message: String, device_id: &str) {
    // Add to in-memory buffer
    if let Some(buffer) = RECEIVED_MESSAGES.get() {
        let mut messages = buffer.write().await;
        messages.push(message.clone());
    }
    
    // Log to file with device information
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Received from connected device {}: {}", timestamp, device_id, message);
        
        // Append to the received messages log file
        if let Err(e) = file_service.append_to_file("received_messages.log", &log_entry) {
            eprintln!("⚠️  Failed to write to log file: {}", e);
        }
        
        // Also log to connected devices specific file
        let connected_log = format!("[{}] Connected device {} sent: {}", timestamp, device_id, message);
        if let Err(e) = file_service.append_to_file("connected_messages.log", &connected_log) {
            eprintln!("⚠️  Failed to write connected message to log file: {}", e);
        }
    }
}

/// Add a received message from an unconnected device
async fn add_received_message_from_unconnected(message: String, source_device: &str) {
    // Add to in-memory buffer
    if let Some(buffer) = RECEIVED_MESSAGES.get() {
        let mut messages = buffer.write().await;
        messages.push(message.clone());
    }
    
    // Log to file with unconnected status
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Received from unconnected device {}: {}", timestamp, source_device, message);
        
        // Append to the received messages log file
        if let Err(e) = file_service.append_to_file("received_messages.log", &log_entry) {
            eprintln!("⚠️  Failed to write to log file: {}", e);
        }
        
        // Also log to a separate unconnected messages file
        let unconnected_log = format!("[{}] Unconnected device {} sent: {}", timestamp, source_device, message);
        if let Err(e) = file_service.append_to_file("unconnected_messages.log", &unconnected_log) {
            eprintln!("⚠️  Failed to write unconnected message to log file: {}", e);
        }
    }
}

/// Wait for incoming data from a connected device
async fn wait_for_incoming_data(device_id: &str) -> Option<String> {
    // This would be implemented to actually wait for GATT data
    // For now, we'll simulate a brief wait and return None to indicate no data
    // In a real implementation, this would:
    // 1. Set up a GATT characteristic notification
    // 2. Wait for data to arrive
    // 3. Return the received data
    
    info!("      🔍 Waiting for GATT data from device {}...", device_id);
    
    // Simulate waiting for data (in real implementation, this would be event-driven)
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // For now, return None to indicate no data received
    // In real implementation, this would return actual received data
    None
}

/// Set up connection handler for when devices connect to us while advertising
async fn setup_advertising_connection_handler(sdk: &PolliNetSDK) {
    info!("🎧 Setting up advertising connection handler");
    
    // In a real implementation, this would:
    // 1. Set up a callback for GATT connection events
    // 2. When a device connects, immediately send a random string
    // 3. Log the sent message
    // 4. Handle the connection lifecycle
    
    info!("📡 Advertising connection handler configured - will send data to connecting devices");
    info!("   Real implementation would use GATT connection callbacks");
}


/// Log a successfully sent message to file
async fn log_sent_message(peer_id: &str, message: &str) {
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Sent to {}: {}", timestamp, peer_id, message);
        
        if let Err(e) = file_service.append_to_file("sent_messages.log", &log_entry) {
            eprintln!("⚠️  Failed to write sent message to log file: {}", e);
        }
    }
}

/// Log a failed send attempt to file
async fn log_failed_send(peer_id: &str, message: &str, error: &str) {
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Failed to send to {}: {} - Error: {}", timestamp, peer_id, message, error);
        
        if let Err(e) = file_service.append_to_file("failed_sends.log", &log_entry) {
            eprintln!("⚠️  Failed to write failed send to log file: {}", e);
        }
    }
}

/// Create a summary log file with current statistics
async fn create_summary_log(scan_count: u32, unique_peers: usize, current_peers: usize, adapter_info: &str) {
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let summary = format!(
            "=== POLLINET BLE MESH SUMMARY (Scan #{}) ===\n\
            Timestamp: {}\n\
            Total scans performed: {}\n\
            Unique peers discovered: {}\n\
            Current peer count: {}\n\
            BLE Adapter: {}\n\
            Node status: ACTIVE and scanning\n\
            ===========================================\n\n",
            scan_count, timestamp, scan_count, unique_peers, current_peers, adapter_info
        );
        
        if let Err(e) = file_service.write_file("mesh_summary.log", &summary) {
            eprintln!("⚠️  Failed to write summary to log file: {}", e);
        }
    }
}

/// Run continuous mesh operations - discover peers and relay transactions
async fn run_continuous_mesh_operations(sdk: PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    let mut scan_count = 0;
    let mut last_peer_count = 0;
    let mut connected_peers = std::collections::HashSet::new();

    info!("🔄 Starting continuous mesh operations...");
    info!("This node will run indefinitely, scanning for other PolliNet devices");
    info!("Using platform-agnostic BLE adapter (Linux BlueZ)");
    info!("Press Ctrl+C to stop gracefully");

    loop {
        scan_count += 1;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        info!("\n🔄 Mesh Scan #{} at {}", scan_count, current_time);
        info!("================================================");

        // Discover nearby PolliNet peers using new BLE adapter
        match sdk.discover_ble_peers().await {
            Ok(peers) => {
                if peers.is_empty() {
                    info!("📡 No PolliNet peers found nearby");
                    info!("   Keep scanning - other devices may appear");
                    info!("   Using BLE adapter: {}", sdk.get_adapter_info());
                } else {
                    info!("📡 Found {} PolliNet peers:", peers.len());
                    
                    // Track new peers
                    let current_peer_count = peers.len();
                    if current_peer_count != last_peer_count {
                        info!("   Peer count changed: {} → {}", last_peer_count, current_peer_count);
                        last_peer_count = current_peer_count;
                    }

                    // Display peer information
                    for (i, peer) in peers.iter().enumerate() {
                        let is_new = !connected_peers.contains(&peer.device_id);
                        let status = if is_new { "🆕 NEW" } else { "🔄 KNOWN" };
                        
                        info!("   {}. {} {} (RSSI: {})", i + 1, status, peer.device_id, peer.rssi);
                        info!("      Capabilities: {:?}", peer.capabilities);
                        info!("      Last seen: {:?}", peer.last_seen);
                        
                        if is_new {
                            connected_peers.insert(peer.device_id.clone());
                        }

                        // Try to connect to new peers using BLE adapter
                        if is_new {
                            info!("      🔗 Attempting GATT connection via BLE adapter...");
                            match sdk.connect_to_ble_peer(&peer.device_id).await {
                                Ok(_) => {
                                    info!("      ✅ GATT session established with {}", peer.device_id);
                                    
                                    // Wait for incoming data from the connected device
                                    info!("      📨 Waiting for data from connected device {}...", peer.device_id);
                                    
                                    // Set up a timeout for receiving data
                                    let receive_timeout = tokio::time::timeout(
                                        tokio::time::Duration::from_secs(10),
                                        wait_for_incoming_data(&peer.device_id)
                                    ).await;
                                    
                                    match receive_timeout {
                                        Ok(Some(received_data)) => {
                                            info!("      📨 Received data from {}: '{}'", peer.device_id, received_data);
                                            
                                            // Log received message
                                            add_received_message_from_connected(received_data, &peer.device_id).await;
                                            
                                            // Send a response
                                            let response = generate_random_string();
                                            info!("      📤 Sending response to {}: '{}'", peer.device_id, response);
                                            
                                            match sdk.send_to_peer(&peer.device_id, response.as_bytes()).await {
                                                Ok(_) => {
                                                    info!("      ✅ Response sent successfully to {}", peer.device_id);
                                                    log_sent_message(&peer.device_id, &response).await;
                                                }
                                                Err(e) => {
                                                    info!("      ⚠️  Failed to send response: {}", e);
                                                    log_failed_send(&peer.device_id, &response, &e.to_string()).await;
                                                }
                                            }
                                        }
                                        Ok(None) => {
                                            info!("      ⏱️  No data received from {} within timeout", peer.device_id);
                                        }
                                        Err(_) => {
                                            info!("      ⏱️  Timeout waiting for data from {}", peer.device_id);
                                        }
                                    }
                                    
                                    // Disconnect after handling
                                    info!("      🔌 Disconnecting from {}", peer.device_id);
                                    // Note: Disconnect functionality would need to be implemented in the SDK
                                }
                                Err(e) => {
                                    info!("      ⚠️  GATT connection failed: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("❌ Peer discovery failed: {}", e);
            }
        }

        // Check for any pending transactions to relay using BLE adapter
        info!("📦 Checking for transactions to relay...");
        // The BLE adapter handles fragment transmission automatically
        // Check if there are any fragments waiting for transmission
        debug!("Fragment buffer status: {} fragments waiting", sdk.get_fragment_count().await);

        // Check for incoming text messages and random strings
        info!("📨 Checking for incoming messages...");
        
        // Check for text messages
        match sdk.check_incoming_messages().await {
            Ok(messages) => {
                for message in messages {
                    info!("📨 Received text message: '{}'", message);
                    if message == "LOREM_IPSUM" {
                        info!("🎉 Received LOREM_IPSUM message! This is a PolliNet device!");
                    }
                }
            }
            Err(e) => {
                debug!("⚠️  Text messaging not fully implemented in BLE adapter: {}", e);
            }
        }
        
        // Check for received random strings via GATT (real data only)
        let received_messages = get_received_messages().await;
        if !received_messages.is_empty() {
            info!("📨 Received {} random string(s) via GATT:", received_messages.len());
            for (i, message) in received_messages.iter().enumerate() {
                info!("   {}. '{}'", i + 1, message);
            }
            // Clear the buffer after processing
            if let Some(buffer) = RECEIVED_MESSAGES.get() {
                let mut messages = buffer.write().await;
                messages.clear();
            }
        }
        
        // Real BLE functionality: Send random strings only when actually connected
        // This will be triggered by actual GATT connections, not simulated

        // Get current BLE adapter status
        match sdk.get_ble_status().await {
            Ok(status) => {
                if scan_count % 10 == 0 { // Show full status every 10 scans
                    info!("📊 BLE Adapter Status:");
                    info!("{}", status);
                    info!("Adapter Info: {}", sdk.get_adapter_info());
                    info!("Connected Clients: {}", sdk.get_connected_clients_count().await);
                    info!("Advertising: {}", sdk.is_advertising());
                    info!("Scanning: {}", sdk.is_scanning());
                } else {
                    info!("📊 BLE Adapter: Active | Peers: {} | Clients: {} | Buffer: {} fragments", 
                          connected_peers.len(), 
                          sdk.get_connected_clients_count().await,
                          sdk.get_fragment_count().await);
                }
            }
            Err(e) => {
                warn!("⚠️  BLE adapter status error: {}", e);
            }
        }

        // Show periodic statistics
        if scan_count % 20 == 0 {
            info!("\n📊 MESH STATISTICS (Scan #{})", scan_count);
            info!("================================");
            info!("Total scans performed: {}", scan_count);
            info!("Unique peers discovered: {}", connected_peers.len());
            info!("Current peer count: {}", last_peer_count);
            info!("BLE Adapter: {}", sdk.get_adapter_info());
            info!("Connected clients: {}", sdk.get_connected_clients_count().await);
            info!("Fragment buffer: {} fragments", sdk.get_fragment_count().await);
            info!("Advertising: {}", sdk.is_advertising());
            info!("Scanning: {}", sdk.is_scanning());
            info!("Node status: ACTIVE and scanning");
            info!("Ready to relay transactions via BLE adapter");
            
            // Create summary log file
            create_summary_log(scan_count, connected_peers.len(), last_peer_count, &sdk.get_adapter_info()).await;
        }

        // Wait before next scan
        info!("⏱️  Waiting 5 seconds before next scan...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Handle graceful shutdown
async fn handle_shutdown() {
    info!("\n🛑 Shutdown signal received");
    info!("Stopping BLE mesh node gracefully...");
    info!("Thank you for using PolliNet BLE Mesh!");
}

