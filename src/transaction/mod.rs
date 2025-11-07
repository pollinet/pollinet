//! Transaction management for PolliNet SDK
//!
//! Handles creation, signing, compression, fragmentation, and submission of Solana transactions

use crate::{BLE_MTU_SIZE, COMPRESSION_THRESHOLD};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use spl_associated_token_account;
use spl_token::instruction as spl_instruction;
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

/// Transaction fragment for BLE transmission
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Fragment {
    /// Unique transaction identifier
    pub id: String,
    /// Fragment index within the transaction
    pub index: usize,
    /// Total number of fragments
    pub total: usize,
    /// Fragment data
    pub data: Vec<u8>,
    /// Fragment type
    pub fragment_type: FragmentType,
    /// SHA-256 checksum of the complete transaction (before fragmentation)
    pub checksum: [u8; 32],
}

/// Fragment type for proper reassembly
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FragmentType {
    FragmentStart,
    FragmentContinue,
    FragmentEnd,
}

/// Cached nonce account data for offline transaction creation
/// This data is fetched once while online and used to create transactions offline
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedNonceData {
    /// Nonce account public key
    pub nonce_account: String,
    /// Nonce authority public key
    pub authority: String,
    /// Stored blockhash from nonce account (durable)
    pub blockhash: String,
    /// Fee per signature (lamports)
    pub lamports_per_signature: u64,
    /// Unix timestamp when this data was cached
    pub cached_at: u64,
    /// Whether this nonce has been used (for tracking)
    #[serde(default)]
    pub used: bool,
}

/// Bundle of multiple nonce accounts prepared for offline use
/// Allows creating multiple offline transactions (one per nonce account)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OfflineTransactionBundle {
    /// Cached nonce data for each nonce account
    pub nonce_caches: Vec<CachedNonceData>,
    /// Maximum number of transactions that can be created
    pub max_transactions: usize,
    /// Unix timestamp when this bundle was created
    pub created_at: u64,
}

/// Transaction packet for BLE transmission
/// Contains pre-signed transaction and metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BLETransactionPacket {
    /// Pre-signed, possibly compressed transaction bytes
    pub transaction_bytes: Vec<u8>,
    /// Metadata for diagnostics and verification
    pub metadata: TransactionMetadata,
    /// SHA-256 checksum for integrity verification
    pub checksum: [u8; 32],
}

/// Nonce refresh request sent from online device back to offline device
/// Used when nonce has been advanced and transaction needs to be rebuilt
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NonceRefreshRequest {
    /// Nonce account that was advanced
    pub nonce_account: String,
    /// New blockhash from advanced nonce
    pub new_blockhash: String,
    /// New nonce authority
    pub new_authority: String,
    /// Original transaction recipient
    pub original_recipient: String,
    /// Original transaction amount
    pub original_amount: u64,
    /// Timestamp of refresh request
    pub timestamp: u64,
}

/// Success confirmation sent back over BLE
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SuccessConfirmation {
    /// Transaction signature
    pub signature: String,
    /// Timestamp of successful submission
    pub timestamp: u64,
}

/// Pending transaction in local cache
#[derive(Debug, Clone)]
pub struct PendingTransaction {
    /// Transaction ID
    pub id: String,
    /// Serialized transaction data
    pub data: Vec<u8>,
    /// Creation timestamp
    pub created_at: std::time::Instant,
    /// Transaction metadata
    pub metadata: TransactionMetadata,
}

/// Transaction metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadata {
    /// Sender public key
    pub sender: String,
    /// Recipient public key
    pub recipient: String,
    /// Transaction amount
    pub amount: u64,
    /// Maximum fee
    pub max_fee: u64,
    /// Expiration timestamp
    pub expires_at: Option<std::time::SystemTime>,
}

impl OfflineTransactionBundle {
    /// Create a new empty offline transaction bundle
    pub fn new() -> Self {
        Self {
            nonce_caches: Vec::new(),
            max_transactions: 0,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    /// Add a cached nonce to the bundle
    pub fn add_nonce(&mut self, mut cached_nonce: CachedNonceData) {
        cached_nonce.used = false; // Ensure it's marked as unused
        self.nonce_caches.push(cached_nonce);
        self.max_transactions = self.nonce_caches.len();
    }
    
    /// Get a nonce cache by index
    pub fn get_nonce(&self, index: usize) -> Option<&CachedNonceData> {
        self.nonce_caches.get(index)
    }
    
    /// Get the next available (unused) nonce
    /// Returns (index, nonce) of the first unused nonce
    pub fn get_next_available_nonce(&self) -> Option<(usize, &CachedNonceData)> {
        self.nonce_caches
            .iter()
            .enumerate()
            .find(|(_, nonce)| !nonce.used)
    }
    
    /// Mark a nonce as used by index
    pub fn mark_used(&mut self, index: usize) -> Result<(), String> {
        if let Some(nonce) = self.nonce_caches.get_mut(index) {
            nonce.used = true;
            Ok(())
        } else {
            Err(format!("Invalid nonce index: {}", index))
        }
    }
    
    /// Mark a nonce as used by nonce account address
    pub fn mark_used_by_account(&mut self, nonce_account: &str) -> Result<(), String> {
        if let Some(nonce) = self.nonce_caches
            .iter_mut()
            .find(|n| n.nonce_account == nonce_account)
        {
            nonce.used = true;
            Ok(())
        } else {
            Err(format!("Nonce account not found: {}", nonce_account))
        }
    }
    
    /// Get the number of total nonces (used + unused)
    pub fn total_nonces(&self) -> usize {
        self.nonce_caches.len()
    }
    
    /// Get the number of available (unused) nonces
    pub fn available_nonces(&self) -> usize {
        self.nonce_caches.iter().filter(|n| !n.used).count()
    }
    
    /// Get the number of used nonces
    pub fn used_nonces(&self) -> usize {
        self.nonce_caches.iter().filter(|n| n.used).count()
    }
    
    /// Check if the bundle has any available nonces
    pub fn is_empty(&self) -> bool {
        self.available_nonces() == 0
    }
    
    /// Get the age of this bundle in hours
    pub fn age_hours(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        (now - self.created_at) / 3600
    }
    
    /// Save bundle to JSON file
    /// Saves ALL nonces (including used ones) so they can be refreshed later
    /// 
    /// Used nonces will be automatically refreshed (fetch new blockhash) when
    /// prepare_offline_bundle() is called again - this is FREE and saves costs!
    /// 
    /// NEVER removes used nonces - they are valuable and will be refreshed!
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
    
    /// Load bundle from JSON file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let bundle: Self = serde_json::from_str(&json)?;
        Ok(bundle)
    }
}

impl BLETransactionPacket {
    /// Create a new BLE transaction packet
    pub fn new(transaction_bytes: Vec<u8>, metadata: TransactionMetadata) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&transaction_bytes);
        let checksum: [u8; 32] = hasher.finalize().into();
        
        Self {
            transaction_bytes,
            metadata,
            checksum,
        }
    }
    
    /// Verify the checksum matches the transaction bytes
    pub fn verify_checksum(&self) -> bool {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&self.transaction_bytes);
        let computed: [u8; 32] = hasher.finalize().into();
        computed == self.checksum
    }
}

/// Local transaction cache for store-and-forward functionality
pub struct TransactionCache {
    /// Pending transactions awaiting submission
    pending_transactions: HashMap<String, PendingTransaction>,
    /// Fragments being reassembled
    reassembly_buffers: HashMap<String, Vec<Option<Fragment>>>,
}

impl TransactionCache {
    /// Create a new transaction cache
    pub fn new() -> Self {
        Self {
            pending_transactions: HashMap::new(),
            reassembly_buffers: HashMap::new(),
        }
    }

    /// Store a pending transaction
    pub fn store_pending(&mut self, tx: PendingTransaction) {
        self.pending_transactions.insert(tx.id.clone(), tx);
    }

    /// Get a pending transaction by ID
    pub fn get_pending(&self, id: &str) -> Option<&PendingTransaction> {
        self.pending_transactions.get(id)
    }

    /// Remove a pending transaction (after successful submission)
    pub fn remove_pending(&mut self, id: &str) {
        self.pending_transactions.remove(id);
    }

    /// Add a fragment to reassembly buffer
    pub fn add_fragment(&mut self, fragment: Fragment) {
        let buffer = self
            .reassembly_buffers
            .entry(fragment.id.clone())
            .or_insert_with(|| vec![None; fragment.total]);

        if fragment.index < buffer.len() {
            buffer[fragment.index] = Some(fragment.clone());
        }
    }

    /// Check if all fragments for a transaction are received
    pub fn all_fragments_received(&self, tx_id: &str) -> bool {
        if let Some(buffer) = self.reassembly_buffers.get(tx_id) {
            buffer.iter().all(|f| f.is_some())
        } else {
            false
        }
    }

