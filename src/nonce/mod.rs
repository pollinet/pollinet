//! Nonce account management for PolliNet SDK
//! 
//! Handles Solana nonce accounts to extend transaction lifespan beyond recent blockhash constraints

use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;
use solana_sdk::{
    pubkey::Pubkey,
    hash::Hash,
    instruction::Instruction,
    system_instruction,
    signature::{Keypair, Signer},
    transaction::Transaction,
    commitment_config::CommitmentConfig,
};

#[cfg(feature = "rpc-client")]
use solana_client::rpc_client::RpcClient;
#[cfg(feature = "rpc-client")]
use solana_client::rpc_filter::{RpcFilterType, Memcmp, MemcmpEncodedBytes};
#[cfg(feature = "rpc-client")]
use solana_account_decoder::UiAccountEncoding;
#[cfg(feature = "rpc-client")]
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};

/// Nonce account manager for PolliNet
pub struct NonceManager {
    /// Current nonce account public key
    nonce_account: Pubkey,
    /// Current nonce value
    current_nonce: Arc<RwLock<u64>>,
    /// Authority public key
    authority: Pubkey,
    /// RPC client for blockchain operations
    #[cfg(feature = "rpc-client")]
    rpc_client: Option<RpcClient>,
    #[cfg(not(feature = "rpc-client"))]
    rpc_client: Option<()>,
}

impl NonceManager {
    /// Create a new nonce manager
    pub async fn new() -> Result<Self, NonceError> {
        // For now, use a mock nonce account
        // In production, this would create or load an existing nonce account
        let nonce_account = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        
        Ok(Self {
            nonce_account,
            current_nonce: Arc::new(RwLock::new(0)),
            authority,
            rpc_client: None,
        })
    }
    
    /// Create a new nonce manager with RPC client
    #[cfg(feature = "rpc-client")]
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, NonceError> {
        let nonce_account = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let rpc_client = RpcClient::new_with_commitment(
            rpc_url.to_string(),
            CommitmentConfig::confirmed(),
        );
        
        Ok(Self {
            nonce_account,
            current_nonce: Arc::new(RwLock::new(0)),
            authority,
            rpc_client: Some(rpc_client),
        })
    }
    
    #[cfg(not(feature = "rpc-client"))]
    pub async fn new_with_rpc(_rpc_url: &str) -> Result<Self, NonceError> {
        Err(NonceError::RpcError("RPC client not enabled for this build. Compile with 'rpc-client' feature.".to_string()))
    }
    
    /// Check if a nonce account exists and is valid
    #[cfg(feature = "rpc-client")]
    pub async fn check_nonce_account_exists(
        &self,
        nonce_pubkey: &Pubkey,
    ) -> Result<bool, NonceError> {
        let client = self.rpc_client.as_ref()
            .ok_or_else(|| NonceError::RpcError("RPC client not initialized".to_string()))?;
        
        match client.get_account(nonce_pubkey) {
            Ok(account) => {
                // Check if account has enough data for a nonce account
                if account.data.len() >= 80 {
                    tracing::info!("✅ Existing nonce account found: {}", nonce_pubkey);
                    Ok(true)
                } else {
                    tracing::warn!("Account exists but is not a valid nonce account");
                    Ok(false)
                }
            }
            Err(_) => {
                tracing::info!("No existing nonce account found for: {}", nonce_pubkey);
                Ok(false)
            }
        }
    }
    
    #[cfg(not(feature = "rpc-client"))]
    pub async fn check_nonce_account_exists(
        &self,
        _nonce_pubkey: &Pubkey,
    ) -> Result<bool, NonceError> {
        Err(NonceError::RpcError(
            "RPC not available on iOS. Use native URLSession for nonce checks.".to_string()
        ))
    }
    
    /// Get the current nonce value
    pub async fn get_current_nonce(&self) -> Result<u64, NonceError> {
        let nonce = self.current_nonce.read().await;
        Ok(*nonce)
    }
    
    /// Advance the nonce value
    pub async fn advance_nonce(&self) -> Result<(), NonceError> {
        let mut nonce = self.current_nonce.write().await;
        *nonce += 1;
        Ok(())
    }
    
    /// Create a nonce account instruction
    pub fn create_nonce_account_instruction(
        &self,
        payer: &Pubkey,
        nonce_account: &Pubkey,
        authority: &Pubkey,
        recent_blockhash: &Hash,
    ) -> Result<Instruction, NonceError> {
        // Mock instruction for now
        // In production, this would create a proper nonce account instruction
        let instruction = system_instruction::transfer(
            payer,
            nonce_account,
            1000000, // 0.001 SOL
        );
        
        Ok(instruction)
    }
    
    /// Create an advance nonce account instruction
    pub fn advance_nonce_account_instruction(
        &self,
        nonce_account: &Pubkey,
        authority: &Pubkey,
    ) -> Result<Instruction, NonceError> {
        let instruction = system_instruction::advance_nonce_account(
            nonce_account,
            authority,
        );
        
        Ok(instruction)
    }
    
