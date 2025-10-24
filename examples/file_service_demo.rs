//! Demo of the temporary file service
//! 
//! This example shows how to use the temporary file service for creating,
//! reading, and writing text files.

use std::time::Duration;
use tokio::time::sleep;

// Include the temp file service
mod temp_file_service {
    include!("../temp_file_service.rs");
}

use temp_file_service::{TempFileService, FileServiceError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 PolliNet Temporary File Service Demo");
    println!("=====================================");
    
    // Create the file service
    let service = TempFileService::new(Some("./demo_files".to_string()))?;
    println!("📁 Using directory: {}", service.get_base_dir());
    
    // Demo 1: Create and read a simple text file
    println!("\n📝 Demo 1: Creating and reading a text file");
    println!("--------------------------------------------");
    
    let initial_content = "Welcome to PolliNet!\nThis is a test file created by the temporary file service.";
    service.create_file("welcome.txt", initial_content).await?;
    
    let read_content = service.read_file("welcome.txt").await?;
    println!("✅ Successfully read file:");
    println!("{}", read_content);
    
    // Demo 2: Append to the file
    println!("\n📝 Demo 2: Appending to the file");
    println!("--------------------------------");
    
    service.append_to_file("welcome.txt", "This line was appended!").await?;
    service.append_to_file("welcome.txt", "And this is another appended line.").await?;
    
    let appended_content = service.read_file("welcome.txt").await?;
    println!("✅ File after appending:");
    println!("{}", appended_content);
    
    // Demo 3: Create a JSON configuration file
    println!("\n📝 Demo 3: Creating a JSON configuration file");
    println!("---------------------------------------------");
    
    let config_json = r#"{
    "pollinet_config": {
        "version": "1.0.0",
        "ble_settings": {
            "service_uuid": "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7",
            "advertising_interval": 1000,
            "scan_timeout": 5000
        },
        "network_settings": {
            "max_peers": 10,
            "relay_timeout": 30000
        }
    }
}"#;
    
    service.create_file("config.json", config_json).await?;
    let config_content = service.read_file("config.json").await?;
    println!("✅ Created configuration file:");
    println!("{}", config_content);
    
    // Demo 4: Create a log file with timestamps
    println!("\n📝 Demo 4: Creating a log file with timestamps");
    println!("---------------------------------------------");
    
    for i in 1..=5 {
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let log_entry = format!("[{}] Log entry #{} - System status: OK", timestamp, i);
        service.append_to_file("system.log", &log_entry).await?;
        
        // Simulate some delay between log entries
        sleep(Duration::from_millis(100)).await;
    }
    
    let log_content = service.read_file("system.log").await?;
    println!("✅ Created log file with entries:");
    println!("{}", log_content);
    
    // Demo 5: File management operations
    println!("\n📝 Demo 5: File management operations");
    println!("------------------------------------");
    
    // List all files
    let files = service.list_files().await?;
    println!("📋 Files in directory:");
    for file in &files {
        let size = service.get_file_size(file).await?;
        let exists = service.file_exists(file).await;
        println!("  - {} ({} bytes, exists: {})", file, size, exists);
    }
    
    // Demo 6: Overwrite a file
    println!("\n📝 Demo 6: Overwriting a file");
    println!("----------------------------");
    
    let new_content = "This completely replaces the previous content!\nThe old content is gone.";
    service.write_file("welcome.txt", new_content).await?;
    
    let overwritten_content = service.read_file("welcome.txt").await?;
    println!("✅ File after overwriting:");
    println!("{}", overwritten_content);
    
    // Demo 7: Error handling
    println!("\n📝 Demo 7: Error handling");
    println!("------------------------");
    
    match service.read_file("nonexistent.txt").await {
        Ok(content) => println!("Unexpectedly read content: {}", content),
        Err(FileServiceError::FileNotFound(path)) => {
            println!("✅ Correctly handled missing file: {}", path);
        }
        Err(e) => println!("❌ Unexpected error: {}", e),
    }
    
    // Demo 8: Cleanup (optional)
    println!("\n📝 Demo 8: Cleanup");
    println!("-----------------");
    
    println!("Files before cleanup:");
    let files_before = service.list_files().await?;
    for file in &files_before {
        println!("  - {}", file);
    }
    
    // Delete one file as an example
    service.delete_file("welcome.txt").await?;
    
    println!("\nFiles after deleting welcome.txt:");
    let files_after = service.list_files().await?;
    for file in &files_after {
        println!("  - {}", file);
    }
    
    println!("\n🎉 Demo completed successfully!");
    println!("📁 Check the '{}' directory for the created files.", service.get_base_dir());
    
    Ok(())
}
