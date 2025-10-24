//! Demo of the simple file service
//! 
//! This example shows how to use the simple file service for various tasks
//! like logging, configuration storage, and data persistence.

// Include the simple file service
mod simple_file_service {
    include!("../simple_file_service.rs");
}

use simple_file_service::SimpleFileService;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 PolliNet Simple File Service Demo");
    println!("===================================");
    
    // Create the file service
    let service = SimpleFileService::new(Some("./demo_files".to_string()))?;
    println!("📁 Using directory: {}", service.get_base_dir());
    
    // Demo 1: System logging
    println!("\n📝 Demo 1: System logging");
    println!("------------------------");
    
    // Create a log file with timestamp
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let log_entry = format!("[{}] System started - PolliNet BLE Mesh Node", timestamp);
    service.create_file("system.log", &log_entry)?;
    
    // Add more log entries
    for i in 1..=5 {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let log_entry = format!("[{}] BLE scan #{} - Found {} peers", timestamp, i, i * 2);
        service.append_to_file("system.log", &log_entry)?;
        
        // Simulate some processing time
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    // Read the log file
    let log_content = service.read_file("system.log")?;
    println!("✅ System log created:");
    println!("{}", log_content);
    
    // Demo 2: Configuration management
    println!("\n📝 Demo 2: Configuration management");
    println!("----------------------------------");
    
    // Create a configuration file
    let config = r#"{
    "pollinet_config": {
        "version": "1.0.0",
        "ble_settings": {
            "service_uuid": "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7",
            "advertising_interval": 1000,
            "scan_timeout": 5000,
            "max_peers": 10
        },
        "network_settings": {
            "relay_timeout": 30000,
            "fragment_size": 512,
            "max_retries": 3
        },
        "logging": {
            "level": "INFO",
            "file": "system.log",
            "max_size": 1048576
        }
    }
}"#;
    
    service.create_file("config.json", config)?;
    let config_content = service.read_file("config.json")?;
    println!("✅ Configuration file created:");
    println!("{}", config_content);
    
    // Demo 3: Data persistence
    println!("\n📝 Demo 3: Data persistence");
    println!("--------------------------");
    
    // Create a data file with peer information
    let peer_data = r#"{
    "peers": [
        {
            "id": "90:65:84:5C:9B:2A",
            "name": "PolliNet-Node-1",
            "capabilities": ["CAN_RELAY", "CAN_VOTE"],
            "last_seen": 1698000000,
            "rssi": -45
        },
        {
            "id": "A1:B2:C3:D4:E5:F6",
            "name": "PolliNet-Node-2", 
            "capabilities": ["CAN_RELAY"],
            "last_seen": 1698000100,
            "rssi": -52
        },
        {
            "id": "F6:E5:D4:C3:B2:A1",
            "name": "PolliNet-Node-3",
            "capabilities": ["CAN_VOTE", "CAN_GOVERN"],
            "last_seen": 1698000200,
            "rssi": -38
        }
    ],
    "total_peers": 3,
    "last_scan": 1698000200
}"#;
    
    service.create_file("peers.json", peer_data)?;
    let peers_content = service.read_file("peers.json")?;
    println!("✅ Peer data file created:");
    println!("{}", peers_content);
    
    // Demo 4: Transaction log
    println!("\n📝 Demo 4: Transaction log");
    println!("-------------------------");
    
    // Create a transaction log
    let tx_log = r#"{
    "transactions": [
        {
            "id": "tx_001",
            "type": "vote",
            "timestamp": 1698000000,
            "status": "pending",
            "fragments": 3,
            "relayed_to": ["90:65:84:5C:9B:2A", "A1:B2:C3:D4:E5:F6"]
        },
        {
            "id": "tx_002", 
            "type": "governance",
            "timestamp": 1698000100,
            "status": "confirmed",
            "fragments": 1,
            "relayed_to": ["F6:E5:D4:C3:B2:A1"]
        },
        {
            "id": "tx_003",
            "type": "relay",
            "timestamp": 1698000200,
            "status": "failed",
            "fragments": 2,
            "relayed_to": []
        }
    ],
    "total_transactions": 3,
    "pending_count": 1,
    "confirmed_count": 1,
    "failed_count": 1
}"#;
    
    service.create_file("transactions.json", tx_log)?;
    let tx_content = service.read_file("transactions.json")?;
    println!("✅ Transaction log created:");
    println!("{}", tx_content);
    
    // Demo 5: File management
    println!("\n📝 Demo 5: File management");
    println!("-------------------------");
    
    // List all files
    let files = service.list_files()?;
    println!("📋 All files in directory:");
    for file in &files {
        let size = service.get_file_size(file)?;
        let exists = service.file_exists(file);
        println!("  - {} ({} bytes, exists: {})", file, size, exists);
    }
    
    // Demo 6: File updates
    println!("\n📝 Demo 6: File updates");
    println!("----------------------");
    
    // Update the system log with a new entry
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let update_entry = format!("[{}] System update - BLE adapter reconnected", timestamp);
    service.append_to_file("system.log", &update_entry)?;
    
    // Read the updated log
    let updated_log = service.read_file("system.log")?;
    println!("✅ Updated system log:");
    println!("{}", updated_log);
    
    // Demo 7: Error handling
    println!("\n📝 Demo 7: Error handling");
    println!("------------------------");
    
    // Try to read a non-existent file
    match service.read_file("nonexistent.txt") {
        Ok(content) => println!("Unexpectedly read: {}", content),
        Err(e) => println!("✅ Correctly handled missing file: {}", e),
    }
    
    // Try to get size of non-existent file
    match service.get_file_size("nonexistent.txt") {
        Ok(size) => println!("Unexpectedly got size: {}", size),
        Err(e) => println!("✅ Correctly handled missing file size: {}", e),
    }
    
    // Demo 8: Cleanup (optional)
    println!("\n📝 Demo 8: Cleanup");
    println!("-----------------");
    
    println!("Files before cleanup:");
    let files_before = service.list_files()?;
    for file in &files_before {
        println!("  - {}", file);
    }
    
    // Delete one file as an example
    service.delete_file("transactions.json")?;
    
    println!("\nFiles after deleting transactions.json:");
    let files_after = service.list_files()?;
    for file in &files_after {
        println!("  - {}", file);
    }
    
    println!("\n🎉 Demo completed successfully!");
    println!("📁 Check the '{}' directory for the created files.", service.get_base_dir());
    
    Ok(())
}