    /// Get the nonce account public key
    pub fn get_nonce_account(&self) -> Pubkey {
        self.nonce_account
    }
    
    /// Get the authority public key
    pub fn get_authority(&self) -> Pubkey {
        self.authority
    }
}

/// Parse nonce account data to extract authority and current nonce
fn parse_nonce_account(data: &[u8]) -> Result<NonceData, NonceError> {
    if data.len() != 80 {
        return Err(NonceError::InvalidAccountData("Invalid nonce account size".to_string()));
    }

    // Parse version (first 4 bytes)
    let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    
    if version != 1 {
        return Err(NonceError::InvalidAccountData(
            format!("Invalid nonce version: {}", version)
        ));
    }

    // Parse authority (bytes 4-35)
    let authority = Pubkey::try_from(&data[4..36])
        .map_err(|_| NonceError::InvalidAccountData("Invalid authority pubkey".to_string()))?;

    // Parse blockhash/nonce (bytes 36-67)
    let blockhash = Hash::new(&data[36..68]);

    Ok(NonceData {
        authority,
        blockhash,
    })
}

/// Check if a specific nonce account exists
#[cfg(feature = "rpc-client")]
pub async fn check_nonce_account_exists(
    client: &RpcClient,
    nonce_pubkey: &Pubkey,
) -> Result<bool, NonceError> {
    match client.get_account(nonce_pubkey) {
        Ok(account) => {
            if account.data.len() == 80 && account.owner == solana_sdk::system_program::id() {
                // Verify it's a valid nonce account
                if parse_nonce_account(&account.data).is_ok() {
                    tracing::info!("✅ Existing nonce account found: {}", nonce_pubkey);
                    return Ok(true);
                }
            }
            tracing::warn!("Account exists but is not a valid nonce account");
            Ok(false)
        }
        Err(_) => {
            tracing::info!("No existing nonce account found for: {}", nonce_pubkey);
            Ok(false)
        }
    }
}

/// Find all nonce accounts where the sender is the authority
#[cfg(feature = "rpc-client")]
pub async fn find_nonce_accounts_by_authority(
    client: &RpcClient,
    authority_pubkey: &Pubkey,
) -> Result<Vec<(Pubkey, Hash)>, NonceError> {
    // The nonce account data structure:
    // Bytes 0-3: Version (u32)
    // Bytes 4-35: Authority pubkey (32 bytes)
    // Bytes 36-67: Blockhash (32 bytes)
    // Bytes 68-79: Fee calculator (12 bytes)
    
    tracing::info!("Searching for nonce accounts with authority: {}", authority_pubkey);
    
    // Create a filter to match accounts where authority equals our pubkey
    let filters = vec![
        // Filter 1: Account must be exactly 128 bytes (nonce account size)
        RpcFilterType::DataSize(128),
        // Filter 2: Match authority pubkey at offset 4
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            4, // Offset: skip first 4 bytes (version)
            authority_pubkey.to_bytes().to_vec(),
        )),
    ];

    let config = RpcProgramAccountsConfig {
        filters: Some(filters),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            data_slice: None,
            commitment: None,
            min_context_slot: None,
        },
        with_context: None,
        sort_results: None,
    };

    // Query all accounts owned by System Program with our filters
    let accounts = client
        .get_program_accounts_with_config(&solana_sdk::system_program::id(), config)
        .map_err(|e| {
            tracing::error!("Failed to fetch nonce accounts: {}", e);
            NonceError::RpcError(e.to_string())
        })?;

    let mut nonce_accounts = Vec::new();

    for (pubkey, account) in accounts {
        // Verify it's actually a nonce account by parsing the data
        if let Ok(nonce_data) = parse_nonce_account(&account.data) {
            if nonce_data.authority == *authority_pubkey {
                tracing::info!(
                    "✅ Found nonce account: {} with current nonce: {}",
                    pubkey,
                    nonce_data.blockhash
                );
                nonce_accounts.push((pubkey, nonce_data.blockhash));
            }
        }
    }

    if nonce_accounts.is_empty() {
        tracing::warn!("No nonce accounts found for authority: {}", authority_pubkey);
    } else {
        tracing::info!(
            "Found {} nonce account(s) for authority: {}",
            nonce_accounts.len(),
            authority_pubkey
        );
    }

    Ok(nonce_accounts)
}

/// Get the first available nonce account for a sender
#[cfg(feature = "rpc-client")]
pub async fn get_or_find_nonce_account(
    client: &RpcClient,
    sender_pubkey: &Pubkey,
    nonce_pubkey_option: Option<&Pubkey>,
) -> Result<(Pubkey, Hash), NonceError> {
    // If nonce pubkey provided, verify and use it
    if let Some(nonce_pubkey) = nonce_pubkey_option {
        if check_nonce_account_exists(client, nonce_pubkey).await? {
            let account = client.get_account(nonce_pubkey)
                .map_err(|e| NonceError::RpcError(e.to_string()))?;
            let nonce_data = parse_nonce_account(&account.data)?;
            
            if nonce_data.authority != *sender_pubkey {
                return Err(NonceError::InvalidAuthority(
                    format!("Sender is not the authority of nonce account {}", nonce_pubkey)
                ));
            }
            
            return Ok((*nonce_pubkey, nonce_data.blockhash));
        }
    }

    // Otherwise, search for existing nonce accounts
    let nonce_accounts = find_nonce_accounts_by_authority(client, sender_pubkey).await?;

    if let Some((pubkey, blockhash)) = nonce_accounts.first() {
        tracing::info!("Using existing nonce account: {}", pubkey);
        Ok((*pubkey, *blockhash))
    } else {
        Err(NonceError::NoNonceAccountFound(
            format!("No nonce account found for sender: {}", sender_pubkey)
        ))
    }
}

