//! PolliNet SDK - Decentralized Solana transaction propagation over BLE mesh networks
//! 
//! This SDK enables offline Solana transactions to be distributed opportunistically
//! over Bluetooth Low Energy (BLE) mesh networks, inspired by biological pollination.

pub mod ble;
pub mod nonce;
pub mod transaction;
pub mod util;

use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;

/// Core PolliNet SDK instance using new platform-agnostic BLE adapter
pub struct PolliNetSDK {
    /// BLE adapter bridge for mesh networking
    ble_bridge: Arc<ble::bridge::BleAdapterBridge>,
    /// Transaction builder and manager
    transaction_service: Arc<transaction::TransactionService>,
    /// Nonce account management
    nonce_manager: Arc<nonce::NonceManager>,
    /// Local transaction cache
    local_cache: Arc<RwLock<transaction::TransactionCache>>,
}

impl PolliNetSDK {
    /// Initialize a new PolliNet SDK instance
    pub async fn new() -> Result<Self, PolliNetError> {
        // Create platform-specific BLE adapter
        let ble_adapter = ble::create_ble_adapter().await?;
        
        // Initialize BLE bridge
        let ble_bridge = Arc::new(ble::bridge::BleAdapterBridge::new(ble_adapter).await?);
        
        // Initialize transaction service
        let transaction_service = Arc::new(transaction::TransactionService::new().await?);
        
        // Initialize local cache
        let local_cache = Arc::new(RwLock::new(transaction::TransactionCache::new()));
        
        Ok(Self {
            ble_bridge,
            transaction_service,
            nonce_manager: Arc::new(nonce::NonceManager::new().await?),
            local_cache,
        })
    }
    
    /// Start BLE advertising and networking
    pub async fn start_ble_networking(&self) -> Result<(), PolliNetError> {
        // Start advertising the PolliNet service
        self.ble_bridge.start_advertising(ble::POLLINET_SERVICE_UUID, ble::POLLINET_SERVICE_NAME).await?;
        
        tracing::info!("🚀 PolliNet BLE networking started with new platform-agnostic adapter");
        Ok(())
    }
    
    /// Create and sign a new transaction
    pub async fn create_transaction(
        &self,
        sender: &str,
        recipient: &str,
        amount: u64,
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self
            .transaction_service
            .create_transaction(sender, recipient, amount)
            .await?)
    }

    /// Fragment a transaction for BLE transmission
    pub fn fragment_transaction(&self, compressed_tx: &[u8]) -> Vec<transaction::Fragment> {
        self.transaction_service.fragment_transaction(compressed_tx)
    }

    /// Relay transaction fragments over BLE mesh
    pub async fn relay_transaction(
        &self,
        fragments: Vec<transaction::Fragment>,
    ) -> Result<(), PolliNetError> {
        // Send fragments via the BLE bridge
        self.ble_bridge.send_fragments(fragments).await?;
        Ok(())
    }
    
    /// Submit a transaction to Solana when online
    pub async fn submit_transaction_to_solana(&self, transaction: &[u8]) -> Result<String, PolliNetError> {
        Ok(self.transaction_service.submit_to_solana(transaction).await?)
    }
    
    /// Broadcast confirmation after successful submission
    pub async fn broadcast_confirmation(&self, signature: &str) -> Result<(), PolliNetError> {
        // Send confirmation via BLE
        let confirmation_data = signature.as_bytes();
        // Note: We need to add a send_confirmation method to the bridge
        tracing::info!("📤 Broadcasting confirmation via BLE: {}", signature);
        Ok(())
    }
    
    /// Cast a governance vote (example use case)
    pub async fn cast_vote(&self, proposal_id: &str, choice: u8) -> Result<(), PolliNetError> {
        Ok(self.transaction_service.cast_vote(proposal_id, choice).await?)
    }
    
    /// Discover nearby BLE peers (placeholder - needs implementation in bridge)
    pub async fn discover_ble_peers(&self) -> Result<Vec<ble::PeerInfo>, PolliNetError> {
        // TODO: Implement peer discovery in the bridge
        tracing::info!("🔍 Peer discovery not yet implemented in new BLE system");
        Ok(vec![])
    }
    
    /// Connect to a BLE peer (placeholder - needs implementation in bridge)
    pub async fn connect_to_ble_peer(&self, peer_id: &str) -> Result<(), PolliNetError> {
        // TODO: Implement peer connection in the bridge
        tracing::info!("🔗 Peer connection not yet implemented in new BLE system for peer: {}", peer_id);
        Ok(())
    }
    
    /// Get BLE status and debugging information
    pub async fn get_ble_status(&self) -> Result<String, PolliNetError> {
        let adapter_info = self.ble_bridge.get_adapter_info();
        let fragment_count = self.ble_bridge.get_fragment_count().await;
        
        let status = format!(
            "BLE Status (New Platform-Agnostic System):\n\
             Platform: {}\n\
             Adapter: {}\n\
             Address: {}\n\
             Powered: {}\n\
             Discoverable: {}\n\
             Advertising: {}\n\
             Fragments in buffer: {}",
            adapter_info.platform,
            adapter_info.name,
            adapter_info.address,
            adapter_info.powered,
            adapter_info.discoverable,
            self.ble_bridge.is_advertising(),
            fragment_count
        );
        
        Ok(status)
    }
    
    /// Scan for ALL BLE devices (placeholder - needs implementation in bridge)
    pub async fn scan_all_devices(&self) -> Result<Vec<String>, PolliNetError> {
        // TODO: Implement device scanning in the bridge
        tracing::info!("🔍 Device scanning not yet implemented in new BLE system");
        Ok(vec![])
    }
}

/// Error types for PolliNet operations
#[derive(Error, Debug)]
pub enum PolliNetError {
    #[error("BLE adapter error: {0}")]
    BleAdapter(#[from] ble::adapter::BleError),
    
    #[error("Transaction error: {0}")]
    Transaction(#[from] transaction::TransactionError),
    
    #[error("Nonce management error: {0}")]
    Nonce(#[from] nonce::NonceError),
    
    #[error("Solana RPC error: {0}")]
    SolanaRpc(String),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
}



/// Service UUID for BLE mesh networking
pub const SERVICE_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7";

/// BLE MTU size for safe packet transmission
pub const BLE_MTU_SIZE: usize = 480;

/// Compression threshold in bytes
pub const COMPRESSION_THRESHOLD: usize = 100;
