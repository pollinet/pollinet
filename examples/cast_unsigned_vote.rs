//! Example: Create Unsigned Governance Vote Transaction
//!
//! This example demonstrates creating an unsigned governance vote transaction
//! that can be signed by multiple parties (voter and fee payer separately).
//!
//! Flow:
//! 1. Load voter keypair (or use pubkey if signing externally)
//! 2. Create unsigned vote transaction (base64 encoded)
//! 3. Add voter signature
//! 4. Add fee payer signature (if different from voter)
//! 5. Submit to Solana
//!
//! Use Case: Multi-party governance voting where voter and fee payer are different

use bs58;
use chrono;
use pollinet::PolliNetSDK;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("=== PolliNet Unsigned Governance Vote Example ===\n");

    // 1. Initialize the SDK and RPC client
    let rpc_url = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    let rpc_client =
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::finalized());
    info!("✅ SDK initialized with RPC client: {}", rpc_url);

    // 2. Load voter keypair from private key
    info!("\n=== Loading Voter Keypair ===");
    let voter_private_key =
        "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu";

    let private_key_bytes = bs58::decode(voter_private_key)
        .into_vec()
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let voter_keypair = Keypair::try_from(&private_key_bytes[..])
        .map_err(|e| format!("Failed to create keypair from private key: {}", e))?;

    info!("✅ Voter loaded: {}", voter_keypair.pubkey());
    info!("   Voter will be the nonce authority");

    // 3. Check voter balance
    info!("\n=== Checking Voter Balance ===");
    let voter_balance = rpc_client.get_balance(&voter_keypair.pubkey())?;
    info!(
        "Voter balance: {} lamports ({} SOL)",
        voter_balance,
        voter_balance as f64 / LAMPORTS_PER_SOL as f64
    );

    if voter_balance == 0 {
        return Err("Voter has no balance. Please fund the wallet first.".into());
    }

    // 4. Set up nonce account
    info!("\n=== Setting Up Nonce Account ===");
    let nonce_account = "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK";
    info!("Using nonce account: {}", nonce_account);
    info!("   Nonce authority: {} (voter)", voter_keypair.pubkey());

    // 5. Set governance vote parameters
    info!("\n=== Governance Vote Parameters ===");
    let voter_pubkey = voter_keypair.pubkey().to_string();
    let proposal_id = "GgathUhdrCWRHowoRKACjgWhYHfxCEdBi5ViqYN6HVxk".to_string();
    let vote_account = voter_keypair.pubkey().to_string();
    let vote_choice = 1; // 0 = No, 1 = Yes, etc.
    let fee_payer = voter_keypair.pubkey().to_string(); // In this example, voter pays the fee

    info!("Voter: {}", voter_pubkey);
    info!("Proposal ID: {}", proposal_id);
    info!("Vote account: {}", vote_account);
    info!("Vote choice: {}", vote_choice);
    info!("Fee payer: {}", fee_payer);

    // 6. Create unsigned vote transaction
    info!("\n=== Creating Unsigned Vote Transaction ===");
    info!("Creating unsigned transaction (base64 encoded)...");

    let unsigned_tx_base64 = sdk
        .cast_unsigned_vote(
            &voter_pubkey,
            &proposal_id,
            &vote_account,
            vote_choice,
            &fee_payer,
            nonce_account,
        )
        .await?;

    info!("✅ Unsigned vote transaction created");
    info!("   Base64 length: {} characters", unsigned_tx_base64.len());
    info!("   Ready for signing by voter and fee payer");
    info!("   Transaction has NO signatures yet");

    // 7. Display unsigned transaction
    info!("\n=== Unsigned Transaction (Base64) ===");
    info!("First 100 characters: {}...", &unsigned_tx_base64[..100.min(unsigned_tx_base64.len())]);
    info!("This can be sent to hardware wallets or other signing devices");

    // 8. Add voter signature
    info!("\n=== Adding Voter Signature ===");
    info!("Voter is signing the vote transaction...");

    // Decode to get message for signing
    let unsigned_bytes = base64::decode(&unsigned_tx_base64)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;
    let tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&unsigned_bytes)
        .map_err(|e| format!("Failed to deserialize transaction: {}", e))?;

    // Sign with voter keypair
    let message = tx.message.clone();
    let voter_signature = voter_keypair.sign_message(message.serialize().as_slice());
    info!("✅ Voter signature created: {}", voter_signature);

    // Add signature using SDK
    let partially_signed_tx = sdk.add_signature(
        &unsigned_tx_base64,
        &voter_keypair.pubkey(),
        &voter_signature,
    )?;

    info!("✅ Voter signature added to transaction");
    info!("   Transaction now has voter's signature");

    // 9. If fee payer is different, add their signature
    // In this example, voter = fee payer, so we can skip this step
    // But here's how you would do it if they were different:
    //
    // if fee_payer != voter_pubkey {
    //     info!("\n=== Adding Fee Payer Signature ===");
    //     let fee_payer_signature = fee_payer_keypair.sign_message(...);
    //     let fully_signed_tx = sdk.add_signature(
    //         &partially_signed_tx,
    //         &fee_payer_keypair.pubkey(),
    //         &fee_payer_signature,
    //     )?;
    // }

    let fully_signed_tx = partially_signed_tx;
    info!("\n✅ Transaction fully signed");
    info!("   Voter signed (as nonce authority and voter)");
    info!("   Fee payer signed (in this case, same as voter)");

    // 10. Optional: Wait to demonstrate nonce durability
    info!("\n=== Waiting Period ===");
    info!("Waiting for 2 minutes to demonstrate vote transaction durability...");

    let total_minutes = 2;
    for remaining_minutes in (1..=total_minutes).rev() {
        let current_time = chrono::Local::now();
        info!(
            "⏳ {} minute(s) remaining until submission | Current time: {}",
            remaining_minutes,
            current_time.format("%Y-%m-%d %H:%M:%S")
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }

    let final_time = chrono::Local::now();
    info!("✅ Wait complete | Time: {}", final_time.format("%Y-%m-%d %H:%M:%S"));
    info!("Vote transaction is still valid thanks to durable nonce!");

    // 11. Submit to Solana blockchain
    info!("\n=== Submitting Vote to Solana ===");
    info!("Decoding base64 and submitting vote transaction to blockchain...");

    let signature = sdk.send_and_confirm_transaction(&fully_signed_tx).await?;
    info!("✅ Governance vote submitted successfully!");
    info!("   Transaction signature: {}", signature);
    info!(
        "   View on Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
        signature
    );

    // 12. Broadcast confirmation
    info!("\n=== Broadcasting Vote Confirmation ===");
    sdk.broadcast_confirmation(&signature).await?;
    info!("✅ Vote confirmation broadcasted to network");

    // 13. Summary
    info!("\n=== Complete Unsigned Vote Flow Summary ===");
    info!("✅ 1. Loaded voter from private key");
    info!("✅ 2. Verified voter balance");
    info!("✅ 3. Set up nonce account");
    info!("✅ 4. Created UNSIGNED vote transaction (base64)");
    info!("✅ 5. Added voter signature");
    info!("✅ 6. Added fee payer signature (voter = fee payer in this example)");
    info!("✅ 7. Waited 2 minutes (vote still valid!)");
    info!("✅ 8. Submitted to Solana");
    info!("✅ 9. Broadcasted confirmation: {}", signature);

    info!("\n=== Key Features ===");
    info!("• Transaction type: Unsigned Governance Vote");
    info!("• Base64 encoding: Easy to transmit and store");
    info!("• Multi-party signing: Voter and fee payer can sign separately");
    info!("• Nonce account: Vote valid until nonce advances");
    info!("• Instructions: [1] Advance nonce, [2] Cast vote");
    info!("• Vote choice: {} (0=No, 1=Yes, etc.)", vote_choice);
    info!("• Durable: Waited 2 minutes, still valid!");

    info!("\n=== Use Cases ===");
    info!("• Hardware Wallet: Send unsigned tx to hardware wallet for signing");
    info!("• Multi-sig Voting: Multiple parties sign the vote");
    info!("• Offline Signing: Create unsigned tx online, sign offline");
    info!("• Separate Fee Payer: Governance program pays fees, not voter");
    info!("• Air-gapped Signing: Maximum security for high-value votes");

    info!("\n=== Next Steps ===");
    info!("• Integrate with real governance programs (SPL Governance, Realms)");
    info!("• Add support for multi-sig voting");
    info!("• Implement vote delegation");
    info!("• Support for encrypted vote transmission over mesh");

    Ok(())
}

