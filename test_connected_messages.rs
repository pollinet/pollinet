//! Test script to demonstrate connected device message handling
//! 
//! This script simulates receiving messages from both connected and unconnected devices
//! to show the different logging behaviors.

// Include the simple file service
mod simple_file_service {
    include!("simple_file_service.rs");
}

use simple_file_service::SimpleFileService;
use std::time::{SystemTime, UNIX_EPOCH};

/// Simulate adding a received message from a connected device
fn add_received_message_from_connected(file_service: &SimpleFileService, message: &str, device_id: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

/// Simulate adding a received message from an unconnected device
fn add_received_message_from_unconnected(file_service: &SimpleFileService, message: &str, source_device: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

fn generate_random_string() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    format!("random_{:x}", hasher.finish())[..12].to_string()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing Connected vs Unconnected Device Messages");
    println!("==================================================");
    
    // Initialize file service
    let file_service = SimpleFileService::new(Some("./connected_test_logs".to_string()))?;
    println!("✅ File service initialized");
    
    // Simulate messages from connected devices
    println!("\n📱 Simulating messages from CONNECTED devices...");
    let connected_devices = vec![
        "90:65:84:5C:9B:2A",
        "A1:B2:C3:D4:E5:F6", 
        "F6:E5:D4:C3:B2:A1",
        "11:22:33:44:55:66"
    ];
    
    for i in 1..=6 {
        let device = connected_devices[i % connected_devices.len()];
        let message = generate_random_string();
        
        println!("📨 Connected device {} sent: {}", device, message);
        add_received_message_from_connected(&file_service, &message, device);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    
    // Simulate messages from unconnected devices
    println!("\n📡 Simulating messages from UNCONNECTED devices...");
    let unconnected_devices = vec![
        "AA:BB:CC:DD:EE:FF",
        "11:22:33:44:55:66", 
        "99:88:77:66:55:44",
        "FF:EE:DD:CC:BB:AA"
    ];
    
    for i in 1..=4 {
        let device = unconnected_devices[i % unconnected_devices.len()];
        let message = generate_random_string();
        
        println!("📡 Unconnected device {} sent: {}", device, message);
        add_received_message_from_unconnected(&file_service, &message, device);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    
    // List all created files
    println!("\n📋 Created log files:");
    let files = file_service.list_files()?;
    for file in files {
        let size = file_service.get_file_size(&file)?;
        println!("  - {} ({} bytes)", file, size);
    }
    
    // Show content of different log files
    println!("\n📖 Content of received_messages.log (all messages):");
    match file_service.read_file("received_messages.log") {
        Ok(content) => println!("{}", content),
        Err(e) => println!("Error reading file: {}", e),
    }
    
    println!("\n📖 Content of connected_messages.log (connected only):");
    match file_service.read_file("connected_messages.log") {
        Ok(content) => println!("{}", content),
        Err(e) => println!("Error reading file: {}", e),
    }
    
    println!("\n📖 Content of unconnected_messages.log (unconnected only):");
    match file_service.read_file("unconnected_messages.log") {
        Ok(content) => println!("{}", content),
        Err(e) => println!("Error reading file: {}", e),
    }
    
    println!("\n✅ Connected vs Unconnected message test completed!");
    println!("📁 Check the './connected_test_logs' directory for the created log files.");
    
    Ok(())
}
