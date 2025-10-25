//! Bridge between the new platform-agnostic BLE adapter and PolliNet functionality

use std::sync::Arc;
use tokio::sync::RwLock;
use crate::ble::adapter::{BleAdapter, BleError};
use crate::transaction::Fragment;

/// Bridge that connects the new BleAdapter to PolliNet's transaction processing
pub struct BleAdapterBridge {
    /// Platform-specific BLE adapter
    adapter: Box<dyn BleAdapter>,
    /// Fragment reassembly buffer
    fragment_buffer: Arc<RwLock<Vec<Fragment>>>,
    /// Transaction cache for reassembly
    transaction_cache: Arc<RwLock<std::collections::HashMap<String, Vec<Fragment>>>>,
    /// Text message buffer for incoming messages
    text_message_buffer: Arc<RwLock<Vec<String>>>,
}

impl BleAdapterBridge {
    /// Create a new bridge with the platform-specific adapter
    pub async fn new(adapter: Box<dyn BleAdapter>) -> Result<Self, BleError> {
        let bridge = Self {
            adapter,
            fragment_buffer: Arc::new(RwLock::new(Vec::new())),
            transaction_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
            text_message_buffer: Arc::new(RwLock::new(Vec::new())),
        };
        
        // Set up receive callback to handle incoming fragments
        bridge.setup_receive_callback().await?;
        
        Ok(bridge)
    }
    
    /// Set up the receive callback to process incoming transaction fragments and text messages
    async fn setup_receive_callback(&self) -> Result<(), BleError> {
        let buffer = Arc::clone(&self.fragment_buffer);
        let cache = Arc::clone(&self.transaction_cache);
        let text_buffer = Arc::clone(&self.text_message_buffer);
        
        self.adapter.on_receive(Box::new(move |data| {
            // Try to parse as fragment first
            if let Ok(fragment) = serde_json::from_slice::<Fragment>(&data) {
                let fragment_id = fragment.id.clone();
                let fragment_index = fragment.index;
                let fragment_total = fragment.total;
                
                tracing::info!("📨 Received fragment {} of {} for transaction {}", 
                    fragment_index + 1, fragment_total, fragment_id);
                
                // Add to reassembly buffer
                let buffer_clone = Arc::clone(&buffer);
                let cache_clone = Arc::clone(&cache);
                
                tokio::spawn(async move {
                    let mut buffer_guard = buffer_clone.write().await;
                    buffer_guard.push(fragment.clone());
                    
                    // Check if we have all fragments for this transaction
                    let mut cache_guard = cache_clone.write().await;
                    let transaction_fragments = cache_guard.entry(fragment.id.clone()).or_insert_with(Vec::new);
                    transaction_fragments.push(fragment);
                    
                    // TODO: Check if transaction is complete and trigger processing
                    if transaction_fragments.len() == fragment_total {
                        tracing::info!("✅ Complete transaction {} received via BLE", fragment_id);
                        // TODO: Process complete transaction
                    }
                });
            } else {
                // Try to parse as text message
                if let Ok(text_message) = String::from_utf8(data) {
                    tracing::info!("📝 Received text message: {}", text_message);
                    
                    let text_buffer_clone = Arc::clone(&text_buffer);
                    tokio::spawn(async move {
                        let mut buffer_guard = text_buffer_clone.write().await;
                        buffer_guard.push(text_message);
                    });
                } else {
                    tracing::warn!("⚠️ Failed to deserialize data as fragment or text message");
                }
            }
        }));
        
        Ok(())
    }
    
    /// Start advertising and networking
    pub async fn start_advertising(&self, service_uuid: &str, service_name: &str) -> Result<(), BleError> {
        self.adapter.start_advertising(service_uuid, service_name).await
    }
    
    /// Send transaction fragments
    pub async fn send_fragments(&self, fragments: Vec<Fragment>) -> Result<(), BleError> {
        for fragment in fragments {
            let data = serde_json::to_vec(&fragment)
                .map_err(|e| BleError::Serialization(e.to_string()))?;
            
            self.adapter.send_packet(&data).await?;
        }
        Ok(())
    }
    
    /// Get the number of fragments waiting for reassembly
    pub async fn get_fragment_count(&self) -> usize {
        self.fragment_buffer.read().await.len()
    }
    
    /// Check if currently scanning
    pub fn is_scanning(&self) -> bool {
        // Note: This would need to be implemented in the BleAdapter trait
        // For now, we'll return false as a placeholder
        false
    }
    
    /// Get adapter information
    pub fn get_adapter_info(&self) -> crate::ble::adapter::AdapterInfo {
        self.adapter.get_adapter_info()
    }
    
    /// Check if advertising
    pub fn is_advertising(&self) -> bool {
        self.adapter.is_advertising()
    }
    
    /// Start scanning for nearby BLE devices
    pub async fn start_scanning(&self) -> Result<(), BleError> {
        self.adapter.start_scanning().await
    }
    
    /// Stop scanning for BLE devices
    pub async fn stop_scanning(&self) -> Result<(), BleError> {
        self.adapter.stop_scanning().await
    }
    
    /// Get list of discovered BLE devices
    pub async fn get_discovered_devices(&self) -> Result<Vec<crate::ble::adapter::DiscoveredDevice>, BleError> {
        self.adapter.get_discovered_devices().await
    }
    
    /// Get connected clients count
    pub async fn connected_clients_count(&self) -> usize {
        self.adapter.connected_clients_count().await
    }
    
    /// Connect to a discovered BLE device
    pub async fn connect_to_device(&self, address: &str) -> Result<(), BleError> {
        self.adapter.connect_to_device(address).await
    }
    
    /// Write data to a connected device
    pub async fn write_to_device(&self, address: &str, data: &[u8]) -> Result<(), BleError> {
        self.adapter.write_to_device(address, data).await
    }
    
    /// Get incoming text messages
    pub async fn get_text_messages(&self) -> Vec<String> {
        let mut buffer_guard = self.text_message_buffer.write().await;
        let messages = buffer_guard.clone();
        buffer_guard.clear();
        messages
    }
    
    /// Check if there are any pending text messages
    pub async fn has_text_messages(&self) -> bool {
        !self.text_message_buffer.read().await.is_empty()
    }
    
    /// Send a text message to connected devices
    pub async fn send_text_message(&self, message: &str) -> Result<(), BleError> {
        let data = message.as_bytes();
        self.adapter.send_packet(data).await
    }
}
