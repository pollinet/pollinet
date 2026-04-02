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

/// Trait for transaction input types that can be submitted to Solana
/// Allows unified `submit_transaction()` method to accept both base64 strings and raw bytes
#[allow(async_fn_in_trait)]
pub trait TransactionInput {
    async fn submit(&self, sdk: &PolliNetSDK) -> Result<String, PolliNetError>;
}

/// Core PolliNet SDK instance
pub struct PolliNetSDK {
    /// Transaction builder and manager
    transaction_service: Arc<transaction::TransactionService>,
    /// Nonce account management
    #[allow(dead_code)]
    nonce_manager: Arc<nonce::NonceManager>,
    /// Local transaction cache
    local_cache: Arc<RwLock<transaction::TransactionCache>>,
    /// Queue manager for all queue operations
    queue_manager: Arc<queue::QueueManager>,
}

impl PolliNetSDK {
    fn make_queue_manager(storage_dir: Option<&str>) -> Arc<queue::QueueManager> {
        if let Some(dir) = storage_dir {
            tracing::info!("Using persistent queue storage: {}", dir);
            Arc::new(queue::QueueManager::with_storage(dir).unwrap_or_else(|e| {
                tracing::warn!("Failed to load queues from storage: {}, starting fresh", e);
                queue::QueueManager::new()
            }))
        } else {
            tracing::info!("No persistent storage configured, queues will not persist");
            Arc::new(queue::QueueManager::new())
        }
    }

    /// Initialize a new PolliNet SDK instance without RPC client
    pub async fn new() -> Result<Self, PolliNetError> {
        Ok(Self {
            transaction_service: Arc::new(transaction::TransactionService::new().await?),
            nonce_manager: Arc::new(nonce::NonceManager::new().await?),
            local_cache: Arc::new(RwLock::new(transaction::TransactionCache::new())),
            queue_manager: Self::make_queue_manager(None),
        })
    }

    /// Initialize a new PolliNet SDK instance with RPC client for nonce account fetching
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, PolliNetError> {
        Ok(Self {
            transaction_service: Arc::new(
                transaction::TransactionService::new_with_rpc(rpc_url).await?,
            ),
            nonce_manager: Arc::new(nonce::NonceManager::new().await?),
            local_cache: Arc::new(RwLock::new(transaction::TransactionCache::new())),
            queue_manager: Self::make_queue_manager(None),
        })
    }

    // =========================================================================
    // Queue Management Methods (Phase 2)
    // =========================================================================

    /// Get queue manager reference
    pub fn queue_manager(&self) -> &Arc<queue::QueueManager> {
        &self.queue_manager
    }

    /// Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
    /// Note: This does NOT clear nonce data
    pub async fn clear_all_queues(&self) -> Result<(), PolliNetError> {
        // Clear queue manager queues
        self.queue_manager.clear_all_queues().await;

        tracing::info!("✅ Cleared all queues via SDK");
        Ok(())
    }

    /// Get queue metrics
    pub async fn get_queue_metrics(&self) -> queue::QueueMetrics {
        self.queue_manager.get_metrics().await
    }

