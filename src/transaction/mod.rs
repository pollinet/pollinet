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
#[derive(Debug, Clone)]
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

        // Fetch the account
        let account = client.get_account(nonce_pubkey).map_err(|e| {
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
