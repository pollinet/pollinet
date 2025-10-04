//! Nonce account management for PolliNet SDK
//!
//! Handles Solana nonce accounts to extend transaction lifespan beyond recent blockhash constraints

use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Nonce account manager for PolliNet
pub struct NonceManager {
    /// Current nonce account public key
    nonce_account: Pubkey,
    /// Current nonce value
    current_nonce: Arc<RwLock<u64>>,
    /// Authority keypair for nonce operations
    authority: Keypair,
}

impl NonceManager {
    /// Create a new nonce manager
    pub async fn new() -> Result<Self, NonceError> {
        // Generate a new keypair for the nonce account
        let authority = Keypair::new();

        // For now, use a mock nonce account
        // In production, this would create or load an existing nonce account
        let nonce_account = Pubkey::new_unique();

        Ok(Self {
            nonce_account,
            current_nonce: Arc::new(RwLock::new(0)),
            authority,
        })
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
        _recent_blockhash: &Hash,
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
        let instruction = system_instruction::advance_nonce_account(nonce_account, authority);

        Ok(instruction)
    }

    /// Get the nonce account public key
    pub fn get_nonce_account(&self) -> Pubkey {
        self.nonce_account
    }

    /// Get the authority public key
    pub fn get_authority(&self) -> Pubkey {
        self.authority.pubkey()
    }
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
}
