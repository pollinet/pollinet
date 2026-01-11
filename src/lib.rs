//! PolliNet SDK - Decentralized Solana transaction propagation over BLE mesh networks
//!
//! This SDK enables offline Solana transactions to be distributed opportunistically
//! over Bluetooth Low Energy (BLE) mesh networks, inspired by biological pollination.

pub mod ble;
pub mod nonce;
pub mod queue;
pub mod storage;
pub mod transaction;
pub mod util;

#[cfg(feature = "android")]
pub mod ffi;

use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Format for BLE device discovery results
#[derive(Debug, Clone, Copy)]
pub enum DiscoveryFormat {
    /// Return detailed PeerInfo objects
    PeerInfo,
    /// Return just device addresses as strings
    Addresses,
}

/// Result of BLE device discovery
#[derive(Debug)]
pub enum DiscoveryResult {
    /// Detailed peer information
    PeerInfo(Vec<ble::PeerInfo>),
    /// Device addresses only
    Addresses(Vec<String>),
}

/// Trait for transaction input types that can be submitted to Solana
/// Allows unified `submit_transaction()` method to accept both base64 strings and raw bytes
pub trait TransactionInput {
    async fn submit(&self, sdk: &PolliNetSDK) -> Result<String, PolliNetError>;
}

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
    /// Currently connected peer address (for central mode)
    connected_peer: Arc<RwLock<Option<String>>>,
    /// Queue manager for all queue operations (Phase 2)
    queue_manager: Arc<queue::QueueManager>,
}

impl PolliNetSDK {
    /// Initialize a new PolliNet SDK instance without RPC client
    pub async fn new() -> Result<Self, PolliNetError> {
        // Create platform-specific BLE adapter
        let ble_adapter = ble::create_ble_adapter().await?;
        
        // Initialize BLE bridge
        let ble_bridge = Arc::new(ble::bridge::BleAdapterBridge::new(ble_adapter).await?);
        
        // Initialize transaction service
        let transaction_service = Arc::new(transaction::TransactionService::new().await?);

        // Initialize local cache
        let local_cache = Arc::new(RwLock::new(transaction::TransactionCache::new()));
        
        // Initialize queue manager with persistence (Phase 5)
        // Use default app data directory for queue storage
        let queue_manager = if let Ok(storage_dir) = std::env::var("POLLINET_QUEUE_STORAGE") {
            tracing::info!("Using persistent queue storage: {}", storage_dir);
            Arc::new(queue::QueueManager::with_storage(storage_dir)
                .unwrap_or_else(|e| {
                    tracing::warn!("Failed to load queues from storage: {}, starting fresh", e);
                    queue::QueueManager::new()
                }))
        } else {
            tracing::info!("No persistent storage configured, queues will not persist");
            Arc::new(queue::QueueManager::new())
        };
        
        Ok(Self {
            ble_bridge,
            transaction_service,
            nonce_manager: Arc::new(nonce::NonceManager::new().await?),
            local_cache,
            connected_peer: Arc::new(RwLock::new(None)),
            queue_manager,
        })
    }

    /// Initialize a new PolliNet SDK instance with RPC client for nonce account fetching
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, PolliNetError> {
        // Create platform-specific BLE adapter
        let ble_adapter = ble::create_ble_adapter().await?;
        
        // Initialize BLE bridge
        let ble_bridge = Arc::new(ble::bridge::BleAdapterBridge::new(ble_adapter).await?);


        // Initialize transaction service with RPC client
        let transaction_service =
            Arc::new(transaction::TransactionService::new_with_rpc(rpc_url).await?);

        // Initialize local cache
        let local_cache = Arc::new(RwLock::new(transaction::TransactionCache::new()));
        
        // Initialize queue manager with persistence (Phase 5)
        let queue_manager = if let Ok(storage_dir) = std::env::var("POLLINET_QUEUE_STORAGE") {
            tracing::info!("Using persistent queue storage: {}", storage_dir);
            Arc::new(queue::QueueManager::with_storage(storage_dir)
                .unwrap_or_else(|e| {
                    tracing::warn!("Failed to load queues from storage: {}, starting fresh", e);
                    queue::QueueManager::new()
                }))
        } else {
            tracing::info!("No persistent storage configured, queues will not persist");
            Arc::new(queue::QueueManager::new())
        };
        
        Ok(Self {
            ble_bridge,
            transaction_service,
            nonce_manager: Arc::new(nonce::NonceManager::new().await?),
            local_cache,
            connected_peer: Arc::new(RwLock::new(None)),
            queue_manager,
        })
    }
    
