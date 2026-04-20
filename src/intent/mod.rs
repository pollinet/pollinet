//! Pollinet intent protocol helpers
//!
//! Provides stateless utilities for the pollinet-executor Anchor program:
//!  - SPL Token `approve_checked` instruction building (delegates to executor PDA)
//!  - Borsh-compatible 169-byte Intent struct serialization
//!  - Executor PDA derivation

use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    transaction::Transaction,
};
use spl_token::instruction::approve_checked;
use std::str::FromStr;

pub const POLLINET_PROGRAM_ID: &str = "EJ28rMA3AgRVdNqdCnq4DrpRUfYA12aPdJy1bbFNsQ1A";

/// Token-2022 program ID (hardcoded to avoid adding the crate).
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

// ─── PDA ─────────────────────────────────────────────────────────────────────

/// Derives the executor PDA `["executor"]` under the pollinet-executor program.
pub fn executor_pda() -> (Pubkey, u8) {
    let program_id = Pubkey::from_str(POLLINET_PROGRAM_ID)
        .expect("POLLINET_PROGRAM_ID is a valid base58 pubkey");
    Pubkey::find_program_address(&[b"executor"], &program_id)
}

// ─── Intent serialization ─────────────────────────────────────────────────────

/// Serializes intent fields into the canonical 169-byte borsh layout:
/// version(1) | from(32) | to(32) | token_mint(32) | amount(8) |
/// nonce(16)  | expires_at(8) | gas_fee_amount(8) | gas_fee_payee(32)
pub fn serialize_intent(
    version: u8,
    from: &[u8; 32],
    to: &[u8; 32],
    token_mint: &[u8; 32],
    amount: u64,
    nonce: &[u8; 16],
    expires_at: i64,
    gas_fee_amount: u64,
    gas_fee_payee: &[u8; 32],
) -> [u8; 169] {
    let mut b = [0u8; 169];
    let mut o = 0usize;
    b[o] = version;
    o += 1;
    b[o..o + 32].copy_from_slice(from);
    o += 32;
    b[o..o + 32].copy_from_slice(to);
    o += 32;
    b[o..o + 32].copy_from_slice(token_mint);
    o += 32;
    b[o..o + 8].copy_from_slice(&amount.to_le_bytes());
    o += 8;
    b[o..o + 16].copy_from_slice(nonce);
    o += 16;
    b[o..o + 8].copy_from_slice(&expires_at.to_le_bytes());
    o += 8;
    b[o..o + 8].copy_from_slice(&gas_fee_amount.to_le_bytes());
    o += 8;
    b[o..o + 32].copy_from_slice(gas_fee_payee);
    b
}

/// Generates 16 random bytes for use as an intent nonce.
pub fn random_nonce() -> [u8; 16] {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

// ─── Revoke ───────────────────────────────────────────────────────────────────

/// Builds a single unsigned `Transaction` with one `revoke` instruction per token account.
/// After submission the executor PDA will no longer have delegate authority over those accounts.
pub fn build_revoke_transaction(
    owner: &Pubkey,
    fee_payer: &Pubkey,
    recent_blockhash: Hash,
    token_accounts: &[String],
    token_program: &str,
) -> Result<String, String> {
    use spl_token::instruction::revoke;

    let token_program_id = if token_program == "token-2022" {
        Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap()
    } else {
        spl_token::id()
    };

    let mut ixs: Vec<Instruction> = Vec::with_capacity(token_accounts.len());
    for acct in token_accounts {
        let token_account = Pubkey::from_str(acct)
            .map_err(|e| format!("Invalid token_account '{}': {}", acct, e))?;
        let ix = revoke(&token_program_id, &token_account, owner, &[])
            .map_err(|e| format!("revoke for account {}: {}", acct, e))?;
        ixs.push(ix);
    }

    let message = Message::new_with_blockhash(&ixs, Some(fee_payer), &recent_blockhash);
    let tx = Transaction::new_unsigned(message);
    let raw = bincode1::serialize(&tx)
        .map_err(|e| format!("Transaction serialization failed: {}", e))?;
    Ok(STANDARD.encode(raw))
}

// ─── Approve instruction building ────────────────────────────────────────────

/// One entry in a batch-approve request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenApprovalInput {
    /// Token mint address (base58).
    pub mint_address: String,
    /// Delegated amount in the token's smallest unit.
    pub amount: u64,
    /// Token decimal places (required by `approve_checked`).
    pub decimals: u8,
    /// Owner's associated (or any) token account for this mint (base58).
    pub token_account: String,
    /// "spl-token" (default) or "token-2022".
    #[serde(default = "default_token_program")]
    pub token_program: String,
}

fn default_token_program() -> String {
    "spl-token".to_string()
}

/// Builds a single unsigned `Transaction` whose instructions are one
/// `approve_checked` per entry in `approvals`.
/// Returns the transaction serialized with bincode and base64-encoded.
pub fn build_approve_transaction(
    owner: &Pubkey,
    fee_payer: &Pubkey,
    recent_blockhash: Hash,
    approvals: &[TokenApprovalInput],
) -> Result<String, String> {
    let (executor, _) = executor_pda();

    let mut ixs: Vec<Instruction> = Vec::with_capacity(approvals.len());

    for item in approvals {
        let mint = Pubkey::from_str(&item.mint_address)
            .map_err(|e| format!("Invalid mint_address '{}': {}", item.mint_address, e))?;
        let token_account = Pubkey::from_str(&item.token_account)
            .map_err(|e| format!("Invalid token_account '{}': {}", item.token_account, e))?;
        let token_program_id = if item.token_program == "token-2022" {
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap()
        } else {
            spl_token::id()
        };

        let ix = approve_checked(
            &token_program_id,
            &token_account,
            &mint,
            &executor,
            owner,
            &[],
            item.amount,
            item.decimals,
        )
        .map_err(|e| format!("approve_checked for mint {}: {}", item.mint_address, e))?;

        ixs.push(ix);
    }

    let message = Message::new_with_blockhash(&ixs, Some(fee_payer), &recent_blockhash);
    let tx = Transaction::new_unsigned(message);

    let raw = bincode1::serialize(&tx)
        .map_err(|e| format!("Transaction serialization failed: {}", e))?;

    Ok(STANDARD.encode(raw))
}