    /// Get queue health status
    pub async fn get_queue_health(&self) -> queue::HealthStatus {
        self.queue_manager.get_health().await
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
    ///
    /// If bundle_file doesn't exist:
    ///   - Creates new bundle with 'count' nonce accounts
    ///
    /// Example:
    /// ```rust,no_run
    /// # use pollinet::PolliNetSDK;
    /// # use solana_sdk::signature::Keypair;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sdk = PolliNetSDK::new_with_rpc("https://api.devnet.solana.com").await?;
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
        Ok(self.transaction_service.create_offline_transaction(
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
    pub fn get_required_signers(&self, base64_tx: &str) -> Result<Vec<String>, PolliNetError> {
        Ok(self.transaction_service.get_required_signers(base64_tx)?)
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
    /// ```rust,ignore
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

    /// Accept and queue a pre-signed transaction from external partners
    ///
    /// This method is designed for accepting transactions from external partners.
    /// It verifies the transaction is properly signed, compresses it if needed,
    /// fragments it for BLE transmission, and adds it to the outbound queue for relay.
    ///
    /// # Arguments
    /// * `base64_signed_tx` - Base64-encoded pre-signed Solana transaction
    /// * `max_payload` - Optional maximum payload size (typically MTU - 10). If None, uses default.
    ///
    /// # Returns
    /// Transaction ID (SHA-256 hash as hex string) for tracking
    ///
    /// # Errors
    /// Returns error if:
    /// - Transaction is not properly signed
    /// - Transaction verification fails
    /// - Compression fails
    /// - Fragmentation fails
    /// - Queue is full
    pub async fn accept_and_queue_external_transaction(
        &self,
        base64_signed_tx: &str,
        max_payload: Option<usize>,
    ) -> Result<String, PolliNetError> {
        use crate::ble::fragmenter;
        use crate::queue::{OutboundTransaction, Priority};
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        use sha2::{Digest, Sha256};

        tracing::info!("📥 Accepting external pre-signed transaction for relay");

        // Decode from base64
        let tx_bytes = BASE64.decode(base64_signed_tx).map_err(|e| {
            PolliNetError::Transaction(transaction::TransactionError::Serialization(format!(
                "Failed to decode base64: {}",
                e
            )))
        })?;

        tracing::info!("Decoded transaction: {} bytes", tx_bytes.len());

        // Deserialize and verify transaction
        let tx: solana_sdk::transaction::Transaction =
            bincode1::deserialize(&tx_bytes).map_err(|e| {
                PolliNetError::Transaction(transaction::TransactionError::Serialization(format!(
                    "Failed to deserialize transaction: {}",
                    e
                )))
            })?;

        // Verify transaction has valid signatures
        let valid_sigs = tx
            .signatures
            .iter()
            .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
            .count();

        if valid_sigs == 0 {
            return Err(PolliNetError::Transaction(
                transaction::TransactionError::Serialization(
                    "Transaction must be signed before queuing for relay".to_string(),
                ),
            ));
        }

        // Verify transaction signatures
        if let Err(err) = tx.verify() {
            tracing::error!("❌ Transaction signature verification failed: {}", err);
            return Err(PolliNetError::Transaction(
                transaction::TransactionError::Serialization(format!(
                    "Transaction signature verification failed: {}",
                    err
                )),
            ));
        }

        tracing::info!(
            "✅ Transaction verified: {}/{} valid signatures",
            valid_sigs,
            tx.signatures.len()
        );
        tracing::info!("   Instructions: {}", tx.message.instructions.len());

        // Store original bytes for transaction ID calculation
        let original_tx_bytes = tx_bytes.clone();

        // Compress the transaction if it exceeds the threshold
        // Use the transaction service's compression logic via process_and_relay_transaction
        // but we'll do it manually to avoid creating fragments twice
        let compressed_tx = if tx_bytes.len() > crate::COMPRESSION_THRESHOLD {
            tracing::info!(
                "Compressing transaction (threshold: {} bytes)",
                crate::COMPRESSION_THRESHOLD
            );
            // Create a temporary compressor to compress
            let compressor = crate::util::lz::Lz4Compressor::new().map_err(|e| {
                PolliNetError::Transaction(transaction::TransactionError::Compression(
                    e.to_string(),
                ))
            })?;
            let compressed = compressor.compress_with_size(&tx_bytes).map_err(|e| {
                PolliNetError::Transaction(transaction::TransactionError::Compression(
                    e.to_string(),
                ))
            })?;
            tracing::info!(
                "Compressed: {} bytes -> {} bytes",
                tx_bytes.len(),
                compressed.len()
            );
            compressed
        } else {
            tracing::info!("Transaction below compression threshold, keeping uncompressed");
            tx_bytes
        };

        tracing::info!("Final transaction size: {} bytes", compressed_tx.len());

        // Fragment the transaction
        let mesh_fragments = if let Some(max_payload) = max_payload {
            fragmenter::fragment_transaction_with_max_payload(&compressed_tx, max_payload)
        } else {
            fragmenter::fragment_transaction(&compressed_tx)
        };

        tracing::info!(
            "✅ Created {} fragments for BLE transmission",
            mesh_fragments.len()
        );

        // Calculate transaction ID (SHA-256 hash of original uncompressed transaction)
        let mut hasher = Sha256::new();
        hasher.update(&original_tx_bytes);
        let tx_id = hex::encode(hasher.finalize());

        // Create outbound transaction with NORMAL priority (external partner transactions)
        let outbound_tx = OutboundTransaction::new(
            tx_id.clone(),
            original_tx_bytes, // Store original uncompressed bytes
            mesh_fragments,
            Priority::Normal, // External partner transactions use normal priority
        );

        // Add to outbound queue
        let mut queue = self.queue_manager.outbound.write().await;
        queue.push(outbound_tx).map_err(|e| {
            PolliNetError::Transaction(transaction::TransactionError::Serialization(format!(
                "Failed to add transaction to queue: {}",
                e
            )))
        })?;
        drop(queue);

        tracing::info!("✅ External transaction queued for relay: {}", tx_id);

        Ok(tx_id)
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

        let tx_id = fragments.first().map(|f| f.id.clone()).ok_or_else(|| {
            PolliNetError::Transaction(transaction::TransactionError::Serialization(
                "No fragments created".to_string(),
            ))
        })?;

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
    #[allow(clippy::too_many_arguments)]
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

    // ── BLE mesh stubs (to be implemented with platform BLE integration) ──

    /// Reset BLE state and clear connections
    pub async fn reset_ble(&self) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Start BLE networking (advertising + scanning)
    pub async fn start_ble_networking(&self) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Start BLE scanning for peers
    pub async fn start_ble_scanning(&self) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Start listener for incoming text messages
    pub async fn start_text_listener(&self) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Get count of currently connected BLE clients
    pub async fn get_connected_clients_count(&self) -> usize {
        0
    }

    /// Represents a discovered BLE peer
    pub async fn discover_ble_peers(&self) -> Result<Vec<BlePeer>, PolliNetError> {
        Ok(Vec::new())
    }

    /// Connect to a specific BLE peer by ID
    pub async fn connect_to_ble_peer(&self, _peer_id: &str) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Send a short text message to a peer
    pub async fn send_text_message(
        &self,
        _peer_id: &str,
        _message: &str,
    ) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Check for incoming text messages from peers
    pub async fn check_incoming_messages(&self) -> Result<Vec<String>, PolliNetError> {
        Ok(Vec::new())
    }

    /// Get IDs of transactions that have been fully reassembled
    pub async fn get_complete_transactions(&self) -> Vec<String> {
        Vec::new()
    }

    /// Get reassembled fragments for a specific transaction ID
    pub async fn get_fragments_for_transaction(
        &self,
        _tx_id: &str,
    ) -> Option<Vec<transaction::Fragment>> {
        None
    }

    /// Clear stored fragments for a transaction after processing
    pub async fn clear_fragments(&self, _tx_id: &str) {}

    /// Relay pre-fragmented transaction over BLE mesh
    pub async fn relay_transaction(
        &self,
        _fragments: Vec<transaction::Fragment>,
    ) -> Result<(), PolliNetError> {
        Ok(())
    }

    /// Reassemble fragments back into a complete transaction
    pub fn reassemble_fragments(
        &self,
        fragments: &[transaction::Fragment],
    ) -> Result<Vec<u8>, PolliNetError> {
        Ok(self.transaction_service.reassemble_fragments(fragments)?)
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
            .cast_vote(
                voter_keypair,
                proposal_id,
                vote_account,
                choice,
                nonce_account,
            )
            .await?)
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
        Ok(sdk.transaction_service.submit_to_solana(self).await?)
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

/// Represents a discovered BLE peer device
pub struct BlePeer {
    /// Peer identifier
    pub peer_id: String,
    /// Received signal strength indicator (dBm)
    pub rssi: i32,
}

/// BLE MTU size for packet fragmentation
pub const BLE_MTU_SIZE: usize = 360;

/// Compression threshold in bytes
pub const COMPRESSION_THRESHOLD: usize = 100;
