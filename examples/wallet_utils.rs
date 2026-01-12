//! Wallet utility functions for examples
//!
//! Provides helper functions for creating new wallets and requesting airdrops
//! on Solana devnet for testing purposes.
//!
//! Supports loading configuration from .env file (RPC URL, wallet private key).

use bs58;
use dotenv::dotenv;
use solana_client::rpc_client::RpcClient;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::env;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

/// Get RPC URL from environment variable or use default
///
/// Checks for SOLANA_URL environment variable from .env file or environment.
/// Falls back to devnet if not set.
///
/// # Returns
/// RPC URL string
pub fn get_rpc_url() -> String {
    // Load .env file (silently fails if not found)
    dotenv().ok();

    // Try to get RPC URL from environment
    env::var("SOLANA_URL").unwrap_or_else(|_| "https://api.devnet.solana.com".to_string())
}

/// Restore a wallet from a private key string (base58 encoded)
///
/// # Arguments
/// * `private_key_str` - Base58 encoded private key string
///
/// # Returns
/// The restored keypair
fn restore_wallet_from_private_key(
    private_key_str: &str,
) -> Result<Keypair, Box<dyn std::error::Error>> {
    // Decode base58 private key
    let private_key_bytes = bs58::decode(private_key_str)
        .into_vec()
        .map_err(|e| format!("Failed to decode private key (base58): {}", e))?;

    // Create keypair from bytes
    let keypair = Keypair::try_from(&private_key_bytes[..])
        .map_err(|e| format!("Failed to create keypair from private key: {}", e))?;

    Ok(keypair)
}

/// Load wallet from .env file if WALLET_PRIVATE_KEY is set
///
/// # Returns
/// Some(keypair) if private key is found and valid, None otherwise
fn load_wallet_from_env() -> Option<Keypair> {
    // Load .env file (silently fails if not found)
    dotenv().ok();

    // Try to get private key from environment
    let private_key = env::var("WALLET_PRIVATE_KEY").ok()?;

    if private_key.is_empty() {
        return None;
    }

    match restore_wallet_from_private_key(&private_key) {
        Ok(keypair) => {
            info!("✅ Wallet restored from .env file");
            Some(keypair)
        }
        Err(e) => {
            warn!("⚠️  Failed to restore wallet from .env: {}", e);
            warn!("   Falling back to creating new wallet");
            None
        }
    }
}

/// Create or restore a wallet and ensure it has sufficient balance
///
/// This function:
/// 1. Tries to restore wallet from .env file (WALLET_PRIVATE_KEY)
/// 2. If restored, checks balance
/// 3. If balance < airdrop_amount_sol, requests airdrop
/// 4. If no .env wallet, creates new wallet and requests airdrop
///
/// # Arguments
/// * `rpc_client` - The RPC client connected to Solana devnet
/// * `airdrop_amount_sol` - Amount of SOL to request if airdrop is needed
///
/// # Returns
/// The keypair (restored or newly created) with sufficient balance
pub async fn create_and_fund_wallet(
    rpc_client: &RpcClient,
    airdrop_amount_sol: f64,
) -> Result<Keypair, Box<dyn std::error::Error>> {
    // Try to restore wallet from .env file
    let keypair = if let Some(restored_keypair) = load_wallet_from_env() {
        let pubkey = restored_keypair.pubkey();
        info!("=== Wallet Restored from .env ===");
        info!("✅ Wallet: {}", pubkey);

        // Check current balance
        let balance = rpc_client.get_balance(&pubkey)?;
        let balance_sol = balance as f64 / LAMPORTS_PER_SOL as f64;
        info!(
            "   Current balance: {} lamports ({} SOL)",
            balance, balance_sol
        );

        // Request airdrop if balance is below threshold
        let airdrop_amount_lamports = (airdrop_amount_sol * LAMPORTS_PER_SOL as f64) as u64;
        if balance < airdrop_amount_lamports {
            info!(
                "   Balance below {} SOL threshold, requesting airdrop...",
                airdrop_amount_sol
            );
            request_and_confirm_airdrop(rpc_client, &pubkey, airdrop_amount_sol).await?;
        } else {
            info!(
                "   ✅ Balance sufficient (above {} SOL threshold)",
                airdrop_amount_sol
            );
        }

        restored_keypair
    } else {
        // No .env wallet found, create new wallet
        info!("=== Creating New Wallet ===");
        let new_keypair = Keypair::new();
        let pubkey = new_keypair.pubkey();

        info!("✅ New wallet created: {}", pubkey);
        info!("   Requesting airdrop of {} SOL...", airdrop_amount_sol);

        request_and_confirm_airdrop(rpc_client, &pubkey, airdrop_amount_sol).await?;

        new_keypair
    };

    // Verify final balance
    let pubkey = keypair.pubkey();
    let balance = rpc_client.get_balance(&pubkey)?;
    let balance_sol = balance as f64 / LAMPORTS_PER_SOL as f64;
    info!(
        "   Final balance: {} lamports ({} SOL)",
        balance, balance_sol
    );

    Ok(keypair)
}

