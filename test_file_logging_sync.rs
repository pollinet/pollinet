//! Test script to demonstrate file logging functionality (synchronous version)
//! 
//! This script simulates the BLE mesh simulation's file logging features
//! without requiring actual BLE hardware.

// Include the simple file service
mod simple_file_service {
    include!("simple_file_service.rs");
}

use simple_file_service::SimpleFileService;
use std::time::{SystemTime, UNIX_EPOCH};

/// Simulate adding a received message
fn add_received_message(file_service: &SimpleFileService, message: &str) {
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

/// Simulate logging a sent message
fn log_sent_message(file_service: &SimpleFileService, peer_id: &str, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let log_entry = format!("[{}] Sent to {}: {}", timestamp, peer_id, message);
    
    if let Err(e) = file_service.append_to_file("sent_messages.log", &log_entry) {
        eprintln!("⚠️  Failed to write sent message to log file: {}", e);
    }
}

/// Simulate logging a failed send
fn log_failed_send(file_service: &SimpleFileService, peer_id: &str, message: &str, error: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let log_entry = format!("[{}] Failed to send to {}: {} - Error: {}", timestamp, peer_id, message, error);
    
    if let Err(e) = file_service.append_to_file("failed_sends.log", &log_entry) {
        eprintln!("⚠️  Failed to write failed send to log file: {}", e);
    }
}

/// Create a summary log
fn create_summary_log(file_service: &SimpleFileService, scan_count: u32, unique_peers: usize, current_peers: usize, adapter_info: &str) {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing BLE Mesh File Logging (Synchronous)");
    println!("==============================================");
    
    // Initialize file service
    let file_service = SimpleFileService::new(Some("./test_logs".to_string()))?;
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
        add_received_message(&file_service, message);
        std::thread::sleep(std::time::Duration::from_millis(100));
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
        log_sent_message(&file_service, peer_id, message);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    // Simulate some failed sends
    println!("\n❌ Simulating failed sends...");
    let failed_messages = vec![
        ("90:65:84:5C:9B:2A", "failed_message_1", "Connection timeout"),
        ("A1:B2:C3:D4:E5:F6", "failed_message_2", "BLE adapter error"),
    ];
    
    for (peer_id, message, error) in failed_messages {
        println!("❌ Simulating failed send to {}: {} - {}", peer_id, message, error);
        log_failed_send(&file_service, peer_id, message, error);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    // Create summary logs
    println!("\n📊 Creating summary logs...");
    for i in 1..=3 {
        create_summary_log(&file_service, i * 20, (i * 2) as usize, i as usize, "Linux (BlueZ)");
        println!("📊 Created summary for scan #{}", i * 20);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    
    // List all created files
    println!("\n📋 Created log files:");
    let files = file_service.list_files()?;
    for file in files {
        let size = file_service.get_file_size(&file)?;
        println!("  - {} ({} bytes)", file, size);
    }
    
    // Show content of received messages log
    println!("\n📖 Content of received_messages.log:");
    match file_service.read_file("received_messages.log") {
        Ok(content) => println!("{}", content),
        Err(e) => println!("Error reading file: {}", e),
    }
    
    println!("\n✅ File logging test completed!");
    println!("📁 Check the './test_logs' directory for the created log files.");
    
    Ok(())
}