    /// Reassemble fragments into complete transaction
    pub fn reassemble_fragments(&self, tx_id: &str) -> Option<Vec<u8>> {
        if let Some(buffer) = self.reassembly_buffers.get(tx_id) {
            if buffer.iter().all(|f| f.is_some()) {
                let mut data = Vec::new();
                for fragment in buffer.iter().flatten() {
                    data.extend_from_slice(&fragment.data);
                }
                Some(data)
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Core transaction service for PolliNet
pub struct TransactionService {
    /// LZ4 compressor for transaction payloads
    compressor: crate::util::lz::Lz4Compressor,
    /// RPC client for fetching nonce account data
    rpc_client: Option<Box<solana_client::rpc_client::RpcClient>>,
}

impl TransactionService {
    /// Create a new transaction service
    pub async fn new() -> Result<Self, TransactionError> {
        let compressor = crate::util::lz::Lz4Compressor::new()
            .map_err(|e| TransactionError::Compression(e.to_string()))?;

        Ok(Self {
            compressor,
            rpc_client: None,
        })
    }

    /// Create a new transaction service with RPC client
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, TransactionError> {
        let compressor = crate::util::lz::Lz4Compressor::new()
            .map_err(|e| TransactionError::Compression(e.to_string()))?;

        let rpc_client = Box::new(solana_client::rpc_client::RpcClient::new_with_commitment(
            rpc_url.to_string(),
            CommitmentConfig::confirmed(),
        ));

        Ok(Self {
            compressor,
            rpc_client: Some(rpc_client),
        })
    }

    /// Add a signature to an unsigned transaction (base64 encoded)
    /// Intelligently adds signature based on signer's role in the transaction
    /// If signer is nonce authority and also sender, signature is used for both roles
    pub fn add_signature(
        &self,
        base64_tx: &str,
        signer_pubkey: &Pubkey,
        signature: &solana_sdk::signature::Signature,
    ) -> Result<String, TransactionError> {
        // Decode from base64
        let unsigned_tx = base64::decode(base64_tx).map_err(|e| {
            TransactionError::Serialization(format!("Failed to decode base64: {}", e))
        })?;

        // Deserialize the unsigned transaction
        let mut tx: Transaction = bincode1::deserialize(&unsigned_tx).map_err(|e| {
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        tracing::info!("Adding signature for signer: {}", signer_pubkey);
        tracing::info!("Current signatures: {}", tx.signatures.len());

        // Find the signer's position in the transaction
        let signer_positions: Vec<usize> = tx
            .message
            .account_keys
            .iter()
            .enumerate()
            .filter(|(_, key)| *key == signer_pubkey)
            .map(|(i, _)| i)
            .collect();

        if signer_positions.is_empty() {
            return Err(TransactionError::InvalidPublicKey(format!(
                "Signer {} is not part of this transaction",
                signer_pubkey
            )));
        }

        tracing::info!("Signer found at position(s): {:?}", signer_positions);

        // Check if this is the nonce authority (first instruction should be advance nonce)
        let is_nonce_authority = if !tx.message.instructions.is_empty() {
            let first_instruction = &tx.message.instructions[0];
            // Advance nonce instruction has the authority as the second account
            if first_instruction.accounts.len() > 1 {
                let nonce_authority_index = first_instruction.accounts[1] as usize;
                if nonce_authority_index < tx.message.account_keys.len() {
                    let nonce_authority = &tx.message.account_keys[nonce_authority_index];
                    *nonce_authority == *signer_pubkey
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // Check if this is also the sender (second instruction should be transfer)
        let is_sender = if tx.message.instructions.len() > 1 {
            let transfer_instruction = &tx.message.instructions[1];
            if !transfer_instruction.accounts.is_empty() {
                let sender_index = transfer_instruction.accounts[0] as usize;
                if sender_index < tx.message.account_keys.len() {
                    let sender = &tx.message.account_keys[sender_index];
                    *sender == *signer_pubkey
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if is_nonce_authority && is_sender {
            tracing::info!("✅ Signer is both nonce authority AND sender");
            tracing::info!("   Adding signature for both roles");

            // Add signature for all positions where this signer appears
            for &position in &signer_positions {
                if position < tx.signatures.len() {
                    tx.signatures[position] = *signature;
                    tracing::info!("   Added signature at position {}", position);
                }
            }
        } else if is_nonce_authority {
            tracing::info!("✅ Signer is nonce authority");
            // Add signature at nonce authority position
            if let Some(&position) = signer_positions.first() {
                if position < tx.signatures.len() {
                    tx.signatures[position] = *signature;
                    tracing::info!("   Added signature at position {}", position);
                }
            }
        } else if is_sender {
            tracing::info!("✅ Signer is sender");
            // Add signature at sender position
            if let Some(&position) = signer_positions.first() {
                if position < tx.signatures.len() {
                    tx.signatures[position] = *signature;
                    tracing::info!("   Added signature at position {}", position);
                }
            }
        } else {
            tracing::info!("✅ Signer is fee payer or other role");
            // Add signature at first occurrence
            if let Some(&position) = signer_positions.first() {
                if position < tx.signatures.len() {
                    tx.signatures[position] = *signature;
                    tracing::info!("   Added signature at position {}", position);
                }
            }
        }

        // Count valid signatures
        let valid_sigs = tx
            .signatures
            .iter()
            .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
            .count();
        tracing::info!("Transaction now has {} valid signature(s)", valid_sigs);

        // Serialize the updated transaction
        let serialized =
            bincode1::serialize(&tx).map_err(|e| TransactionError::Serialization(e.to_string()))?;

        // Encode to base64
        let base64_tx = base64::encode(&serialized);
        tracing::info!(
            "Encoded updated transaction to base64: {} characters",
            base64_tx.len()
        );

        Ok(base64_tx)
    }

    /// Send and confirm a transaction from base64 encoded bytes
    /// Decodes base64, deserializes, and submits to Solana
    pub async fn send_and_confirm_transaction(
        &self,
        base64_tx: &str,
    ) -> Result<String, TransactionError> {
        let client = self.rpc_client.as_ref().ok_or_else(|| {
            TransactionError::RpcClient(
                "RPC client not initialized. Use new_with_rpc()".to_string(),
            )
        })?;

        tracing::info!("Decoding base64 transaction...");

        // Decode from base64
        let tx_bytes = base64::decode(base64_tx).map_err(|e| {
            TransactionError::Serialization(format!("Failed to decode base64: {}", e))
        })?;

        tracing::info!("Decoded {} bytes from base64", tx_bytes.len());

        // Deserialize the transaction
        let tx: Transaction = bincode1::deserialize(&tx_bytes).map_err(|e| {
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        tracing::info!("✅ Transaction deserialized successfully");
        tracing::info!("   Signatures: {}", tx.signatures.len());
        tracing::info!("   Instructions: {}", tx.message.instructions.len());

        // Verify transaction has signatures
        let valid_sigs = tx
            .signatures
            .iter()
            .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
            .count();

        if valid_sigs == 0 {
            return Err(TransactionError::Serialization(
                "Transaction has no valid signatures".to_string(),
            ));
        }

        tracing::info!(
            "   Valid signatures: {}/{}",
            valid_sigs,
            tx.signatures.len()
        );

        // Submit to Solana
        tracing::info!("Submitting transaction to Solana...");
        let signature = client.send_and_confirm_transaction(&tx).map_err(|e| {
            TransactionError::RpcClient(format!("Failed to submit transaction: {}", e))
        })?;

        tracing::info!("✅ Transaction submitted successfully: {}", signature);

        Ok(signature.to_string())
    }

    /// Create an unsigned SPL token transfer transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned SPL token transaction
    /// Automatically derives ATAs from wallet pubkeys and mint address
    pub async fn create_unsigned_spl_transaction(
        &self,
        sender_wallet: &str,
        recipient_wallet: &str,
        fee_payer: &str,
        mint_address: &str,
        amount: u64,
        nonce_account: &str,
    ) -> Result<String, TransactionError> {
        // Validate public keys
        let sender_pubkey = Pubkey::from_str(sender_wallet).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid sender wallet: {}", e))
        })?;
        let recipient_pubkey = Pubkey::from_str(recipient_wallet).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid recipient wallet: {}", e))
        })?;
        let fee_payer_pubkey = Pubkey::from_str(fee_payer)
            .map_err(|e| TransactionError::InvalidPublicKey(format!("Invalid fee payer: {}", e)))?;
        let mint_pubkey = Pubkey::from_str(mint_address).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid mint address: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account: {}", e))
        })?;

        // Derive Associated Token Accounts
        let sender_token_account = spl_associated_token_account::get_associated_token_address(
            &sender_pubkey,
            &mint_pubkey,
        );
        let recipient_token_account = spl_associated_token_account::get_associated_token_address(
            &recipient_pubkey,
            &mint_pubkey,
        );

        tracing::info!("Derived Associated Token Accounts:");
        tracing::info!("  Sender ATA: {}", sender_token_account);
        tracing::info!("  Recipient ATA: {}", recipient_token_account);
        tracing::info!("  Mint: {}", mint_pubkey);

        // Fetch nonce account data to get the blockhash
        tracing::info!("Fetching nonce account data from blockchain...");
        let nonce_data = self.fetch_nonce_account_data(&nonce_account_pubkey).await?;

        tracing::info!("Building unsigned SPL token transfer instructions...");

        // Create advance nonce instruction (must be first instruction)
        // Use sender as nonce authority
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &sender_pubkey, // Sender is the nonce authority
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");
        tracing::info!("   Nonce account: {}", nonce_account_pubkey);
        tracing::info!("   Authority: {} (sender)", sender_pubkey);

        // Create SPL token transfer instruction
        let spl_transfer_ix = spl_instruction::transfer(
            &spl_token::id(),
            &sender_token_account,
            &recipient_token_account,
            &sender_pubkey, // Owner of sender token account
            &[],            // No multisig signers
            amount,
        )
        .map_err(|e| TransactionError::SolanaInstruction(e.to_string()))?;

        tracing::info!("✅ Instruction 2: SPL Token Transfer {} tokens", amount);
        tracing::info!("   From token account: {}", sender_token_account);
        tracing::info!("   To token account: {}", recipient_token_account);
        tracing::info!("   Owner: {}", sender_pubkey);
        tracing::info!("   Fee payer: {}", fee_payer_pubkey);

        // Create transaction with nonce advance as first instruction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, spl_transfer_ix],
            Some(&fee_payer_pubkey), // Fee payer pays the fee
        );
        tracing::info!(
            "Unsigned SPL transaction created with {} instructions",
            transaction.message.instructions.len()
        );

        // Use the nonce account's stored blockhash
        transaction.message.recent_blockhash = nonce_data.blockhash();
        tracing::info!("Transaction blockhash set from nonce account");

        // Serialize the UNSIGNED transaction using bincode 1.x (Solana wire format)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!(
            "Unsigned SPL transaction serialized: {} bytes (uncompressed)",
            serialized.len()
        );

        // Encode to base64
        let base64_tx = base64::encode(&serialized);
        tracing::info!("Encoded to base64: {} characters", base64_tx.len());
        tracing::info!("SPL transaction is ready for signing by sender/owner and fee payer");

        Ok(base64_tx)
    }

    /// Create an unsigned governance vote transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned vote transaction
    pub async fn cast_unsigned_vote(
        &self,
        voter: &str,
        proposal_id: &str,
        vote_account: &str,
        choice: u8,
        fee_payer: &str,
        nonce_account: &str,
    ) -> Result<String, TransactionError> {
        // Validate public keys
        let voter_pubkey = Pubkey::from_str(voter).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid voter public key: {}", e))
        })?;
        let proposal_pubkey = Pubkey::from_str(proposal_id).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid proposal ID: {}", e))
        })?;
        let vote_account_pubkey = Pubkey::from_str(vote_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid vote account: {}", e))
        })?;
        let fee_payer_pubkey = Pubkey::from_str(fee_payer).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid fee payer public key: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account public key: {}", e))
        })?;

