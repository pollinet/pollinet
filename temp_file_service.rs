// Temporary file service for testing and debugging
// 
// This service provides simple file operations for creating, reading, and writing text files.

use std::fs;
use std::io::{self, Write, Read};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;

/// Error types for file operations
#[derive(Error, Debug)]
pub enum FileServiceError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    
    #[error("File not found: {0}")]
    FileNotFound(String),
    
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// Temporary file service
pub struct TempFileService {
    /// Base directory for temporary files
    base_dir: String,
    /// File locks to prevent concurrent access
    file_locks: Arc<RwLock<std::collections::HashMap<String, Arc<RwLock<()>>>>>,
}

impl TempFileService {
    /// Create a new temporary file service
    pub fn new(base_dir: Option<String>) -> Result<Self, FileServiceError> {
        let base_dir = base_dir.unwrap_or_else(|| "/tmp/pollinet_files".to_string());
        
        // Create base directory if it doesn't exist
        if !Path::new(&base_dir).exists() {
            fs::create_dir_all(&base_dir)
                .map_err(|e| FileServiceError::Io(e))?;
        }
        
        Ok(Self {
            base_dir,
            file_locks: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }
    
    /// Get the full path for a file
    fn get_file_path(&self, filename: &str) -> String {
        format!("{}/{}", self.base_dir, filename)
    }
    
    /// Create a new text file with content
    pub async fn create_file(&self, filename: &str, content: &str) -> Result<(), FileServiceError> {
        let file_path = self.get_file_path(filename);
        
        // Get or create file lock
        let lock = {
            let mut locks = self.file_locks.write().await;
            locks.entry(filename.to_string())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };
        
        let _guard = lock.write().await;
        
        // Create parent directories if needed
        if let Some(parent) = Path::new(&file_path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| FileServiceError::Io(e))?;
        }
        
        // Write content to file
        fs::write(&file_path, content)
            .map_err(|e| FileServiceError::Io(e))?;
        
        println!("✅ Created file: {} with {} bytes", file_path, content.len());
        Ok(())
    }
    
    /// Read content from a text file
    pub async fn read_file(&self, filename: &str) -> Result<String, FileServiceError> {
        let file_path = self.get_file_path(filename);
        
        // Check if file exists
        if !Path::new(&file_path).exists() {
            return Err(FileServiceError::FileNotFound(file_path));
        }
        
        // Get file lock
        let lock = {
            let locks = self.file_locks.read().await;
            locks.get(filename)
                .cloned()
                .unwrap_or_else(|| Arc::new(RwLock::new(())))
        };
        
        let _guard = lock.read().await;
        
        // Read file content
        let content = fs::read_to_string(&file_path)
            .map_err(|e| FileServiceError::Io(e))?;
        
        println!("📖 Read file: {} ({} bytes)", file_path, content.len());
        Ok(content)
    }
    
    /// Append content to a text file
    pub async fn append_to_file(&self, filename: &str, content: &str) -> Result<(), FileServiceError> {
        let file_path = self.get_file_path(filename);
        
        // Get or create file lock
        let lock = {
            let mut locks = self.file_locks.write().await;
            locks.entry(filename.to_string())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };
        
        let _guard = lock.write().await;
        
        // Open file in append mode
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| FileServiceError::Io(e))?;
        
        // Write content
        file.write_all(content.as_bytes())
            .map_err(|e| FileServiceError::Io(e))?;
        
        file.write_all(b"\n")
            .map_err(|e| FileServiceError::Io(e))?;
        
        println!("📝 Appended to file: {} ({} bytes)", file_path, content.len());
        Ok(())
    }
    
    /// Overwrite content in a text file
    pub async fn write_file(&self, filename: &str, content: &str) -> Result<(), FileServiceError> {
        let file_path = self.get_file_path(filename);
        
        // Get or create file lock
        let lock = {
            let mut locks = self.file_locks.write().await;
            locks.entry(filename.to_string())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };
        
        let _guard = lock.write().await;
        
        // Create parent directories if needed
        if let Some(parent) = Path::new(&file_path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| FileServiceError::Io(e))?;
        }
        