/// Request airdrop and wait for confirmation
async fn request_and_confirm_airdrop(
    rpc_client: &RpcClient,
    pubkey: &solana_sdk::pubkey::Pubkey,
    airdrop_amount_sol: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Request airdrop with retry logic for rate limiting
    let airdrop_amount_lamports = (airdrop_amount_sol * LAMPORTS_PER_SOL as f64) as u64;
    let signature = request_airdrop_with_retry(rpc_client, pubkey, airdrop_amount_lamports).await?;

    info!("   Airdrop transaction signature: {}", signature);
    info!("   Waiting for confirmation...");

    // Wait for confirmation with retry
    confirm_transaction_with_retry(rpc_client, &signature).await?;
    sleep(Duration::from_secs(15)).await;

    // Verify balance
    let balance = rpc_client.get_balance(pubkey)?;
    info!("✅ Airdrop confirmed!");
    info!(
        "   Wallet balance: {} lamports ({} SOL)",
        balance,
        balance as f64 / LAMPORTS_PER_SOL as f64
    );

    if balance < airdrop_amount_lamports {
        return Err(format!(
            "Airdrop incomplete. Expected {} SOL, got {} SOL",
            airdrop_amount_sol,
            balance as f64 / LAMPORTS_PER_SOL as f64
        )
        .into());
    }

    Ok(())
}

/// Request airdrop with retry logic to handle rate limiting
/// For local validators, this should work without issues
async fn request_airdrop_with_retry(
    rpc_client: &RpcClient,
    pubkey: &solana_sdk::pubkey::Pubkey,
    lamports: u64,
) -> Result<solana_sdk::signature::Signature, Box<dyn std::error::Error>> {
    const MAX_RETRIES: u32 = 5;
    const INITIAL_DELAY_SECS: u64 = 1; // Shorter delay for local validators

    for attempt in 0..MAX_RETRIES {
        match rpc_client.request_airdrop(pubkey, lamports) {
            Ok(sig) => return Ok(sig),
            Err(e) => {
                let error_str = e.to_string();

                // Log detailed error for debugging
                if attempt == 0 {
                    warn!("⚠️  Airdrop request failed: {}", error_str);
                    warn!("   Pubkey: {}", pubkey);
                    warn!(
                        "   Amount: {} lamports ({} SOL)",
                        lamports,
                        lamports as f64 / LAMPORTS_PER_SOL as f64
                    );
                }

                // Check for retriable errors
                let is_rate_limit = error_str.contains("rate limit")
                    || error_str.contains("rate_limit")
                    || error_str.contains("too many");

                let is_retriable = is_rate_limit
                    || error_str.contains("timeout")
                    || error_str.contains("connection");

                if is_retriable && attempt < MAX_RETRIES - 1 {
                    let delay_secs = INITIAL_DELAY_SECS * (attempt + 1) as u64;
                    warn!(
                        "   Retrying airdrop (attempt {}/{}) in {} seconds...",
                        attempt + 1,
                        MAX_RETRIES,
                        delay_secs
                    );
                    sleep(Duration::from_secs(delay_secs)).await;
                    continue;
                }

                // Non-retriable error or max retries reached
                if attempt == MAX_RETRIES - 1 {
                    warn!("   ❌ Failed after {} attempts", MAX_RETRIES);
                    warn!("   For local validators, make sure:");
                    warn!("     1. Validator is running (check with: solana cluster-version)");
                    warn!("     2. Using correct port (default: 8899)");
                    warn!("     3. Validator has sufficient funds");
                }

                return Err(Box::new(e));
            }
        }
    }

    Err("Failed to request airdrop after maximum retries".into())
}

/// Confirm transaction with retry logic
async fn confirm_transaction_with_retry(
    rpc_client: &RpcClient,
    signature: &solana_sdk::signature::Signature,
) -> Result<(), Box<dyn std::error::Error>> {
    const MAX_RETRIES: u32 = 10;
    const RETRY_DELAY_SECS: u64 = 2;

    for attempt in 0..MAX_RETRIES {
        match rpc_client.confirm_transaction(signature) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if attempt < MAX_RETRIES - 1 {
                    warn!(
                        "⚠️  Transaction confirmation failed (attempt {}/{}). Retrying in {} seconds...",
                        attempt + 1,
                        MAX_RETRIES,
                        RETRY_DELAY_SECS
                    );
                    sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;
                    continue;
                }
                return Err(Box::new(e));
            }
        }
    }

    Err("Failed to confirm transaction after maximum retries".into())
}

/// Main function for cargo example compilation
/// This is a helper module, not a standalone example.
/// It's used by other examples to create/fund wallets.
#[allow(dead_code)]
fn main() {
    println!("This is a helper module for other examples.");
    println!("Use create_and_fund_wallet() or get_rpc_url() in your examples.");
}
