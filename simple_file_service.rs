// Simple file service for testing and debugging
// 
// This service provides simple synchronous file operations for creating, reading, and writing text files.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Simple file service
#[derive(Debug)]
pub struct SimpleFileService {
    /// Base directory for files
    base_dir: String,
}

impl SimpleFileService {
    /// Create a new simple file service
    pub fn new(base_dir: Option<String>) -> Result<Self, io::Error> {
        let base_dir = base_dir.unwrap_or_else(|| "/tmp/pollinet_files".to_string());
        
        // Create base directory if it doesn't exist
        if !Path::new(&base_dir).exists() {
            fs::create_dir_all(&base_dir)?;
        }
        
        Ok(Self { base_dir })
    }
    
    /// Get the full path for a file
    fn get_file_path(&self, filename: &str) -> String {
        format!("{}/{}", self.base_dir, filename)
    }
    
    /// Create a new text file with content
    pub fn create_file(&self, filename: &str, content: &str) -> Result<(), io::Error> {
        let file_path = self.get_file_path(filename);
        
        // Create parent directories if needed
        if let Some(parent) = Path::new(&file_path).parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Write content to file
        fs::write(&file_path, content)?;
        
        println!("✅ Created file: {} with {} bytes", file_path, content.len());
        Ok(())
    }
    
    /// Read content from a text file
    pub fn read_file(&self, filename: &str) -> Result<String, io::Error> {
        let file_path = self.get_file_path(filename);
        
        // Read file content
        let content = fs::read_to_string(&file_path)?;
        
        println!("📖 Read file: {} ({} bytes)", file_path, content.len());
        Ok(content)
    }
    
    /// Append content to a text file
    pub fn append_to_file(&self, filename: &str, content: &str) -> Result<(), io::Error> {
        let file_path = self.get_file_path(filename);
        
        // Open file in append mode
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        
        // Write content
        file.write_all(content.as_bytes())?;
        file.write_all(b"\n")?;
        
        println!("📝 Appended to file: {} ({} bytes)", file_path, content.len());
        Ok(())
    }
    
    /// Overwrite content in a text file
    pub fn write_file(&self, filename: &str, content: &str) -> Result<(), io::Error> {
        let file_path = self.get_file_path(filename);
        
        // Create parent directories if needed
        if let Some(parent) = Path::new(&file_path).parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Write content to file (overwrites existing content)
        fs::write(&file_path, content)?;
        
        println!("✏️  Wrote to file: {} ({} bytes)", file_path, content.len());
        Ok(())
    }
    
    /// Check if a file exists
    pub fn file_exists(&self, filename: &str) -> bool {
        let file_path = self.get_file_path(filename);
        Path::new(&file_path).exists()
    }
    
    /// Get file size
    pub fn get_file_size(&self, filename: &str) -> Result<u64, io::Error> {
        let file_path = self.get_file_path(filename);
        let metadata = fs::metadata(&file_path)?;
        Ok(metadata.len())
    }
    
    /// List all files in the base directory
    pub fn list_files(&self) -> Result<Vec<String>, io::Error> {
        let mut files = Vec::new();
        
        let entries = fs::read_dir(&self.base_dir)?;
        
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(filename) = path.file_name() {
                    if let Some(name) = filename.to_str() {
                        files.push(name.to_string());
                    }
                }
            }
        }
        
        files.sort();
        Ok(files)
    }
    
    /// Delete a file
    pub fn delete_file(&self, filename: &str) -> Result<(), io::Error> {
        let file_path = self.get_file_path(filename);
        fs::remove_file(&file_path)?;
        println!("🗑️  Deleted file: {}", file_path);
        Ok(())
    }
    
    /// Get the base directory path
    pub fn get_base_dir(&self) -> &str {
        &self.base_dir
    }
}

/// Example usage and testing
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Simple File Service");
    
    // Create service
    let service = SimpleFileService::new(Some("./temp_files".to_string()))?;
    println!("📁 Base directory: {}", service.get_base_dir());
    
    // Test creating a file
    println!("\n📝 Testing file creation...");
    service.create_file("test.txt", "Hello, World!")?;
    service.create_file("data.json", r#"{"name": "PolliNet", "version": "1.0"}"#)?;
    
    // Test reading files
    println!("\n📖 Testing file reading...");
    let content = service.read_file("test.txt")?;
    println!("Content: {}", content);
    
    let json_content = service.read_file("data.json")?;
    println!("JSON: {}", json_content);
    
    // Test appending to file
    println!("\n📝 Testing file appending...");
    service.append_to_file("test.txt", "This is appended content")?;
    service.append_to_file("test.txt", "Another line")?;
    
    // Read the updated file
    let updated_content = service.read_file("test.txt")?;
    println!("Updated content:\n{}", updated_content);
    
    // Test file listing
    println!("\n📋 Testing file listing...");
    let files = service.list_files()?;
    println!("Files in directory: {:?}", files);
    
    // Test file size
    println!("\n📏 Testing file size...");
    for file in &files {
        let size = service.get_file_size(file)?;
        println!("File '{}': {} bytes", file, size);
    }
    
    // Test overwriting
    println!("\n✏️  Testing file overwriting...");
    service.write_file("test.txt", "This completely replaces the content!")?;
    let final_content = service.read_file("test.txt")?;
    println!("Final content: {}", final_content);
    
    // Test file existence
    println!("\n🔍 Testing file existence...");
    println!("test.txt exists: {}", service.file_exists("test.txt"));
    println!("nonexistent.txt exists: {}", service.file_exists("nonexistent.txt"));
    
    // Test error handling
    println!("\n❌ Testing error handling...");
    match service.read_file("nonexistent.txt") {
        Ok(content) => println!("Unexpectedly read content: {}", content),
        Err(e) => println!("✅ Correctly handled missing file: {}", e),
    }
    
    println!("\n✅ All tests completed successfully!");
    
    Ok(())
}
