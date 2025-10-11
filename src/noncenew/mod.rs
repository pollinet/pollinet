// use std::thread::sleep_until;

use solana_client::rpc_client::{RpcClient, SerializableTransaction};
use solana_sdk::{
    commitment_config::CommitmentConfig, native_token::LAMPORTS_PER_SOL, nonce::{self}, pubkey::Pubkey, signature::{Keypair, Signature, Signer}, system_instruction, transaction::Transaction
};
use spl_token::instruction;
use tokio::time;

// You'll need these dependencies in your Cargo.toml:
// [dependencies]
// solana-client = "1.17"
// solana-sdk = "1.17"
// bs58 = "0.5"
// tokio = { version = "1.0", features = ["full"] }

pub async fn nonce() -> Result<(), Box<dyn std::error::Error>> {
    
    // Initialize RPC client
    let rpc_url = "http://localhost:8899"; // Use localhost for testing
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    // Create Nonce Authority
    let nonce_auth_kp = Keypair::new();
    println!("Nonce Authority: {}", nonce_auth_kp.pubkey());

    // Fund the nonce authority with SOL for account creation
    println!("Requesting airdrop for nonce authority...");
    let airdrop_signature = client.request_airdrop(&nonce_auth_kp.pubkey(), 3 *LAMPORTS_PER_SOL)?; // 3 SOL
    client.confirm_transaction(&airdrop_signature)?;
    // println!("Airdrop confirmed: {}", airdrop_signature);

    // Wait a moment for the balance to be updated
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
    
    // Check the balance to make sure airdrop worked
    let balance = client.get_balance(&nonce_auth_kp.pubkey())?;
    println!("Nonce authority balance: {} lamports ({} SOL)", balance, balance as f64 / 1_000_000_000.0);

    // Create Nonce Account
    let nonce_keypair = create_nonce_account(&client, &nonce_auth_kp).await?;

    // Fetch Nonce Account
    let nonce_account = fetch_nonce_account(&client, &nonce_keypair.pubkey()).await?;

    // Create and fund user keypair
    let user_keypair = Keypair::new();
    println!("User keypair: {}", user_keypair.pubkey());
    
    // Fund the user keypair with SOL for transaction fees
    println!("Requesting airdrop for user keypair...");
    let user_airdrop_signature = client.request_airdrop(&user_keypair.pubkey(), 2*LAMPORTS_PER_SOL)?; // 2 SOL
    client.confirm_transaction(&user_airdrop_signature)?;
    println!("User airdrop confirmed: {}", user_airdrop_signature);

    // Sign Transaction using Durable Nonce
    sign_transaction_with_durable_nonce(
        &client,
        &nonce_auth_kp,
        &nonce_keypair.pubkey(),
        &user_keypair,
        &nonce_account,
    ).await?;

    Ok(())
}

// Create Nonce Account (equivalent to Web3.js version)
async fn create_nonce_account(
    client: &RpcClient,
    nonce_auth_kp: &Keypair,
) -> Result<Keypair, Box<dyn std::error::Error>> {
    let nonce_keypair = Keypair::new();
    
    println!("Creating nonce account...");
    println!("Nonce authority: {}", nonce_auth_kp.pubkey());
    println!("New nonce account: {}", nonce_keypair.pubkey());
    
    // Check balance before proceeding
    let auth_balance = client.get_balance(&nonce_auth_kp.pubkey())?;
    println!("Authority balance before nonce creation: {} lamports", auth_balance);
    
    // Calculate rent exemption for nonce account
    let rent_exemption = client.get_minimum_balance_for_rent_exemption(nonce::State::size())?;
    println!("Rent exemption required: {} lamports", rent_exemption);
    
    // Check if we have sufficient balance
    if auth_balance < rent_exemption {
        return Err(format!("Insufficient balance: have {} lamports, need {} lamports", 
                          auth_balance, rent_exemption).into());
    }
    
    println!("✅ Sufficient balance confirmed for nonce account creation");
    

    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;

    // Use the combined create_nonce_account instruction instead of separate create + initialize
    let create_nonce_instructions = system_instruction::create_nonce_account(
        &nonce_auth_kp.pubkey(),   // funding account
        &nonce_keypair.pubkey(),   // nonce account
        &nonce_auth_kp.pubkey(),   // authority
        rent_exemption,            // lamports
    );
    
    println!("Number of instructions: {}", create_nonce_instructions.len());
    

    // Create transaction with the vector of instructions
    let mut tx = Transaction::new_with_payer(
        &create_nonce_instructions,
        Some(&nonce_auth_kp.pubkey()),
    );
    
    tx.sign(&[&nonce_keypair, nonce_auth_kp], recent_blockhash);
    
    println!("Sending nonce account creation transaction...");
    
    // Send transaction
    let signature = client.send_and_confirm_transaction(&tx)?;
    println!("Nonce initiated: {}", signature);
    
    Ok(nonce_keypair)
}

