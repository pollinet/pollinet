//! PolliNet SDK - Decentralized Solana transaction propagation over BLE mesh networks
//!
//! This SDK enables offline Solana transactions to be distributed opportunistically
//! over Bluetooth Low Energy (BLE) mesh networks, inspired by biological pollination.

pub mod ble;
pub mod nonce;
pub mod transaction;
pub mod util;

use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Core PolliNet SDK instance
pub struct PolliNetSDK {
    /// BLE transport layer for mesh networking
    ble_transport: Arc<ble::MeshTransport>,
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
        // Initialize BLE transport
        let ble_transport = Arc::new(ble::MeshTransport::new().await?);

        // Initialize transaction service
        let transaction_service = Arc::new(transaction::TransactionService::new().await?);

        // Initialize local cache
        let local_cache = Arc::new(RwLock::new(transaction::TransactionCache::new()));

        Ok(Self {
            ble_transport,
            transaction_service,
            nonce_manager: Arc::new(nonce::NonceManager::new().await?),
            local_cache,
        })
    }

    /// Start BLE advertising and scanning
    pub async fn start_ble_networking(&self) -> Result<(), PolliNetError> {
        self.ble_transport.start_advertising().await?;
        self.ble_transport.start_scanning().await?;
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
        Ok(self.ble_transport.relay_fragments(fragments).await?)
    }

    /// Submit a transaction to Solana when online
    pub async fn submit_transaction_to_solana(
        &self,
        transaction: &[u8],
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .submit_to_solana(transaction)
            .await?)
    }

    /// Broadcast confirmation after successful submission
    pub async fn broadcast_confirmation(&self, signature: &str) -> Result<(), PolliNetError> {
        Ok(self
            .transaction_service
            .broadcast_confirmation(signature)
            .await?)
    }

    /// Cast a governance vote (example use case)
    pub async fn cast_vote(&self, proposal_id: &str, choice: u8) -> Result<(), PolliNetError> {
        Ok(self
            .transaction_service
            .cast_vote(proposal_id, choice)
            .await?)
    }

    /// Discover nearby BLE peers
    pub async fn discover_ble_peers(&self) -> Result<Vec<ble::PeerInfo>, PolliNetError> {
        Ok(self.ble_transport.discover_peers().await?)
    }

    /// Connect to a BLE peer
    pub async fn connect_to_ble_peer(&self, peer_id: &str) -> Result<(), PolliNetError> {
        Ok(self.ble_transport.connect_to_peer(peer_id).await?)
    }

    /// Get BLE status and debugging information
    pub async fn get_ble_status(&self) -> Result<String, PolliNetError> {
        Ok(self.ble_transport.get_ble_status().await?)
    }

    /// Scan for ALL BLE devices (for debugging)
    pub async fn scan_all_devices(&self) -> Result<Vec<String>, PolliNetError> {
        Ok(self.ble_transport.scan_all_devices().await?)
    }
}

/// Error types for PolliNet operations
#[derive(Error, Debug)]
pub enum PolliNetError {
    #[error("BLE transport error: {0}")]
    BleTransport(#[from] ble::BleError),

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
pub const SERVICE_UUID: &str = "12345678-1234-1234-1234-123456789abc";

/// BLE MTU size for safe packet transmission
pub const BLE_MTU_SIZE: usize = 480;

/// Compression threshold in bytes
pub const COMPRESSION_THRESHOLD: usize = 100;
