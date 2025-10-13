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
    /// Initialize a new PolliNet SDK instance without RPC client
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

    /// Initialize a new PolliNet SDK instance with RPC client for nonce account fetching
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, PolliNetError> {
        // Initialize BLE transport
        let ble_transport = Arc::new(ble::MeshTransport::new().await?);

        // Initialize transaction service with RPC client
        let transaction_service =
            Arc::new(transaction::TransactionService::new_with_rpc(rpc_url).await?);

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

    /// Create an unsigned transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned transaction
    /// Sender is used as nonce authority
    pub async fn create_unsigned_transaction(
        &self,
        sender: &str,
        recipient: &str,
        fee_payer: &str,
        amount: u64,
        nonce_account: &str,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .create_unsigned_transaction(
                sender,
                recipient,
                fee_payer,
                amount,
                nonce_account,
            )
            .await?)
    }
    
    /// Create an unsigned SPL token transfer transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned SPL token transaction
    /// Automatically derives ATAs from wallet pubkeys and mint address
    /// Sender is used as nonce authority
    pub async fn create_unsigned_spl_transaction(
        &self,
        sender_wallet: &str,
        recipient_wallet: &str,
        fee_payer: &str,
        mint_address: &str,
        amount: u64,
        nonce_account: &str,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .create_unsigned_spl_transaction(
                sender_wallet,
                recipient_wallet,
                fee_payer,
                mint_address,
                amount,
                nonce_account,
            )
            .await?)
    }
    
    /// Add a signature to an unsigned transaction (base64 encoded)
    /// Intelligently adds signature based on signer's role
    /// If signer is nonce authority and sender, signature is added for both roles
    /// Returns base64 encoded updated transaction
    pub fn add_signature(
        &self,
        base64_tx: &str,
        signer_pubkey: &solana_sdk::pubkey::Pubkey,
        signature: &solana_sdk::signature::Signature,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .add_signature(base64_tx, signer_pubkey, signature)?)
    }
    
    /// Send and confirm a base64 encoded transaction
    /// Decodes, deserializes, validates, and submits to Solana
    pub async fn send_and_confirm_transaction(
        &self,
        base64_tx: &str,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .send_and_confirm_transaction(base64_tx)
            .await?)
    }
    
    /// Process and relay a presigned custom transaction
    /// Takes a presigned transaction (base64), compresses, fragments, and relays over BLE
    /// Returns transaction ID for tracking
    pub async fn process_and_relay_transaction(
        &self,
        base64_signed_tx: &str,
    ) -> Result<String, PolliNetError> {
        // Process the transaction (compress and fragment)
        let fragments = self
            .transaction_service
            .process_and_relay_transaction(base64_signed_tx)
            .await?;
        
        // Get transaction ID from first fragment
        let tx_id = fragments.first()
            .map(|f| f.id.clone())
            .ok_or_else(|| PolliNetError::Transaction(
                transaction::TransactionError::Serialization("No fragments created".to_string())
            ))?;
        
        // Relay fragments over BLE mesh
        self.ble_transport.relay_fragments(fragments).await?;
        
        Ok(tx_id)
    }
    
    /// Create and sign a new transaction with durable nonce
    /// Creates a presigned transaction using a nonce account for longer lifetime
    pub async fn create_transaction(
        &self,
        sender: &str,
        sender_keypair: &solana_sdk::signature::Keypair,
        recipient: &str,
        amount: u64,
        nonce_account: &str,
        nonce_authority_keypair: &solana_sdk::signature::Keypair,
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self
            .transaction_service
            .create_transaction(
                sender,
                sender_keypair,
                recipient,
                amount,
                nonce_account,
                nonce_authority_keypair,
            )
            .await?)
    }
    
    /// Create and sign a new SPL token transfer transaction with durable nonce
    /// Creates a presigned SPL token transaction using a nonce account for longer lifetime
    /// Automatically derives Associated Token Accounts from wallet pubkeys and mint address
    pub async fn create_spl_transaction(
        &self,
        sender_wallet: &str,
        sender_keypair: &solana_sdk::signature::Keypair,
        recipient_wallet: &str,
        mint_address: &str,
        amount: u64,
        nonce_account: &str,
        nonce_authority_keypair: &solana_sdk::signature::Keypair,
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self
            .transaction_service
            .create_spl_transaction(
                sender_wallet,
                sender_keypair,
                recipient_wallet,
                mint_address,
                amount,
                nonce_account,
                nonce_authority_keypair,
            )
            .await?)
    }

    /// Fragment a transaction for BLE transmission
    pub fn fragment_transaction(&self, compressed_tx: &[u8]) -> Vec<transaction::Fragment> {
        self.transaction_service.fragment_transaction(compressed_tx)
    }

    /// Reassemble fragments back into a complete transaction
    pub fn reassemble_fragments(
        &self,
        fragments: &[transaction::Fragment],
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self.transaction_service.reassemble_fragments(fragments)?)
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