    // =========================================================================
    // Queue Management Methods (Phase 2)
    // =========================================================================
    
    /// Get queue manager reference
    pub fn queue_manager(&self) -> &Arc<queue::QueueManager> {
        &self.queue_manager
    }
    
    /// Get queue metrics
    pub async fn get_queue_metrics(&self) -> queue::QueueMetrics {
        self.queue_manager.get_metrics().await
    }
    
    /// Get queue health status
    pub async fn get_queue_health(&self) -> queue::HealthStatus {
        self.queue_manager.get_health().await
    }

    /// Start BLE advertising and scanning
    pub async fn start_ble_networking(&self) -> Result<(), PolliNetError> {
        // Start advertising the PolliNet service
        self.ble_bridge.start_advertising(ble::POLLINET_SERVICE_UUID, ble::POLLINET_SERVICE_NAME).await?;
        
        tracing::info!("🚀 PolliNet BLE networking started with new platform-agnostic adapter");
        Ok(())
    }

    /// Create an unsigned transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned transaction
    /// Sender is used as nonce authority
    /// 
    /// If `nonce_data` is provided, it will be used directly (no RPC call).
    /// Otherwise, if `nonce_account` is provided, it will fetch the nonce data from blockchain.
    pub async fn create_unsigned_transaction(
        &self,
        sender: &str,
        recipient: &str,
        fee_payer: &str,
        amount: u64,
        nonce_account: Option<&str>,
        nonce_data: Option<&transaction::CachedNonceData>,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .create_unsigned_transaction(
                sender,
                recipient,
                fee_payer,
                amount,
                nonce_account,
                nonce_data,
            )
            .await?)
    }
    
    /// Create an unsigned SPL token transfer transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned SPL token transaction
    /// Automatically derives ATAs from wallet pubkeys and mint address
    /// Sender is used as nonce authority
    /// 
    /// If `nonce_data` is provided, it will be used directly (no RPC call).
    /// Otherwise, if `nonce_account` is provided, it will fetch the nonce data from blockchain.
    pub async fn create_unsigned_spl_transaction(
        &self,
        sender_wallet: &str,
        recipient_wallet: &str,
        fee_payer: &str,
        mint_address: &str,
        amount: u64,
        nonce_account: Option<&str>,
        nonce_data: Option<&transaction::CachedNonceData>,
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
                nonce_data,
            )
            .await?)
    }
    
    /// Create an UNSIGNED offline SPL token transfer transaction using cached nonce data
    /// This variant is designed for MWA/Seed Vault workflows where private keys do not
    /// leave the device and nonce/blockhash data comes from an offline bundle.
    ///
    /// Returns base64-encoded unsigned transaction suitable for MWA signing.
    pub fn create_unsigned_offline_spl_transaction(
        &self,
        sender_wallet: &str,
        recipient_wallet: &str,
        fee_payer: &str,
        mint_address: &str,
        amount: u64,
        cached_nonce: &transaction::CachedNonceData,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .create_unsigned_offline_spl_transaction(
                sender_wallet,
                recipient_wallet,
                fee_payer,
                mint_address,
                amount,
                cached_nonce,
            )?)
    }
    
    /// Create an unsigned governance vote transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned vote transaction
    /// Voter is used as nonce authority
    /// 
    /// If `nonce_data` is provided, it will be used directly (no RPC call).
    /// Otherwise, if `nonce_account` is provided, it will fetch the nonce data from blockchain.
    pub async fn cast_unsigned_vote(
        &self,
        voter: &str,
        proposal_id: &str,
        vote_account: &str,
        choice: u8,
        fee_payer: &str,
        nonce_account: Option<&str>,
        nonce_data: Option<&transaction::CachedNonceData>,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .cast_unsigned_vote(
                voter,
                proposal_id,
                vote_account,
                choice,
                fee_payer,
                nonce_account,
                nonce_data,
            )
            .await?)
    }
    
    /// Prepare offline nonce data for creating transactions without internet
    /// Fetches and caches nonce account data that can be used offline
    /// 
    /// Call this while online to prepare for offline transaction creation
    /// Returns CachedNonceData that can be saved and used offline
    pub async fn prepare_offline_nonce_data(
        &self,
        nonce_account: &str,
    ) -> Result<transaction::CachedNonceData, PolliNetError> {
        Ok(self
            .transaction_service
            .prepare_offline_nonce_data(nonce_account)
            .await?)
    }
    
    /// Get an available nonce account from cached bundle
    /// 
    /// Loads the bundle from the specified file path and returns the first
    /// available (unused) nonce account data.
    /// 
    /// This allows users to either manage their own nonce accounts or let
    /// PolliNet manage them automatically.
    /// 
    /// Returns None if:
    /// - Bundle file doesn't exist
    /// - Bundle has no available nonces (all are used)
    pub fn get_available_nonce_from_bundle(
        &self,
        bundle_file: &str,
    ) -> Result<Option<transaction::CachedNonceData>, PolliNetError> {
        Ok(self
            .transaction_service
            .get_available_nonce_from_bundle(bundle_file)?)
    }
    
    /// Prepare multiple nonce accounts for offline use
    /// Smart bundle management: refreshes used nonces (FREE!), creates new ones only when necessary
    /// 
    /// COST OPTIMIZATION:
    /// - Refreshes used/advanced nonces by fetching new blockhash (FREE!)
    /// - Only creates NEW nonce accounts if total < count (~$0.20 each)
    /// - Saves money by reusing existing nonce accounts
    /// 
    /// If bundle_file exists:
    ///   - Loads existing bundle
    ///   - Refreshes used nonces (fetches new blockhash from advanced nonces) - FREE!
    ///   - Creates additional accounts ONLY if total < count
    ///   - Returns bundle with 'count' nonces ready to use
    /// If bundle_file doesn't exist:
    ///   - Creates new bundle with 'count' nonce accounts
    /// 
    /// Example:
    /// ```rust,no_run
    /// # use pollinet::PolliNetSDK;
    /// # use solana_sdk::signature::Keypair;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sdk = PolliNetSDK::new_with_rpc("https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6").await?;
    /// let sender_keypair = Keypair::new();
    /// 
    /// // First time: Creates 10 new nonce accounts (~$2.00)
    /// let bundle = sdk.prepare_offline_bundle(10, &sender_keypair, Some("bundle.json")).await?;
    /// bundle.save_to_file("bundle.json")?;
    /// 
    /// // After using 7 nonces: Refreshes 7 used nonces (FREE!), creates 0 new
    /// let bundle = sdk.prepare_offline_bundle(10, &sender_keypair, Some("bundle.json")).await?;
    /// // Cost: $0.00! Saved $1.40 by refreshing instead of creating new!
    /// # Ok(())
    /// # }
    /// ```
    pub async fn prepare_offline_bundle(
        &self,
        count: usize,
        sender_keypair: &solana_sdk::signature::Keypair,
        bundle_file: Option<&str>,
    ) -> Result<transaction::OfflineTransactionBundle, PolliNetError> {
        Ok(self
            .transaction_service
            .prepare_offline_bundle(count, sender_keypair, bundle_file)
            .await?)
    }
    
    /// Create transaction completely offline using cached nonce data
    /// NO internet connection required - all data comes from cached_nonce
    /// 
    /// Returns compressed transaction bytes ready for BLE transmission
    pub fn create_offline_transaction(
        &self,
        sender_keypair: &solana_sdk::signature::Keypair,
        recipient: &str,
        amount: u64,
        nonce_authority_keypair: &solana_sdk::signature::Keypair,
        cached_nonce: &transaction::CachedNonceData,
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self
            .transaction_service
            .create_offline_transaction(
                sender_keypair,
                recipient,
                amount,
                nonce_authority_keypair,
                cached_nonce,
            )?)
    }
    
    /// Create UNSIGNED offline transaction for MWA (Mobile Wallet Adapter) signing
    /// 
    /// This is the MWA-compatible version that takes PUBLIC KEYS only (no private keys).
    /// Perfect for Solana Mobile Stack integration where private keys never leave Seed Vault.
    /// 
    /// Flow:
    /// 1. Create unsigned transaction with this method (public keys only)
    /// 2. Pass to MWA for signing in Seed Vault (secure hardware)
    /// 3. Submit signed transaction to blockchain
    /// 
    /// Returns base64-encoded unsigned transaction
    pub fn create_unsigned_offline_transaction(
        &self,
        sender_pubkey: &str,
        recipient: &str,
        amount: u64,
        nonce_authority_pubkey: &str,
        cached_nonce: &transaction::CachedNonceData,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .create_unsigned_offline_transaction(
                sender_pubkey,
                recipient,
                amount,
                nonce_authority_pubkey,
                cached_nonce,
            )?)
    }
    
    /// Get transaction message bytes that need to be signed
    /// 
    /// Extracts the raw message from an unsigned transaction for MWA signing.
    /// MWA/Seed Vault will sign these bytes securely.
    /// 
    /// Returns message bytes to sign
    pub fn get_transaction_message_to_sign(
        &self,
        base64_tx: &str,
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self
            .transaction_service
            .get_transaction_message_to_sign(base64_tx)?)
    }
    
    /// Get list of public keys that need to sign this transaction
    /// 
    /// Returns signers in the order required by Solana protocol.
    /// Useful for MWA authorization requests.
    /// 
    /// Returns vector of public key strings (base58)
    pub fn get_required_signers(
        &self,
        base64_tx: &str,
    ) -> Result<Vec<String>, PolliNetError> {
        Ok(self
            .transaction_service
            .get_required_signers(base64_tx)?)
    }
    
    /// Submit offline-created transaction to blockchain
    /// Optionally verifies nonce is still valid before submission
    /// 
    /// Returns transaction signature if successful
    pub async fn submit_offline_transaction(
        &self,
        compressed_tx: &[u8],
        verify_nonce: bool,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .submit_offline_transaction(compressed_tx, verify_nonce)
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
    
    /// Submit a transaction to Solana
    /// 
    /// Unified method that accepts either base64-encoded transaction strings or raw transaction bytes.
    /// Automatically detects the input format and processes accordingly.
    /// 
    /// - **Base64 string** (`&str` or `String`): Decodes, validates signatures, and submits
    /// - **Raw bytes** (`&[u8]` or `Vec<u8>`): Handles LZ4 compression if detected, then submits
    /// 
    /// Returns transaction signature if successful.
    /// 
    /// # Examples
    /// ```rust,no_run
    /// // Submit from base64 string
    /// let signature = sdk.submit_transaction("base64_encoded_tx...").await?;
    /// 
    /// // Submit from raw bytes
    /// let signature = sdk.submit_transaction(&tx_bytes).await?;
    /// ```
    /// Submit a transaction to Solana
    /// 
    /// Unified method that accepts either base64-encoded transaction strings or raw transaction bytes.
    /// Automatically detects the input format and processes accordingly.
    /// 
    /// - **Base64 string** (`&str` or `String`): Decodes, validates signatures, and submits
    /// - **Raw bytes** (`&[u8]` or `Vec<u8>`): Handles LZ4 compression if detected, then submits
    /// 
    /// Returns transaction signature if successful.
    /// 
    /// # Examples
    /// ```rust,no_run
    /// // Submit from base64 string
    /// let signature = sdk.submit_transaction("base64_encoded_tx...").await?;
    /// 
    /// // Submit from raw bytes
    /// let signature = sdk.submit_transaction(&tx_bytes).await?;
    /// ```
    pub async fn submit_transaction(
        &self,
        transaction: impl TransactionInput,
    ) -> Result<String, PolliNetError> {
        transaction.submit(self).await
    }
    
    /// Refresh the blockhash in an unsigned transaction
    /// 
    /// Use this right before sending an unsigned transaction to MWA for signing
    /// to ensure the blockhash is fresh and won't expire during the signing process.
    pub async fn refresh_blockhash_in_unsigned_transaction(
        &self,
        unsigned_tx_base64: &str,
    ) -> Result<String, PolliNetError> {
        Ok(self
            .transaction_service
            .refresh_blockhash_in_unsigned_transaction(unsigned_tx_base64)
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
        self.ble_bridge.send_fragments(fragments).await?;
        
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
        // TEMPORARY FIX: Use broadcast mode due to GATT MTU limitations
        // GATT notifications are limited to ~20 bytes by default MTU
        // Broadcast mode doesn't have this limitation
        tracing::info!("📤 Using broadcast mode for {} fragments (bypassing GATT MTU limitation)", fragments.len());
        return Ok(self.ble_bridge.send_fragments(fragments).await?);
        
        // TODO: Implement proper MTU negotiation for GATT write
        // Once MTU is negotiated to 512 bytes, re-enable GATT write path below:
        
        /* DISABLED - MTU issue causes only 4 bytes to be received
        let connected_peer = self.connected_peer.read().await;
        
        if let Some(peer_address) = connected_peer.as_ref() {
            // Try to send to the connected peer using write_to_device (central mode)
            tracing::info!("📤 Attempting to send {} fragments to connected peer: {}", fragments.len(), peer_address);
            
            let mut write_succeeded = true;
            let fragments_clone = fragments.clone();
            
            for fragment in &fragments_clone {
                let data = serde_json::to_vec(&fragment)
                    .map_err(|e| PolliNetError::Serialization(e.to_string()))?;
                
                match self.ble_bridge.write_to_device(peer_address, &data).await {
                    Ok(_) => {
                        tracing::debug!("✅ Fragment sent via GATT write");
                    }
                    Err(e) => {
                        tracing::warn!("⚠️  GATT write failed: {}", e);
                        tracing::info!("   Falling back to broadcast mode...");
                        write_succeeded = false;
                        break;
                    }
                }
                
                // Small delay between fragments to avoid overwhelming the connection
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
            
            if write_succeeded {
                tracing::info!("✅ All fragments sent successfully via GATT to peer: {}", peer_address);
                Ok(())
            } else {
                // Fallback to broadcast mode if GATT write failed
                tracing::info!("📤 Falling back to broadcast mode for all fragments");
                Ok(self.ble_bridge.send_fragments(fragments).await?)
            }
        } else {
            // No connected peer, use broadcast mode (peripheral mode)
            tracing::info!("📤 Broadcasting {} fragments (no specific peer connected)", fragments.len());
            Ok(self.ble_bridge.send_fragments(fragments).await?)
        }
        */
    }

    /// Broadcast confirmation after successful submission
    pub async fn broadcast_confirmation(&self, signature: &str) -> Result<(), PolliNetError> {
        Ok(self
            .transaction_service
            .broadcast_confirmation(signature)
            .await?)
    }

    /// Cast a governance vote with durable nonce
    /// Creates a presigned vote transaction using a nonce account for longer lifetime
    /// Returns compressed transaction bytes ready for BLE transmission
    pub async fn cast_vote(
        &self,
        voter_keypair: &solana_sdk::signature::Keypair,
        proposal_id: &str,
        vote_account: &str,
        choice: u8,
        nonce_account: &str,
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self
            .transaction_service
            .cast_vote(voter_keypair, proposal_id, vote_account, choice, nonce_account)
            .await?)
    }

    /// Discover nearby BLE peers
    /// 
    /// Scans for BLE devices and returns them in the specified format.
    /// Automatically stops scanning after discovery completes.
    /// 
    /// # Arguments
    /// * `wait_seconds` - How long to wait for devices to be discovered (default: 3 seconds)
    /// * `format` - Return format: `PeerInfo` (detailed) or `String` (addresses only)
    /// 
    /// # Examples
    /// ```rust,no_run
    /// // Get detailed peer information
    /// let peers = sdk.discover_ble_peers_with_format(3, DiscoveryFormat::PeerInfo).await?;
    /// 
    /// // Get just device addresses
    /// let addresses = sdk.discover_ble_peers_with_format(5, DiscoveryFormat::Addresses).await?;
    /// ```
    pub async fn discover_ble_peers_with_format(
        &self,
        wait_seconds: u64,
        format: DiscoveryFormat,
    ) -> Result<DiscoveryResult, PolliNetError> {
        tracing::info!("🔍 Starting BLE peer discovery (wait: {}s)...", wait_seconds);
        
        // Start scanning
        self.ble_bridge.start_scanning().await?;
        
        // Wait for devices to be discovered
        tokio::time::sleep(tokio::time::Duration::from_secs(wait_seconds)).await;
        
        // Get discovered devices
        let discovered = self.ble_bridge.get_discovered_devices().await?;
        
        // Stop scanning
        self.ble_bridge.stop_scanning().await?;
        
        tracing::info!("✅ Found {} BLE devices", discovered.len());
        
        // Convert to requested format
        let result = match format {
            DiscoveryFormat::PeerInfo => {
                let peers: Vec<ble::PeerInfo> = discovered.into_iter().map(|device| {
                    ble::PeerInfo {
                        peer_id: device.address.clone(),
                        device_uuid: None,
                        capabilities: vec!["CAN_RELAY".to_string()],
                        rssi: device.rssi.unwrap_or(-100),
                        first_seen: device.last_seen,
                        last_seen: device.last_seen,
                        state: ble::PeerState::Discovered,
                        connection_attempts: 0,
                        last_attempt: None,
                    }
                }).collect();
                DiscoveryResult::PeerInfo(peers)
            }
            DiscoveryFormat::Addresses => {
                let addresses: Vec<String> = discovered.into_iter()
                    .map(|d| d.address.clone())
                    .collect();
                DiscoveryResult::Addresses(addresses)
            }
        };
        
        Ok(result)
    }
    
    /// Discover nearby BLE peers (returns detailed PeerInfo)
    /// 
    /// Convenience method that calls `discover_ble_peers_with_format()` with default settings.
    /// Scans for 3 seconds and returns detailed peer information.
    pub async fn discover_ble_peers(&self) -> Result<Vec<ble::PeerInfo>, PolliNetError> {
        match self.discover_ble_peers_with_format(3, DiscoveryFormat::PeerInfo).await? {
            DiscoveryResult::PeerInfo(peers) => Ok(peers),
            DiscoveryResult::Addresses(_) => unreachable!(),
        }
    }
    
    /// Scan all BLE devices (returns device addresses)
    /// 
    /// Convenience method that calls `discover_ble_peers_with_format()` with address format.
    /// Scans for 5 seconds and returns device addresses as strings.
    pub async fn scan_all_devices(&self) -> Result<Vec<String>, PolliNetError> {
        match self.discover_ble_peers_with_format(5, DiscoveryFormat::Addresses).await? {
            DiscoveryResult::PeerInfo(_) => unreachable!(),
            DiscoveryResult::Addresses(addresses) => Ok(addresses),
        }
    }
    
    /// Connect to a discovered BLE peer and establish GATT session
    pub async fn connect_to_ble_peer(&self, peer_address: &str) -> Result<(), PolliNetError> {
        tracing::info!("🔗 Connecting to BLE peer: {}", peer_address);
        self.ble_bridge.connect_to_device(peer_address).await?;
        
        // Track the connected peer for sending data
        let mut connected_peer = self.connected_peer.write().await;
        *connected_peer = Some(peer_address.to_string());
        
        tracing::info!("✅ Connected to peer: {}", peer_address);
        Ok(())
    }
    
    /// Send data to a connected BLE peer
    pub async fn send_to_peer(&self, peer_address: &str, data: &[u8]) -> Result<(), PolliNetError> {
        tracing::info!("📤 Sending {} bytes to peer: {}", data.len(), peer_address);
        self.ble_bridge.write_to_device(peer_address, data).await?;
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
    
    /// Start continuous BLE scanning
    pub async fn start_ble_scanning(&self) -> Result<(), PolliNetError> {
        self.ble_bridge.start_scanning().await?;
        tracing::info!("🔍 BLE scanning started - discovering PolliNet devices...");
        Ok(())
    }
    
    /// Stop BLE scanning
    pub async fn stop_ble_scanning(&self) -> Result<(), PolliNetError> {
        self.ble_bridge.stop_scanning().await?;
        tracing::info!("🛑 BLE scanning stopped");
        Ok(())
    }
    
    /// Stop BLE advertising
    pub async fn stop_ble_advertising(&self) -> Result<(), PolliNetError> {
        self.ble_bridge.stop_advertising().await?;
        tracing::info!("🛑 BLE advertising stopped");
        Ok(())
    }
    
    /// Reset BLE state - stop all scanning and advertising
    pub async fn reset_ble(&self) -> Result<(), PolliNetError> {
        tracing::info!("🔄 Resetting BLE state...");
        
        // Stop scanning if active
        if self.is_scanning() {
            let _ = self.ble_bridge.stop_scanning().await;
        }
        
        // Stop advertising if active
        if self.is_advertising() {
            let _ = self.ble_bridge.stop_advertising().await;
        }
        
        // Clear connected peer
        let mut connected_peer = self.connected_peer.write().await;
        *connected_peer = None;
        
        tracing::info!("✅ BLE state reset complete");
        Ok(())
    }
    
    /// Get list of discovered PolliNet devices
    pub async fn get_discovered_pollinet_devices(&self) -> Result<Vec<ble::adapter::DiscoveredDevice>, PolliNetError> {
        let devices = self.ble_bridge.get_discovered_devices().await?;
        tracing::info!("📱 Found {} discovered PolliNet devices", devices.len());
        Ok(devices)
    }
    
    /// Send text message to a connected peer
    pub async fn send_text_message(&self, peer_id: &str, message: &str) -> Result<(), PolliNetError> {
        tracing::info!("📤 Sending text message to {}: '{}'", peer_id, message);
        self.ble_bridge.send_text_message(message).await
            .map_err(|e| PolliNetError::BleAdapter(e))?;
        tracing::info!("✅ Text message sent successfully");
        Ok(())
    }
    
    /// Start listening for incoming text messages
    pub async fn start_text_listener(&self) -> Result<(), PolliNetError> {
        tracing::info!("🎧 Starting text message listener...");
        tracing::info!("✅ Text message listener started - messages will be buffered for retrieval");
        Ok(())
    }
    
    /// Check for incoming text messages from connected peers
    pub async fn check_incoming_messages(&self) -> Result<Vec<String>, PolliNetError> {
        tracing::debug!("🔍 Checking for incoming text messages...");
        let messages = self.ble_bridge.get_text_messages().await;
        if !messages.is_empty() {
            tracing::info!("📨 Retrieved {} text message(s)", messages.len());
        }
        Ok(messages)
    }
    
    /// Check if there are any pending text messages
    pub async fn has_pending_messages(&self) -> bool {
        self.ble_bridge.has_text_messages().await
    }
    
    /// Get BLE adapter information
    pub fn get_adapter_info(&self) -> String {
        self.ble_bridge.get_adapter_info().to_string()
    }
    
    /// Get number of connected BLE clients
    pub async fn get_connected_clients_count(&self) -> usize {
        self.ble_bridge.connected_clients_count().await
    }
    
    /// Get number of fragments in the buffer
    pub async fn get_fragment_count(&self) -> usize {
        self.ble_bridge.get_fragment_count().await
    }
    
    /// Check if BLE adapter is advertising
    pub fn is_advertising(&self) -> bool {
        self.ble_bridge.is_advertising()
    }
    
    /// Check if BLE adapter is scanning
    pub fn is_scanning(&self) -> bool {
        self.ble_bridge.is_scanning()
    }
    
    /// Get all fragments for a specific transaction
    pub async fn get_fragments_for_transaction(&self, tx_id: &str) -> Option<Vec<transaction::Fragment>> {
        self.ble_bridge.get_fragments_for_transaction(tx_id).await
    }
    
    /// Get all transaction IDs that have complete fragments
    pub async fn get_complete_transactions(&self) -> Vec<String> {
        self.ble_bridge.get_complete_transactions().await
    }
    
    /// Clear fragments for a specific transaction
    pub async fn clear_fragments(&self, tx_id: &str) {
        self.ble_bridge.clear_fragments(tx_id).await;
    }
}

// TransactionInput trait implementations for unified submit_transaction() method
impl TransactionInput for &str {
    async fn submit(&self, sdk: &PolliNetSDK) -> Result<String, PolliNetError> {
        Ok(sdk
            .transaction_service
            .send_and_confirm_transaction(self)
            .await?)
    }
}

impl TransactionInput for String {
    async fn submit(&self, sdk: &PolliNetSDK) -> Result<String, PolliNetError> {
        Ok(sdk
            .transaction_service
            .send_and_confirm_transaction(self.as_str())
            .await?)
    }
}

impl TransactionInput for &[u8] {
    async fn submit(&self, sdk: &PolliNetSDK) -> Result<String, PolliNetError> {
        Ok(sdk
            .transaction_service
            .submit_to_solana(self)
            .await?)
    }
}

impl TransactionInput for Vec<u8> {
    async fn submit(&self, sdk: &PolliNetSDK) -> Result<String, PolliNetError> {
        Ok(sdk
            .transaction_service
            .submit_to_solana(self.as_slice())
            .await?)
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
