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
}

impl BleAdapterBridge {
    /// Create a new bridge with the platform-specific adapter
    pub async fn new(adapter: Box<dyn BleAdapter>) -> Result<Self, BleError> {
        let bridge = Self {
            adapter,
            fragment_buffer: Arc::new(RwLock::new(Vec::new())),
            transaction_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        };
        
        // Set up receive callback to handle incoming fragments
        bridge.setup_receive_callback().await?;
        
        Ok(bridge)
    }
    
    /// Set up the receive callback to process incoming transaction fragments
    async fn setup_receive_callback(&self) -> Result<(), BleError> {
        let buffer = Arc::clone(&self.fragment_buffer);
        let cache = Arc::clone(&self.transaction_cache);
        
        self.adapter.on_receive(Box::new(move |data| {
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
                tracing::warn!("⚠️ Failed to deserialize fragment data");
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
    pub fn connected_clients_count(&self) -> usize {
        self.adapter.connected_clients_count()
    }
}