        // Fetch nonce account data to get the blockhash
        tracing::info!("Fetching nonce account data from blockchain...");
        let nonce_data = self.fetch_nonce_account_data(&nonce_account_pubkey).await?;

        tracing::info!("Building unsigned governance vote instructions...");

        // Create advance nonce instruction (must be first instruction)
        // Use voter as nonce authority
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &voter_pubkey, // Voter is the nonce authority
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");
        tracing::info!("   Nonce account: {}", nonce_account_pubkey);
        tracing::info!("   Authority: {} (voter)", voter_pubkey);

        // Create vote instruction
        let vote_ix = self.build_cast_vote_instruction(proposal_id, vote_account, choice)?;
        tracing::info!("✅ Instruction 2: Cast vote");
        tracing::info!("   Proposal: {}", proposal_pubkey);
        tracing::info!("   Vote account: {}", vote_account_pubkey);
        tracing::info!("   Choice: {}", choice);
        tracing::info!("   Fee payer: {}", fee_payer_pubkey);

        // Create transaction with nonce advance as first instruction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, vote_ix],
            Some(&fee_payer_pubkey), // Fee payer pays the fee
        );
        tracing::info!(
            "Unsigned vote transaction created with {} instructions",
            transaction.message.instructions.len()
        );

        // Use the nonce account's stored blockhash
        transaction.message.recent_blockhash = nonce_data.blockhash();
        tracing::info!("Transaction blockhash set from nonce account");

        // Serialize the UNSIGNED transaction using bincode 1.x (Solana wire format)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!(
            "Unsigned vote transaction serialized: {} bytes (uncompressed)",
            serialized.len()
        );

        // Encode to base64
        let base64_tx = base64::encode(&serialized);
        tracing::info!("Encoded to base64: {} characters", base64_tx.len());
        tracing::info!("Vote transaction is ready for signing by voter and fee payer");

        Ok(base64_tx)
    }

    /// Create an unsigned transaction with durable nonce
    /// Returns base64 encoded uncompressed, unsigned transaction
    pub async fn create_unsigned_transaction(
        &self,
        sender: &str,
        recipient: &str,
        fee_payer: &str,
        amount: u64,
        nonce_account: &str,
    ) -> Result<String, TransactionError> {
        // Validate public keys
        let sender_pubkey = Pubkey::from_str(sender).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid sender public key: {}", e))
        })?;
        let recipient_pubkey = Pubkey::from_str(recipient).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid recipient public key: {}", e))
        })?;
        let fee_payer_pubkey = Pubkey::from_str(fee_payer).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid fee payer public key: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account public key: {}", e))
        })?;

        // Fetch nonce account data to get the blockhash
        tracing::info!("Fetching nonce account data from blockchain...");
        let nonce_data = self.fetch_nonce_account_data(&nonce_account_pubkey).await?;

        tracing::info!("Building unsigned transaction instructions...");

        // Create advance nonce instruction (must be first instruction)
        // Use sender as nonce authority
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &sender_pubkey, // Sender is the nonce authority
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");
        tracing::info!("   Nonce account: {}", nonce_account_pubkey);
        tracing::info!("   Authority: {} (sender)", sender_pubkey);

        // Create transfer instruction
        let transfer_ix = system_instruction::transfer(&sender_pubkey, &recipient_pubkey, amount);
        tracing::info!("✅ Instruction 2: Transfer {} lamports", amount);
        tracing::info!("   From: {}", sender_pubkey);
        tracing::info!("   To: {}", recipient_pubkey);
        tracing::info!("   Fee payer: {}", fee_payer_pubkey);

        // Create transaction with nonce advance as first instruction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, transfer_ix],
            Some(&fee_payer_pubkey), // Fee payer pays the fee
        );
        tracing::info!(
            "Unsigned transaction created with {} instructions",
            transaction.message.instructions.len()
        );

        // Use the nonce account's stored blockhash
        transaction.message.recent_blockhash = nonce_data.blockhash();
        tracing::info!("Transaction blockhash set from nonce account");

        // Serialize the UNSIGNED transaction using bincode 1.x (Solana wire format)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!(
            "Unsigned transaction serialized: {} bytes (uncompressed)",
            serialized.len()
        );

        // Encode to base64
        let base64_tx = base64::encode(&serialized);
        tracing::info!("Encoded to base64: {} characters", base64_tx.len());
        tracing::info!("Transaction is ready for signing by sender and fee payer");

        Ok(base64_tx)
    }

    /// Create and sign a new transaction with durable nonce
    /// Creates a presigned transaction using a nonce account for longer lifetime
    /// Sender pays the gas fee
    pub async fn create_transaction(
        &self,
        sender: &str,
        sender_keypair: &Keypair,
        recipient: &str,
        amount: u64,
        nonce_account: &str,
        nonce_authority_keypair: &Keypair,
    ) -> Result<Vec<u8>, TransactionError> {
        // Validate public keys first
        let sender_pubkey = Pubkey::from_str(sender).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid sender public key: {}", e))
        })?;
        let recipient_pubkey = Pubkey::from_str(recipient).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid recipient public key: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account public key: {}", e))
        })?;

        // Verify sender keypair matches sender pubkey
        if sender_keypair.pubkey() != sender_pubkey {
            return Err(TransactionError::InvalidPublicKey(
                "Sender keypair does not match sender public key".to_string(),
            ));
        }

        // Fetch nonce account data to get the blockhash
        tracing::info!("Fetching nonce account data from blockchain...");
        let nonce_data = self.fetch_nonce_account_data(&nonce_account_pubkey).await?;

        tracing::info!("Building transaction instructions...");

        // Create advance nonce instruction (must be first instruction)
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &nonce_authority_keypair.pubkey(),
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");
        tracing::info!("   Nonce account: {}", nonce_account_pubkey);
        tracing::info!("   Authority: {}", nonce_authority_keypair.pubkey());

        // Create transfer instruction
        let transfer_ix = system_instruction::transfer(&sender_pubkey, &recipient_pubkey, amount);
        tracing::info!("✅ Instruction 2: Transfer {} lamports", amount);
        tracing::info!("   From: {}", sender_pubkey);
        tracing::info!("   To: {}", recipient_pubkey);

        // Create transaction with nonce advance as first instruction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, transfer_ix],
            Some(&sender_pubkey), // Sender pays the fee
        );
        tracing::info!(
            "Transaction created with {} instructions",
            transaction.message.instructions.len()
        );

        // Use the nonce account's stored blockhash
        transaction.message.recent_blockhash = nonce_data.blockhash();

        // Sign with both required signers (nonce authority and sender)
        transaction.sign(
            &[nonce_authority_keypair, sender_keypair],
            nonce_data.blockhash(),
        );

        // Serialize the signed transaction using bincode 1.x (Solana wire format)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!("Transaction serialized: {} bytes", serialized.len());

        // Compress the transaction if it exceeds the threshold
        let compressed_tx = if serialized.len() > COMPRESSION_THRESHOLD {
            tracing::info!(
                "Compressing transaction (threshold: {} bytes)",
                COMPRESSION_THRESHOLD
            );
            // Use compression with size header for proper decompression
            let compressed = self.compressor.compress_with_size(&serialized)?;
            tracing::info!(
                "Compressed: {} bytes -> {} bytes",
                serialized.len(),
                compressed.len()
            );
            compressed
        } else {
            tracing::info!("Transaction below compression threshold, keeping uncompressed");
            serialized
        };

        tracing::info!("Final transaction size: {} bytes", compressed_tx.len());
        Ok(compressed_tx)
    }

    /// Fetch nonce account data from the blockchain
    async fn fetch_nonce_account_data(
        &self,
        nonce_pubkey: &Pubkey,
    ) -> Result<solana_sdk::nonce::state::Data, TransactionError> {
        let client = self.rpc_client.as_ref().ok_or_else(|| {
            TransactionError::RpcClient(
                "RPC client not initialized. Use new_with_rpc()".to_string(),
            )
        })?;

        // Clone what we need for the blocking call
        let nonce_pubkey = *nonce_pubkey;
        let client_url = client.url();
        
        // Fetch the account in a blocking task to avoid blocking the async runtime
        let account = tokio::task::spawn_blocking(move || {
            let blocking_client = solana_client::rpc_client::RpcClient::new_with_commitment(
                client_url,
                CommitmentConfig::confirmed(),
            );
            blocking_client.get_account(&nonce_pubkey)
        })
        .await
        .map_err(|e| TransactionError::RpcClient(format!("Task join error: {}", e)))?
        .map_err(|e| {
            TransactionError::RpcClient(format!("Failed to fetch nonce account: {}", e))
        })?;

        // Verify account has sufficient data for a nonce account (80 bytes)
        if account.data.len() < 80 {
            return Err(TransactionError::InvalidNonceAccount(
                "Nonce account data is too small, may not be initialized".to_string(),
            ));
        }

        tracing::info!("Fetched nonce account data: {} bytes", account.data.len());

        // Deserialize the nonce account state using bincode
        let nonce_state: solana_sdk::nonce::state::Versions = bincode1::deserialize(&account.data)
            .map_err(|e| {
                TransactionError::Serialization(format!(
                    "Failed to deserialize nonce account: {}",
                    e
                ))
            })?;

        // Extract the nonce data from the state
        let nonce_data = match nonce_state.state() {
            solana_sdk::nonce::State::Initialized(data) => {
                tracing::info!("✅ Nonce account initialized");
                tracing::info!("Nonce authority: {}", data.authority);
                tracing::info!("Nonce blockhash: {}", data.blockhash());
                data.clone()
            }
            _ => {
                return Err(TransactionError::InvalidNonceAccount(
                    "Nonce account is not initialized".to_string(),
                ));
            }
        };

        Ok(nonce_data)
    }

    /// Prepare offline nonce data for creating transactions without internet
    /// Fetches and caches nonce account data that can be used offline
    /// 
    /// This should be called while online to prepare for offline transaction creation
    pub async fn prepare_offline_nonce_data(
        &self,
        nonce_account: &str,
    ) -> Result<CachedNonceData, TransactionError> {
        let nonce_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account: {}", e))
        })?;

        tracing::info!("Fetching nonce data for offline use: {}", nonce_account);
        let nonce_data = self.fetch_nonce_account_data(&nonce_pubkey).await?;

        let cached = CachedNonceData {
            nonce_account: nonce_account.to_string(),
            authority: nonce_data.authority.to_string(),
            blockhash: nonce_data.blockhash().to_string(),
            lamports_per_signature: nonce_data.fee_calculator.lamports_per_signature,
            cached_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            used: false, // Initialize as unused
        };

        tracing::info!("✅ Nonce data cached for offline use");
        tracing::info!("   Authority: {}", cached.authority);
        tracing::info!("   Blockhash: {}", cached.blockhash);
        tracing::info!("   Fee: {} lamports", cached.lamports_per_signature);

        Ok(cached)
    }

    /// Prepare multiple nonce accounts for offline use
    /// Smart bundle management: refreshes used nonces, creates new ones only when necessary
    /// 
    /// If bundle_file exists:
    ///   - Loads existing bundle
    ///   - Refreshes used nonces (fetches new blockhash from advanced nonces)
    ///   - Creates additional nonces ONLY if total < count
    ///   - Returns bundle with exactly 'count' nonces ready to use
    /// If bundle_file doesn't exist:
    ///   - Creates new bundle with 'count' nonce accounts
    /// 
    /// This saves money by reusing existing nonce accounts instead of creating new ones!
    /// 
    /// Returns an OfflineTransactionBundle ready to use
    pub async fn prepare_offline_bundle(
        &self,
        count: usize,
        sender_keypair: &Keypair,
        bundle_file: Option<&str>,
    ) -> Result<OfflineTransactionBundle, TransactionError> {
        let client = self.rpc_client.as_ref().ok_or_else(|| {
            TransactionError::RpcClient(
                "RPC client not initialized. Use new_with_rpc()".to_string(),
            )
        })?;

        tracing::info!("Preparing offline bundle for {} transactions...", count);
        
        // Try to load existing bundle
        let mut bundle = if let Some(file_path) = bundle_file {
            match OfflineTransactionBundle::load_from_file(file_path) {
                Ok(existing_bundle) => {
                    tracing::info!("📂 Found existing bundle: {}", file_path);
                    tracing::info!("   Total nonces: {}", existing_bundle.total_nonces());
                    tracing::info!("   Available (unused): {}", existing_bundle.available_nonces());
                    tracing::info!("   Used: {}", existing_bundle.used_nonces());
                    existing_bundle
                }
                Err(e) => {
                    tracing::info!("📂 No existing bundle found ({})", e);
                    tracing::info!("   Creating new bundle...");
                    OfflineTransactionBundle::new()
                }
            }
        } else {
            tracing::info!("📂 No bundle file specified, creating new bundle");
            OfflineTransactionBundle::new()
        };

        // Refresh used nonces (they've been advanced, fetch new blockhash)
        let used_count = bundle.used_nonces();
        if used_count > 0 {
            tracing::info!("♻️  Refreshing {} used nonce accounts (advanced)...", used_count);
            
            let mut refreshed = 0;
            for nonce in bundle.nonce_caches.iter_mut() {
                if nonce.used {
                    tracing::info!("   Refreshing nonce: {}", nonce.nonce_account);
                    
                    // Fetch updated nonce data (nonce was advanced)
                    match self.prepare_offline_nonce_data(&nonce.nonce_account).await {
                        Ok(fresh_data) => {
                            // Update with new blockhash (nonce was advanced)
                            nonce.authority = fresh_data.authority;
                            nonce.blockhash = fresh_data.blockhash;
                            nonce.lamports_per_signature = fresh_data.lamports_per_signature;
                            nonce.cached_at = fresh_data.cached_at;
                            nonce.used = false; // Mark as available again!
                            
                            refreshed += 1;
                            tracing::info!("     ✅ Refreshed with new blockhash: {}", nonce.blockhash);
                        }
                        Err(e) => {
                            tracing::warn!("     ⚠️  Failed to refresh: {}", e);
                            tracing::warn!("     Keeping nonce marked as used");
                        }
                    }
                }
            }
            
            if refreshed > 0 {
                tracing::info!("✅ Refreshed {} nonce accounts (FREE!)", refreshed);
                tracing::info!("   These nonces can be reused for new transactions");
            }
        }

        let total = bundle.total_nonces();
        let available = bundle.available_nonces();
        
        if total >= count {
            // We have enough nonce accounts (including refreshed ones)!
            tracing::info!("✅ Sufficient nonce accounts: {} total (need {})", total, count);
            if available < count {
                tracing::info!("   {} are currently available", available);
                tracing::info!("   {} were refreshed and are now available", total - available);
            }
            tracing::info!("   No new nonce accounts needed");
        } else {
            // Need to create more nonce accounts
            let needed = count - total;
            tracing::info!("⚠️  Need {} more nonce accounts (have {}, need {})", needed, total, count);
            tracing::info!("   Creating {} new nonce accounts...", needed);
            
            for i in 0..needed {
                tracing::info!("Creating nonce account {}/{}...", i + 1, needed);
                
                // Create nonce account
                let nonce_keypair = crate::nonce::create_nonce_account(client, sender_keypair)
                    .await
                    .map_err(|e| TransactionError::RpcClient(format!("Failed to create nonce account: {}", e)))?;
                
                // Wait for confirmation
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                
                // Fetch and cache nonce data
                let cached_nonce = self.prepare_offline_nonce_data(
                    &nonce_keypair.pubkey().to_string()
                ).await?;
                
                bundle.add_nonce(cached_nonce);
                tracing::info!("  ✅ Nonce account {}/{} prepared", i + 1, needed);
            }
            
            tracing::info!("✅ Created {} new nonce accounts", needed);
        }

        tracing::info!("✅ Bundle ready with {} nonce accounts", bundle.total_nonces());
        tracing::info!("   Available for offline transactions: {}", bundle.available_nonces());

        Ok(bundle)
    }

    /// Create transaction completely offline using cached nonce data
    /// NO internet connection required - all data comes from cached_nonce
    /// 
    /// This allows true offline transaction creation after preparing nonce data online
    pub fn create_offline_transaction(
        &self,
        sender_keypair: &Keypair,
        recipient: &str,
        amount: u64,
        nonce_authority_keypair: &Keypair,
        cached_nonce: &CachedNonceData,
    ) -> Result<Vec<u8>, TransactionError> {
        // Validate public keys
        let sender_pubkey = sender_keypair.pubkey();
        let recipient_pubkey = Pubkey::from_str(recipient).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid recipient: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(&cached_nonce.nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account: {}", e))
        })?;
        let nonce_blockhash = solana_sdk::hash::Hash::from_str(&cached_nonce.blockhash).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid blockhash: {}", e))
        })?;

        // Verify nonce authority matches
        if nonce_authority_keypair.pubkey().to_string() != cached_nonce.authority {
            return Err(TransactionError::InvalidPublicKey(format!(
                "Nonce authority mismatch. Expected: {}, Got: {}",
                cached_nonce.authority,
                nonce_authority_keypair.pubkey()
            )));
        }

        // Calculate age of cached data
        let age_seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - cached_nonce.cached_at;
        let age_hours = age_seconds / 3600;

        if age_hours > 24 {
            tracing::warn!("⚠️  Cached nonce data is {} hours old", age_hours);
            tracing::warn!("   Nonce may have been advanced by another party");
            tracing::warn!("   Transaction may fail if nonce is no longer valid");
        }

        tracing::info!("Creating transaction OFFLINE using cached nonce data");
        tracing::info!("   Nonce account: {}", cached_nonce.nonce_account);
        tracing::info!("   Nonce blockhash: {}", cached_nonce.blockhash);
        tracing::info!("   Cached {} hours ago", age_hours);
        tracing::info!("   NO internet connection required");

        // Create instructions (NO RPC calls needed)
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &nonce_authority_keypair.pubkey(),
        );
        tracing::info!("✅ Instruction 1: Advance nonce account (offline)");

        let transfer_ix = system_instruction::transfer(&sender_pubkey, &recipient_pubkey, amount);
        tracing::info!("✅ Instruction 2: Transfer {} lamports (offline)", amount);

        // Create transaction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, transfer_ix],
            Some(&sender_pubkey),
        );

        // Use cached blockhash (NOT recent blockhash from network)
        transaction.message.recent_blockhash = nonce_blockhash;

        // Sign offline
        transaction.sign(
            &[nonce_authority_keypair, sender_keypair],
            nonce_blockhash,
        );

        tracing::info!("✅ Transaction signed offline");

        // Serialize (still offline)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!("Transaction serialized: {} bytes", serialized.len());

        // Compress if needed (still offline)
        let compressed = if serialized.len() > COMPRESSION_THRESHOLD {
            tracing::info!("Compressing transaction (threshold: {} bytes)", COMPRESSION_THRESHOLD);
            let compressed = self.compressor.compress_with_size(&serialized)?;
            tracing::info!("Compressed: {} bytes -> {} bytes", serialized.len(), compressed.len());
            compressed
        } else {
            tracing::info!("Transaction below compression threshold, keeping uncompressed");
            serialized
        };

        tracing::info!("✅ OFFLINE transaction created: {} bytes", compressed.len());
        tracing::info!("   Ready for BLE transmission and later submission");

        Ok(compressed)
    }

    /// Create UNSIGNED offline transaction for MWA signing
    /// Takes PUBLIC KEYS only (no private keys exposed)
    /// Returns base64-encoded unsigned transaction that MWA can sign
    /// 
    /// This is the MWA-compatible version - NO signing happens in Rust
    /// The nonce authority is automatically read from the cached nonce data
    pub fn create_unsigned_offline_transaction(
        &self,
        sender_pubkey: &str,
        recipient: &str,
        amount: u64,
        _nonce_authority_pubkey: &str, // Ignored - we use authority from cached_nonce
        cached_nonce: &CachedNonceData,
    ) -> Result<String, TransactionError> {
        // Parse public keys
        let sender_pubkey = Pubkey::from_str(sender_pubkey).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid sender pubkey: {}", e))
        })?;
        let recipient_pubkey = Pubkey::from_str(recipient).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid recipient: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(&cached_nonce.nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account: {}", e))
        })?;
        let nonce_blockhash = solana_sdk::hash::Hash::from_str(&cached_nonce.blockhash).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid blockhash: {}", e))
        })?;

        // Use the nonce authority from the cached data (the actual owner)
        // This ensures we always use the correct authority regardless of parameter
        let nonce_authority_pubkey = Pubkey::from_str(&cached_nonce.authority).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce authority from cache: {}", e))
        })?;
        
        tracing::info!("📌 Using nonce authority from cached data: {}", cached_nonce.authority);

        // Calculate age of cached data
        let age_seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - cached_nonce.cached_at;
        let age_hours = age_seconds / 3600;

        if age_hours > 24 {
            tracing::warn!("⚠️  Cached nonce data is {} hours old", age_hours);
            tracing::warn!("   Nonce may have been advanced by another party");
        }

        tracing::info!("🔓 Creating UNSIGNED transaction for MWA signing");
        tracing::info!("   Sender: {}", sender_pubkey);
        tracing::info!("   Recipient: {}", recipient_pubkey);
        tracing::info!("   Amount: {} lamports", amount);
        tracing::info!("   Nonce account: {}", cached_nonce.nonce_account);
        tracing::info!("   NO private keys involved - MWA will sign");

        // Create instructions (NO signing)
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &nonce_authority_pubkey,
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");

        let transfer_ix = system_instruction::transfer(&sender_pubkey, &recipient_pubkey, amount);
        tracing::info!("✅ Instruction 2: Transfer {} lamports", amount);

        // Create UNSIGNED transaction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, transfer_ix],
            Some(&sender_pubkey),
        );

        // Use cached blockhash
        transaction.message.recent_blockhash = nonce_blockhash;

        // DO NOT SIGN - leave signatures empty for MWA
        tracing::info!("✅ Unsigned transaction created");
        tracing::info!("   Signers needed: nonce authority, sender");
        tracing::info!("   Ready for MWA signing");

        // Serialize unsigned transaction
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        // Encode to base64 for transport
        let base64_tx = base64::encode(&serialized);

        tracing::info!("✅ Unsigned transaction: {} bytes (base64: {} chars)", 
            serialized.len(), base64_tx.len());

        Ok(base64_tx)
    }

    /// Extract the message that needs to be signed for MWA
    /// Returns the raw message bytes that MWA/Seed Vault will sign
    pub fn get_transaction_message_to_sign(
        &self,
        base64_tx: &str,
    ) -> Result<Vec<u8>, TransactionError> {
        // Decode from base64
        let tx_bytes = base64::decode(base64_tx).map_err(|e| {
            TransactionError::Serialization(format!("Failed to decode base64: {}", e))
        })?;

        // Deserialize transaction
        let tx: Transaction = bincode1::deserialize(&tx_bytes).map_err(|e| {
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        // The message to sign is the serialized message
        let message_bytes = tx.message_data();

        tracing::info!("📝 Extracted message to sign: {} bytes", message_bytes.len());

        Ok(message_bytes)
    }

    /// Get list of public keys that need to sign this transaction
    /// Returns array of public keys in the order they need to sign
    pub fn get_required_signers(
        &self,
        base64_tx: &str,
    ) -> Result<Vec<String>, TransactionError> {
        // Decode from base64
        let tx_bytes = base64::decode(base64_tx).map_err(|e| {
            TransactionError::Serialization(format!("Failed to decode base64: {}", e))
        })?;

        // Deserialize transaction
        let tx: Transaction = bincode1::deserialize(&tx_bytes).map_err(|e| {
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        // Get signers from message header
        let num_required_signatures = tx.message.header.num_required_signatures as usize;
        let signers: Vec<String> = tx.message.account_keys[..num_required_signatures]
            .iter()
            .map(|key| key.to_string())
            .collect();

        tracing::info!("👥 Required signers: {:?}", signers);

        Ok(signers)
    }

    /// Create unsigned nonce account creation transactions for MWA signing
    /// 
    /// This generates N unsigned transactions that create nonce accounts.
    /// Each transaction needs to be co-signed by:
    /// 1. The ephemeral nonce account keypair (generated here, returned to caller)
    /// 2. The payer (signed by MWA wallet)
    /// 
    /// Workflow:
    /// 1. Generate nonce keypairs
    /// 2. Create unsigned transactions with nonce account creation instructions
    /// 3. Return transactions + nonce keypairs to Kotlin
    /// 4. Kotlin signs with nonce keypairs
    /// 5. MWA co-signs with payer keypair
    /// 6. Submit fully signed transactions
    /// 
    /// Returns: Vec of (unsigned_tx_base64, nonce_keypair_base64, nonce_pubkey)
    pub async fn create_unsigned_nonce_transactions(
        &self,
        count: usize,
        payer_pubkey_str: &str,
    ) -> Result<Vec<crate::ffi::types::UnsignedNonceTransaction>, TransactionError> {
        use crate::ffi::types::UnsignedNonceTransaction;
        
        let client = self.rpc_client.as_ref().ok_or_else(|| {
            TransactionError::RpcClient(
                "RPC client not initialized. Use new_with_rpc()".to_string(),
            )
        })?;

        tracing::info!("Creating {} unsigned nonce account transactions", count);

        // Parse payer pubkey
        let payer_pubkey = Pubkey::from_str(payer_pubkey_str).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid payer pubkey: {}", e))
        })?;

        // Get rent exemption amount
        let rent_exemption = client
            .get_minimum_balance_for_rent_exemption(solana_sdk::nonce::State::size())
            .map_err(|e| TransactionError::RpcClient(format!("Failed to get rent exemption: {}", e)))?;
        
        tracing::info!("Rent exemption for nonce account: {} lamports", rent_exemption);

        // Get recent blockhash
        let recent_blockhash = client
            .get_latest_blockhash()
            .map_err(|e| TransactionError::RpcClient(format!("Failed to get blockhash: {}", e)))?;

        let mut result = Vec::new();

        for i in 0..count {
            // Generate ephemeral nonce keypair
            let nonce_keypair = Keypair::new();
            let nonce_pubkey = nonce_keypair.pubkey();

            tracing::info!("Transaction {}/{}: Nonce account {}", i + 1, count, nonce_pubkey);

            // Create nonce account instructions
            let create_nonce_instructions = system_instruction::create_nonce_account(
                &payer_pubkey,         // funding account
                &nonce_pubkey,         // nonce account
                &payer_pubkey,         // authority (set to payer)
                rent_exemption,        // lamports
            );

            // Create transaction (completely unsigned)
            let mut tx = Transaction::new_with_payer(
                &create_nonce_instructions,
                Some(&payer_pubkey),
            );
            tx.message.recent_blockhash = recent_blockhash;

            // DO NOT sign yet - keep it completely unsigned
            // MWA will add payer signature first, then we'll add nonce signature
            tracing::info!("Creating unsigned transaction (no signatures yet)");

            // Serialize unsigned transaction
            let tx_bytes = bincode1::serialize(&tx).map_err(|e| {
                TransactionError::Serialization(format!("Failed to serialize transaction: {}", e))
            })?;

            // Serialize nonce keypair (will be used to add signature after MWA signs)
            let nonce_keypair_bytes = nonce_keypair.to_bytes();

            result.push(UnsignedNonceTransaction {
                unsigned_transaction_base64: base64::encode(&tx_bytes),
                nonce_keypair_base64: base64::encode(&nonce_keypair_bytes),
                nonce_pubkey: nonce_pubkey.to_string(),
            });
        }

        tracing::info!("✅ Created {} unsigned nonce transactions", result.len());

        Ok(result)
    }

    /// Submit offline-created transaction to blockchain
    /// Optionally verifies nonce is still valid before submission
    /// 
    /// Returns transaction signature if successful
    pub async fn submit_offline_transaction(
        &self,
        compressed_tx: &[u8],
        verify_nonce: bool,
    ) -> Result<String, TransactionError> {
        tracing::info!("Submitting offline-created transaction");

        // Decompress if needed
        let decompressed = if compressed_tx.len() >= 8 && compressed_tx.starts_with(b"LZ4") {
            tracing::info!("✅ Detected LZ4 compression, decompressing...");
            self.compressor.decompress_with_size(compressed_tx)?
        } else {
            tracing::info!("No compression detected, using raw data");
            compressed_tx.to_vec()
        };

        // Deserialize transaction
        let tx: Transaction = bincode1::deserialize(&decompressed).map_err(|e| {
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        tracing::info!("✅ Transaction deserialized");
        tracing::info!("   Signatures: {}", tx.signatures.len());
        tracing::info!("   Instructions: {}", tx.message.instructions.len());
        tracing::info!("   Blockhash: {}", tx.message.recent_blockhash);

        // Verify signatures locally before submitting to RPC
        let required_signers = tx.message.header.num_required_signatures as usize;
        let signer_keys: Vec<String> = tx.message.account_keys
            .iter()
            .take(required_signers)
            .map(|k| k.to_string())
            .collect();

        tracing::info!("   Required signers ({}): {:?}", required_signers, signer_keys);

        if let Err(err) = tx.verify() {
            tracing::error!("❌ Local signature verification failed before submission: {}", err);
            for (index, signature) in tx.signatures.iter().enumerate() {
                tracing::error!("   Signature[{}]: {}", index, signature);
            }

            return Err(TransactionError::RpcClient(
                format!("Transaction signature verification failed locally: {}", err),
            ));
        }

        // Optional: Verify nonce account hasn't been advanced
        if verify_nonce {
            tracing::info!("Verifying nonce account is still valid...");

            // Extract nonce account from first instruction
            if let Some(first_ix) = tx.message.instructions.first() {
                if !first_ix.accounts.is_empty() {
                    let nonce_account = &tx.message.account_keys[first_ix.accounts[0] as usize];

                    match self.fetch_nonce_account_data(nonce_account).await {
                        Ok(current_nonce) => {
                            if current_nonce.blockhash() != tx.message.recent_blockhash {
                                tracing::error!("❌ Nonce verification FAILED!");
                                tracing::error!("   Transaction blockhash: {}", tx.message.recent_blockhash);
                                tracing::error!("   Current nonce blockhash: {}", current_nonce.blockhash());
                                return Err(TransactionError::InvalidNonceAccount(
                                    "Nonce has been advanced, transaction is now invalid".to_string()
                                ));
                            }
                            tracing::info!("✅ Nonce verification passed");
                            tracing::info!("   Nonce is still valid and matches transaction");

                            // todo: resend a message to origin to resend transaction with another detail
                        }
                        Err(e) => {
                            tracing::warn!("⚠️  Could not verify nonce: {}", e);
                            tracing::warn!("   Proceeding with submission anyway");
                        }
                    }
                }
            }
        } else {
            tracing::info!("Skipping nonce verification (not requested)");
        }

        // Submit to blockchain
        let client = self.rpc_client.as_ref().ok_or_else(|| {
            TransactionError::RpcClient("RPC client not initialized. Use new_with_rpc()".to_string())
        })?;

        tracing::info!("Submitting transaction to Solana...");
        let signature = client.send_and_confirm_transaction(&tx).map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Blockhash not found") {
                tracing::error!("❌ Blockhash not found - nonce was likely advanced");
                TransactionError::InvalidNonceAccount(
                    "Nonce has been advanced, transaction invalid".to_string()
                )
            } else {
                TransactionError::RpcClient(format!("Failed to submit transaction: {}", e))
            }
        })?;

        tracing::info!("✅ Offline transaction submitted successfully!");
        tracing::info!("   Signature: {}", signature);

        Ok(signature.to_string())
    }

    /// Process and relay a presigned custom transaction
    /// Takes a presigned transaction (base64), compresses, fragments, and returns fragments for relay
    /// Returns fragments ready for BLE transmission
    pub async fn process_and_relay_transaction(
        &self,
        base64_signed_tx: &str,
    ) -> Result<Vec<Fragment>, TransactionError> {
        tracing::info!("Processing presigned custom transaction for relay");

        // Decode from base64
        let tx_bytes = base64::decode(base64_signed_tx).map_err(|e| {
            TransactionError::Serialization(format!("Failed to decode base64: {}", e))
        })?;

        tracing::info!("Decoded transaction: {} bytes", tx_bytes.len());

        // Validate it's a signed transaction
        let tx: Transaction = bincode1::deserialize(&tx_bytes).map_err(|e| {
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        // Verify transaction has signatures
        let valid_sigs = tx
            .signatures
            .iter()
            .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
            .count();

        if valid_sigs == 0 {
            return Err(TransactionError::Serialization(
                "Transaction must be signed before processing for relay".to_string(),
            ));
        }

        tracing::info!("✅ Transaction validated");
        tracing::info!(
            "   Valid signatures: {}/{}",
            valid_sigs,
            tx.signatures.len()
        );
        tracing::info!("   Instructions: {}", tx.message.instructions.len());

        // Compress the transaction if it exceeds the threshold
        let compressed_tx = if tx_bytes.len() > COMPRESSION_THRESHOLD {
            tracing::info!(
                "Compressing transaction (threshold: {} bytes)",
                COMPRESSION_THRESHOLD
            );
            let compressed = self.compressor.compress_with_size(&tx_bytes)?;
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
        let fragments = self.fragment_transaction(&compressed_tx);

        tracing::info!("✅ Transaction processed and ready for relay");
        tracing::info!(
            "   Created {} fragments for BLE transmission",
            fragments.len()
        );
        tracing::info!("   Each fragment has SHA-256 checksum for integrity verification");

        Ok(fragments)
    }

    /// Fragment a transaction for BLE transmission
    /// Each fragment includes a SHA-256 checksum of the complete transaction for verification
    pub fn fragment_transaction(&self, compressed_tx: &[u8]) -> Vec<Fragment> {
        use sha2::{Digest, Sha256};

        let mut fragments = Vec::new();
        let total_fragments = (compressed_tx.len() + BLE_MTU_SIZE - 1) / BLE_MTU_SIZE;
        let tx_id = self.generate_tx_id();

        // Calculate SHA-256 checksum of the complete transaction
        let mut hasher = Sha256::new();
        hasher.update(compressed_tx);
        let checksum: [u8; 32] = hasher.finalize().into();

        tracing::info!(
            "Fragmenting transaction: {} bytes into {} fragments",
            compressed_tx.len(),
            total_fragments
        );
        tracing::info!("Transaction checksum: {}", hex::encode(checksum));

        for (i, chunk) in compressed_tx.chunks(BLE_MTU_SIZE).enumerate() {
            let fragment_type = if i == 0 {
                FragmentType::FragmentStart
            } else if i == total_fragments - 1 {
                FragmentType::FragmentEnd
            } else {
                FragmentType::FragmentContinue
            };

            fragments.push(Fragment {
                id: tx_id.clone(),
                index: i,
                total: total_fragments,
                data: chunk.to_vec(),
                fragment_type,
                checksum, // Same checksum for all fragments
            });
        }

        tracing::info!(
            "Created {} fragments with checksum verification",
            fragments.len()
        );
        fragments
    }

    /// Reassemble fragments back into a complete transaction
    /// Verifies checksum to ensure data integrity
    pub fn reassemble_fragments(
        &self,
        fragments: &[Fragment],
    ) -> Result<Vec<u8>, TransactionError> {
        use sha2::{Digest, Sha256};

        if fragments.is_empty() {
            return Err(TransactionError::Serialization(
                "No fragments to reassemble".to_string(),
            ));
        }

        tracing::info!("Reassembling {} fragments", fragments.len());

        // Get expected checksum from first fragment (all fragments should have the same checksum)
        let expected_checksum = fragments[0].checksum;
        tracing::info!("Expected checksum: {}", hex::encode(expected_checksum));

        // Verify all fragments have the same checksum
        for (i, fragment) in fragments.iter().enumerate() {
            if fragment.checksum != expected_checksum {
                return Err(TransactionError::Serialization(format!(
                    "Fragment {} has mismatched checksum. Expected: {}, Got: {}",
                    i,
                    hex::encode(expected_checksum),
                    hex::encode(fragment.checksum)
                )));
            }
        }
        tracing::info!("✅ All fragments have matching checksum");

        // Sort fragments by index
        let mut sorted_fragments = fragments.to_vec();
        sorted_fragments.sort_by_key(|f| f.index);

        // Verify we have all fragments
        let expected_count = sorted_fragments[0].total;
        if sorted_fragments.len() != expected_count {
            return Err(TransactionError::Serialization(format!(
                "Missing fragments: expected {}, got {}",
                expected_count,
                sorted_fragments.len()
            )));
        }

        tracing::info!("All {} fragments present", expected_count);

        // Reassemble data
        let mut reassembled = Vec::new();
        for (i, fragment) in sorted_fragments.iter().enumerate() {
            tracing::debug!(
                "Adding fragment {}/{}: {} bytes",
                i + 1,
                expected_count,
                fragment.data.len()
            );
            reassembled.extend_from_slice(&fragment.data);
        }

        tracing::info!("Reassembled total: {} bytes", reassembled.len());

        // Verify checksum of reassembled data
        let mut hasher = Sha256::new();
        hasher.update(&reassembled);
        let actual_checksum: [u8; 32] = hasher.finalize().into();

        if actual_checksum != expected_checksum {
            return Err(TransactionError::Serialization(format!(
                "Checksum verification failed! Expected: {}, Got: {}",
                hex::encode(expected_checksum),
                hex::encode(actual_checksum)
            )));
        }

        tracing::info!("✅ Checksum verification passed");
        tracing::info!("Reassembled checksum: {}", hex::encode(actual_checksum));

        Ok(reassembled)
    }

    /// Submit a transaction to Solana RPC
    /// Handles both compressed and uncompressed transactions
    pub async fn submit_to_solana(&self, transaction: &[u8]) -> Result<String, TransactionError> {
        let client = self.rpc_client.as_ref().ok_or_else(|| {
            TransactionError::RpcClient(
                "RPC client not initialized. Use new_with_rpc()".to_string(),
            )
        })?;

        tracing::info!("Received transaction: {} bytes", transaction.len());

        // Check first few bytes to detect compression
        if transaction.len() >= 4 {
            tracing::info!("First 4 bytes: {:02x?}", &transaction[..4]);
        }

        // Decompress if needed - check for "LZ4" header (3 bytes: 0x4c 0x5a 0x34)
        let decompressed = if transaction.len() >= 8 && transaction.starts_with(b"LZ4") {
            tracing::info!("✅ Detected LZ4 compression header");
            tracing::info!("Decompressing transaction ({} bytes)...", transaction.len());
            // Decompress the transaction
            let result = self.compressor.decompress_with_size(transaction)?;
            tracing::info!(
                "Decompressed: {} bytes -> {} bytes",
                transaction.len(),
                result.len()
            );
            result
        } else {
            tracing::info!("No LZ4 compression header detected, using raw data");
            transaction.to_vec()
        };

        tracing::info!(
            "Deserializing transaction ({} bytes)...",
            decompressed.len()
        );
        tracing::info!(
            "First 16 bytes: {:02x?}",
            &decompressed[..decompressed.len().min(16)]
        );

        // Deserialize the transaction using bincode 1.x (Solana wire format)
        let tx: Transaction = bincode1::deserialize(&decompressed).map_err(|e| {
            tracing::error!("Deserialization failed!");
            tracing::error!("Data length: {}", decompressed.len());
            tracing::error!("Error: {}", e);
            TransactionError::Serialization(format!("Failed to deserialize transaction: {}", e))
        })?;

        tracing::info!("✅ Transaction deserialized successfully");
        tracing::info!("Transaction has {} signatures", tx.signatures.len());
        tracing::info!(
            "Transaction has {} instructions",
            tx.message.instructions.len()
        );

        // Submit to Solana
        tracing::info!("Submitting transaction to Solana...");
        let signature = client.send_and_confirm_transaction(&tx).map_err(|e| {
            TransactionError::RpcClient(format!("Failed to submit transaction: {}", e))
        })?;

        tracing::info!("✅ Transaction submitted successfully: {}", signature);

        Ok(signature.to_string())
    }

    /// Broadcast confirmation after successful submission
    pub async fn broadcast_confirmation(&self, signature: &str) -> Result<(), TransactionError> {
        // Create confirmation packet
        let confirmation_packet = ConfirmationPacket {
            tx_id: self.generate_tx_id(),
            signature: signature.to_string(),
            new_nonce: 0, // Placeholder - would query from nonce account in production
        };

        // Serialize and broadcast over BLE
        let serialized = serde_json::to_vec(&confirmation_packet)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        // This would integrate with BLE transport
        // For now, just log the confirmation
        // todo: integrate with BLE transport
        tracing::info!("Broadcasting confirmation: {:?}", confirmation_packet);

        Ok(())
    }

    /// Cast a governance vote with durable nonce
    /// Creates a presigned vote transaction using a nonce account for longer lifetime
    pub async fn cast_vote(
        &self,
        voter_keypair: &Keypair,
        proposal_id: &str,
        vote_account: &str,
        choice: u8,
        nonce_account: &str,
    ) -> Result<Vec<u8>, TransactionError> {
        // Validate public keys
        let proposal_pubkey = Pubkey::from_str(proposal_id).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid proposal ID: {}", e))
        })?;
        let vote_account_pubkey = Pubkey::from_str(vote_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid vote account: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account: {}", e))
        })?;

        // Fetch nonce account data to get the blockhash
        tracing::info!("Fetching nonce account data from blockchain...");
        let nonce_data = self.fetch_nonce_account_data(&nonce_account_pubkey).await?;

        tracing::info!("Building governance vote instructions...");

        // Create advance nonce instruction (must be first instruction)
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &voter_keypair.pubkey(),
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");
        tracing::info!("   Nonce account: {}", nonce_account_pubkey);
        tracing::info!("   Authority: {} (voter)", voter_keypair.pubkey());

        // Create vote instruction
        let vote_ix = self.build_cast_vote_instruction(proposal_id, vote_account, choice)?;
        tracing::info!("✅ Instruction 2: Cast vote");
        tracing::info!("   Proposal: {}", proposal_pubkey);
        tracing::info!("   Vote account: {}", vote_account_pubkey);
        tracing::info!("   Choice: {}", choice);

        // Create transaction with nonce advance as first instruction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, vote_ix],
            Some(&voter_keypair.pubkey()), // Voter pays the fee
        );
        tracing::info!(
            "Vote transaction created with {} instructions",
            transaction.message.instructions.len()
        );

        // Use the nonce account's stored blockhash
        transaction.message.recent_blockhash = nonce_data.blockhash();

        // Sign with voter (as both nonce authority and voter)
        transaction.sign(&[voter_keypair], nonce_data.blockhash());

        // Serialize the signed transaction using bincode 1.x (Solana wire format)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!("Vote transaction serialized: {} bytes", serialized.len());

        // Compress the transaction if it exceeds the threshold
        let compressed_tx = if serialized.len() > COMPRESSION_THRESHOLD {
            tracing::info!(
                "Compressing vote transaction (threshold: {} bytes)",
                COMPRESSION_THRESHOLD
            );
            let compressed = self.compressor.compress_with_size(&serialized)?;
            tracing::info!(
                "Compressed: {} bytes -> {} bytes",
                serialized.len(),
                compressed.len()
            );
            compressed
        } else {
            tracing::info!("Vote transaction below compression threshold, keeping uncompressed");
            serialized
        };

        tracing::info!("Final vote transaction size: {} bytes", compressed_tx.len());
        Ok(compressed_tx)
    }

    /// Create and sign a new SPL token transfer transaction with durable nonce
    /// Creates a presigned SPL token transaction using a nonce account for longer lifetime
    /// Automatically derives Associated Token Accounts from wallet pubkeys and mint address
    pub async fn create_spl_transaction(
        &self,
        sender_wallet: &str,
        sender_keypair: &Keypair,
        recipient_wallet: &str,
        mint_address: &str,
        amount: u64,
        nonce_account: &str,
        nonce_authority_keypair: &Keypair,
    ) -> Result<Vec<u8>, TransactionError> {
        // Validate public keys
        let sender_pubkey = Pubkey::from_str(sender_wallet).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid sender wallet: {}", e))
        })?;
        let recipient_pubkey = Pubkey::from_str(recipient_wallet).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid recipient wallet: {}", e))
        })?;
        let mint_pubkey = Pubkey::from_str(mint_address).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid mint address: {}", e))
        })?;
        let nonce_account_pubkey = Pubkey::from_str(nonce_account).map_err(|e| {
            TransactionError::InvalidPublicKey(format!("Invalid nonce account public key: {}", e))
        })?;

        // Verify sender keypair matches sender pubkey
        if sender_keypair.pubkey() != sender_pubkey {
            return Err(TransactionError::InvalidPublicKey(
                "Sender keypair does not match sender wallet".to_string(),
            ));
        }

        // Derive Associated Token Accounts
        let sender_token_account = spl_associated_token_account::get_associated_token_address(
            &sender_pubkey,
            &mint_pubkey,
        );
        let recipient_token_account = spl_associated_token_account::get_associated_token_address(
            &recipient_pubkey,
            &mint_pubkey,
        );

        tracing::info!("Derived Associated Token Accounts:");
        tracing::info!("  Sender ATA: {}", sender_token_account);
        tracing::info!("  Recipient ATA: {}", recipient_token_account);
        tracing::info!("  Mint: {}", mint_pubkey);

        // Fetch nonce account data to get the blockhash
        tracing::info!("Fetching nonce account data from blockchain...");
        let nonce_data = self.fetch_nonce_account_data(&nonce_account_pubkey).await?;

        tracing::info!("Building SPL token transfer instructions...");

        // Create advance nonce instruction (must be first instruction)
        let advance_nonce_ix = system_instruction::advance_nonce_account(
            &nonce_account_pubkey,
            &nonce_authority_keypair.pubkey(),
        );
        tracing::info!("✅ Instruction 1: Advance nonce account");
        tracing::info!("   Nonce account: {}", nonce_account_pubkey);
        tracing::info!("   Authority: {}", nonce_authority_keypair.pubkey());

        // Create SPL token transfer instruction
        let spl_transfer_ix = spl_instruction::transfer(
            &spl_token::id(),
            &sender_token_account,
            &recipient_token_account,
            &sender_keypair.pubkey(), // Owner of sender token account
            &[],                      // No multisig signers
            amount,
        )
        .map_err(|e| TransactionError::SolanaInstruction(e.to_string()))?;

        tracing::info!("✅ Instruction 2: SPL Token Transfer {} tokens", amount);
        tracing::info!("   From token account: {}", sender_token_account);
        tracing::info!("   To token account: {}", recipient_token_account);
        tracing::info!("   Owner: {}", sender_keypair.pubkey());

        // Create transaction with nonce advance as first instruction
        let mut transaction = Transaction::new_with_payer(
            &[advance_nonce_ix, spl_transfer_ix],
            Some(&sender_keypair.pubkey()), // Sender pays the fee
        );
        tracing::info!(
            "SPL transaction created with {} instructions",
            transaction.message.instructions.len()
        );

        // Use the nonce account's stored blockhash
        transaction.message.recent_blockhash = nonce_data.blockhash();

        // Sign with both required signers (nonce authority and sender)
        transaction.sign(
            &[nonce_authority_keypair, sender_keypair],
            nonce_data.blockhash(),
        );

        // Serialize the signed transaction using bincode 1.x (Solana wire format)
        let serialized = bincode1::serialize(&transaction)
            .map_err(|e| TransactionError::Serialization(e.to_string()))?;

        tracing::info!("SPL transaction serialized: {} bytes", serialized.len());

        // Compress the transaction if it exceeds the threshold
        let compressed_tx = if serialized.len() > COMPRESSION_THRESHOLD {
            tracing::info!(
                "Compressing SPL transaction (threshold: {} bytes)",
                COMPRESSION_THRESHOLD
            );
            let compressed = self.compressor.compress_with_size(&serialized)?;
            tracing::info!(
                "Compressed: {} bytes -> {} bytes",
                serialized.len(),
                compressed.len()
            );
            compressed
        } else {
            tracing::info!("SPL transaction below compression threshold, keeping uncompressed");
            serialized
        };

        tracing::info!("Final SPL transaction size: {} bytes", compressed_tx.len());
        Ok(compressed_tx)
    }

    /// Build cast vote instruction (example governance use case)
    fn build_cast_vote_instruction(
        &self,
        proposal_id: &str,
        vote_account: &str,
        choice: u8,
    ) -> Result<Instruction, TransactionError> {
        // This would integrate with actual governance program
        // For now, create a mock instruction using system transfer
        let proposal_pubkey = Pubkey::from_str(proposal_id)
            .map_err(|e| TransactionError::InvalidPublicKey(e.to_string()))?;
        let vote_account_pubkey = Pubkey::from_str(vote_account)
            .map_err(|e| TransactionError::InvalidPublicKey(e.to_string()))?;

        // Mock governance instruction - in production this would use actual governance program
        // For demonstration, we use a transfer instruction
        let instruction = system_instruction::transfer(
            &vote_account_pubkey,
            &proposal_pubkey,
            choice as u64, // Using choice as amount for demo
        );

        Ok(instruction)
    }
    /// Generate unique transaction ID
    fn generate_tx_id(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("tx_{:x}", timestamp)
    }
}


/// Confirmation packet for successful transaction submission
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfirmationPacket {
    /// Transaction ID
    pub tx_id: String,
    /// Solana transaction signature
    pub signature: String,
    /// New nonce value
    pub new_nonce: u64,
}

/// Error types for transaction operations
#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Solana instruction error: {0}")]
    SolanaInstruction(String),

    #[error("RPC client error: {0}")]
    RpcClient(String),

    #[error("Invalid nonce account: {0}")]
    InvalidNonceAccount(String),
}

impl From<crate::util::lz::Lz4Error> for TransactionError {
    fn from(err: crate::util::lz::Lz4Error) -> Self {
        TransactionError::Compression(err.to_string())
    }
}

// Re-export for convenience
pub use solana_sdk::hash::Hash;

pub mod pollinet_message;
pub use pollinet_message::{HopRecord, PolliNetMessage};

// MWA integration tests
#[cfg(test)]
mod mwa_tests;