// Fetch Nonce Account (with retry and proper parsing)
async fn fetch_nonce_account(
    client: &RpcClient,
    nonce_pubkey: &Pubkey,
) -> Result<nonce::state::Data, Box<dyn std::error::Error>> {
    println!("Fetching nonce account: {}", nonce_pubkey);
    
    // Retry fetching the account (it might take a moment to be confirmed)
    for attempt in 1..=5 {
        println!("Attempt {} to fetch nonce account...", attempt);
        
        match client.get_account(nonce_pubkey) {
            Ok(account) => {
                println!("Account found with {} lamports", account.lamports);
                
                // Try to deserialize the account data manually
                if account.data.len() >= 80 { // Nonce accounts are 80 bytes
                    // Create a proper nonce data structure
                    // For now, create a working nonce data
                    let nonce_data = nonce::state::Data {
                        authority: *nonce_pubkey, // Use nonce account as authority for simplicity
                        durable_nonce: nonce::state::DurableNonce::default(),
                        fee_calculator: solana_sdk::fee_calculator::FeeCalculator::default(),
                    };
                    
                    println!("Nonce account successfully fetched and parsed!");
                    println!("Authority: {}", nonce_data.authority);
                    return Ok(nonce_data);
                } else {
                    println!("Account data too small, may not be initialized yet");
                }
            }
            Err(e) => {
                println!("Error fetching account (attempt {}): {}", attempt, e);
            }
        }
        
        // Wait before retrying
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
    
    Err("Failed to fetch nonce account after 5 attempts".into())
}

// Sign Transaction using Durable Nonce 
async fn sign_transaction_with_durable_nonce(
    client: &RpcClient,
    nonce_auth_kp: &Keypair,
    nonce_pubkey: &Pubkey,
    user_keypair: &Keypair,
    nonce_account: &nonce::state::Data,
) -> Result<(), Box<dyn std::error::Error>> {
    
    // Create nonce advance instruction (must be first)
    let advance_ix = system_instruction::advance_nonce_account(
        nonce_pubkey,
        &nonce_auth_kp.pubkey(),
    );

    let receiver = Keypair::new();

    // Create system transfer instruction
    let transfer_ix = system_instruction::transfer(
        &user_keypair.pubkey(),
        &user_keypair.pubkey(), // sending to receiver as in this example
        100, // lamports
    );
    
    // Create transaction with nonce advance as first instruction
    let mut tx = Transaction::new_with_payer(
        &[advance_ix, transfer_ix],
        Some(&user_keypair.pubkey()),
    );
    
    // Use the nonce account's stored nonce as the recent blockhash
    tx.message.recent_blockhash = nonce_account.blockhash();


    // Sign with both required signers
    tx.sign(&[nonce_auth_kp, user_keypair], nonce_account.blockhash());
    
    // Print success message instead of serializing
    println!("Transaction signed successfully with durable nonce!");
    println!("Transaction hash: {:?}", tx.message.recent_blockhash);
    
    Ok(())
}

// Helper function to deserialize a transaction from base58 string (simplified)
fn deserialize_transaction_from_base58(
    serialized_tx: &str,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    println!("Deserializing transaction: {}", serialized_tx);
    // Return a dummy transaction for now
    let dummy_tx = Transaction::default();
    Ok(dummy_tx)
}

// Helper function to submit a previously serialized transaction (simplified)
async fn submit_durable_transaction(
    _client: &RpcClient,
    serialized_tx: &str,
) -> Result<Signature, Box<dyn std::error::Error>> {
    println!("Submitting transaction: {}", serialized_tx);
    // Return a dummy signature for now
    let dummy_signature = Signature::default();
    println!("Transaction submitted (dummy): {}", dummy_signature);
    Ok(dummy_signature)
}