// /// Get or create a nonce account
// /// Checks if nonce account exists, if not creates a new one
// pub async fn get_or_create_nonce_account(
//     client: &RpcClient,
//     sender_keypair: &Keypair,
//     existing_nonce_pubkey: Option<&Pubkey>,
// ) -> Result<Keypair, NonceError> {
//     // If nonce pubkey provided, check if it exists
//     if let Some(nonce_pubkey) = existing_nonce_pubkey {
//         if check_nonce_account_exists(client, nonce_pubkey).await? {
//             tracing::info!("Using existing nonce account: {}", nonce_pubkey);
//             // Note: We can't return the keypair for existing account
//             // In production, you'd load it from secure storage
//             return Err(NonceError::InvalidState(
//                 "Cannot retrieve keypair for existing nonce account. Please provide the keypair.".to_string()
//             ));
//         }
//     }
    
//     // Create new nonce account
//     create_nonce_account(client, sender_keypair).await
// }

/// Create a nonce account on Solana
/// The sender funds the nonce account and is set as the authority
/// This allows the sender to advance the nonce when needed
#[cfg(feature = "rpc-client")]
pub async fn create_nonce_account(
    client: &RpcClient,
    sender_keypair: &Keypair,
) -> Result<Keypair, NonceError> {
    let nonce_keypair = Keypair::new();

    tracing::info!("Creating new nonce account...");
    tracing::info!("Sender (and nonce authority): {}", sender_keypair.pubkey());
    tracing::info!("New nonce account: {}", nonce_keypair.pubkey());

    // Calculate rent exemption for nonce account
    let rent_exemption = client
        .get_minimum_balance_for_rent_exemption(solana_sdk::nonce::State::size())
        .map_err(|e| NonceError::RpcError(format!("Failed to get rent exemption: {}", e)))?;
    tracing::info!("Rent exemption required: {} lamports", rent_exemption);

    // Check sender's balance
    let sender_balance = client
        .get_balance(&sender_keypair.pubkey())
        .map_err(|e| NonceError::RpcError(format!("Failed to get sender balance: {}", e)))?;
    
    if sender_balance < rent_exemption {
        return Err(NonceError::CreationFailed(format!(
            "Insufficient balance: have {} lamports, need {} lamports",
            sender_balance, rent_exemption
        )));
    }

    tracing::info!("✅ Sufficient balance confirmed for nonce account creation");

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .map_err(|e| NonceError::RpcError(format!("Failed to get blockhash: {}", e)))?;

    // Create nonce account instructions
    // Sender funds the account and is set as the authority
    let create_nonce_instructions = system_instruction::create_nonce_account(
        &sender_keypair.pubkey(), // funding account (sender)
        &nonce_keypair.pubkey(),  // nonce account
        &sender_keypair.pubkey(), // authority (sender)
        rent_exemption,           // lamports
    );

    tracing::info!(
        "Number of instructions: {}",
        create_nonce_instructions.len()
    );

    // Create and sign transaction
    let mut tx = Transaction::new_with_payer(
        &create_nonce_instructions,
        Some(&sender_keypair.pubkey()),
    );
    tx.sign(&[&nonce_keypair, sender_keypair], recent_blockhash);

    tracing::info!("Sending nonce account creation transaction...");

    // Send transaction
    let signature = client
        .send_and_confirm_transaction(&tx)
        .map_err(|e| NonceError::CreationFailed(format!("Failed to send transaction: {}", e)))?;
    tracing::info!("✅ Nonce account created! Signature: {}", signature);

    Ok(nonce_keypair)
}

/// Helper struct for parsed nonce data
#[derive(Debug, Clone)]
pub struct NonceData {
    pub authority: Pubkey,
    pub blockhash: Hash,
}

/// Error types for nonce operations
#[derive(Error, Debug)]
pub enum NonceError {
    #[error("Nonce account creation failed: {0}")]
    CreationFailed(String),
    
    #[error("Nonce advancement failed: {0}")]
    AdvancementFailed(String),
    
    #[error("Invalid nonce account state: {0}")]
    InvalidState(String),
    
    #[error("RPC error: {0}")]
    RpcError(String),
    
    #[error("Invalid account data: {0}")]
    InvalidAccountData(String),
    
    #[error("Invalid authority: {0}")]
    InvalidAuthority(String),
    
    #[error("No nonce account found: {0}")]
    NoNonceAccountFound(String),
}