        // Write content to file (overwrites existing content)
        fs::write(&file_path, content)
            .map_err(|e| FileServiceError::Io(e))?;
        
        println!("✏️  Wrote to file: {} ({} bytes)", file_path, content.len());
        Ok(())
    }
    
    /// Check if a file exists
    pub async fn file_exists(&self, filename: &str) -> bool {
        let file_path = self.get_file_path(filename);
        Path::new(&file_path).exists()
    }
    
    /// Get file size
    pub async fn get_file_size(&self, filename: &str) -> Result<u64, FileServiceError> {
        let file_path = self.get_file_path(filename);
        
        if !Path::new(&file_path).exists() {
            return Err(FileServiceError::FileNotFound(file_path));
        }
        
        let metadata = fs::metadata(&file_path)
            .map_err(|e| FileServiceError::Io(e))?;
        
        Ok(metadata.len())
    }
    
    /// List all files in the base directory
    pub async fn list_files(&self) -> Result<Vec<String>, FileServiceError> {
        let mut files = Vec::new();
        
        let entries = fs::read_dir(&self.base_dir)
            .map_err(|e| FileServiceError::Io(e))?;
        
        for entry in entries {
            let entry = entry.map_err(|e| FileServiceError::Io(e))?;
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
    pub async fn delete_file(&self, filename: &str) -> Result<(), FileServiceError> {
        let file_path = self.get_file_path(filename);
        
        if !Path::new(&file_path).exists() {
            return Err(FileServiceError::FileNotFound(file_path));
        }
        
        fs::remove_file(&file_path)
            .map_err(|e| FileServiceError::Io(e))?;
        
        // Remove from locks
        {
            let mut locks = self.file_locks.write().await;
            locks.remove(filename);
        }
        
        println!("🗑️  Deleted file: {}", file_path);
        Ok(())
    }
    
    /// Get the base directory path
    pub fn get_base_dir(&self) -> &str {
        &self.base_dir
    }
}

/// Example usage and testing
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Temporary File Service");
    
    // Create service
    let service = TempFileService::new(Some("./temp_files".to_string()))?;
    println!("📁 Base directory: {}", service.get_base_dir());
    
    // Test creating a file
    println!("\n📝 Testing file creation...");
    service.create_file("test.txt", "Hello, World!").await?;
    service.create_file("data.json", r#"{"name": "PolliNet", "version": "1.0"}"#).await?;
    
    // Test reading files
    println!("\n📖 Testing file reading...");
    let content = service.read_file("test.txt").await?;
    println!("Content: {}", content);
    
    let json_content = service.read_file("data.json").await?;
    println!("JSON: {}", json_content);
    
    // Test appending to file
    println!("\n📝 Testing file appending...");
    service.append_to_file("test.txt", "This is appended content").await?;
    service.append_to_file("test.txt", "Another line").await?;
    
    // Read the updated file
    let updated_content = service.read_file("test.txt").await?;
    println!("Updated content:\n{}", updated_content);
    
    // Test file listing
    println!("\n📋 Testing file listing...");
    let files = service.list_files().await?;
    println!("Files in directory: {:?}", files);
    
    // Test file size
    println!("\n📏 Testing file size...");
    for file in &files {
        let size = service.get_file_size(file).await?;
        println!("File '{}': {} bytes", file, size);
    }
    
    // Test overwriting
    println!("\n✏️  Testing file overwriting...");
    service.write_file("test.txt", "This completely replaces the content!").await?;
    let final_content = service.read_file("test.txt").await?;
    println!("Final content: {}", final_content);
    
    // Test file existence
    println!("\n🔍 Testing file existence...");
    println!("test.txt exists: {}", service.file_exists("test.txt").await);
    println!("nonexistent.txt exists: {}", service.file_exists("nonexistent.txt").await);
    
    println!("\n✅ All tests completed successfully!");
    
    Ok(())
}
