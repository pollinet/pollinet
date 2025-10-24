//! Test script to demonstrate file logging functionality
//! 
//! This script simulates the BLE mesh simulation's file logging features
//! without requiring actual BLE hardware.

// Include the simple file service
mod simple_file_service {
    include!("simple_file_service.rs");
}

use simple_file_service::SimpleFileService;
use std::time::{SystemTime, UNIX_EPOCH};

// Global file service for logging
static FILE_SERVICE: std::sync::OnceLock<SimpleFileService> = std::sync::OnceLock::new();

/// Simulate adding a received message
async fn add_received_message(message: String) {
    // Log to file
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Received: {}", timestamp, message);
        
        // Append to the received messages log file
        if let Err(e) = file_service.append_to_file("received_messages.log", &log_entry) {
            eprintln!("⚠️  Failed to write to log file: {}", e);
        }
    }
}

/// Simulate logging a sent message
async fn log_sent_message(peer_id: &str, message: &str) {
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Sent to {}: {}", timestamp, peer_id, message);
        
        if let Err(e) = file_service.append_to_file("sent_messages.log", &log_entry) {
            eprintln!("⚠️  Failed to write sent message to log file: {}", e);
        }
    }
}

/// Simulate logging a failed send
async fn log_failed_send(peer_id: &str, message: &str, error: &str) {
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let log_entry = format!("[{}] Failed to send to {}: {} - Error: {}", timestamp, peer_id, message, error);
        
        if let Err(e) = file_service.append_to_file("failed_sends.log", &log_entry) {
            eprintln!("⚠️  Failed to write failed send to log file: {}", e);
        }
    }
}

/// Create a summary log
async fn create_summary_log(scan_count: u32, unique_peers: usize, current_peers: usize, adapter_info: &str) {
    if let Some(file_service) = FILE_SERVICE.get() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing BLE Mesh File Logging");
    println!("================================");
    
    // Initialize file service
    let file_service = SimpleFileService::new(Some("./test_logs".to_string()))?;
    FILE_SERVICE.set(file_service).unwrap();
    println!("✅ File service initialized");
    
    // Simulate some received messages
    println!("\n📨 Simulating received messages...");
    let received_messages = vec![
        "hello_world_from_peer_1",
        "random_data_xyz123",
        "pollinet_test_message",
        "ble_mesh_communication",
        "another_random_string"
    ];
    
    for message in received_messages {
        println!("📥 Simulating received: {}", message);
        add_received_message(message.to_string()).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    // Simulate some sent messages
    println!("\n📤 Simulating sent messages...");
    let sent_messages = vec![
        ("90:65:84:5C:9B:2A", "sent_to_peer_1_abc"),
        ("A1:B2:C3:D4:E5:F6", "sent_to_peer_2_def"),
        ("F6:E5:D4:C3:B2:A1", "sent_to_peer_3_ghi"),
    ];
    
    for (peer_id, message) in sent_messages {
        println!("📤 Simulating sent to {}: {}", peer_id, message);
        log_sent_message(peer_id, message).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    // Simulate some failed sends
    println!("\n❌ Simulating failed sends...");
    let failed_messages = vec![
        ("90:65:84:5C:9B:2A", "failed_message_1", "Connection timeout"),
        ("A1:B2:C3:D4:E5:F6", "failed_message_2", "BLE adapter error"),
    ];
    
    for (peer_id, message, error) in failed_messages {
        println!("❌ Simulating failed send to {}: {} - {}", peer_id, message, error);
        log_failed_send(peer_id, message, error).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    // Create summary logs
    println!("\n📊 Creating summary logs...");
    for i in 1..=3 {
        create_summary_log(i * 20, i * 2, i, "Linux (BlueZ)").await;
        println!("📊 Created summary for scan #{}", i * 20);
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
    
    // List all created files
    println!("\n📋 Created log files:");
    if let Some(file_service) = FILE_SERVICE.get() {
        let files = file_service.list_files()?;
        for file in files {
            let size = file_service.get_file_size(&file)?;
            println!("  - {} ({} bytes)", file, size);
        }
    }
    
    println!("\n✅ File logging test completed!");
    println!("📁 Check the './test_logs' directory for the created log files.");
    
    Ok(())
}
