//! Example: Cast Governance Vote with Durable Nonce
//!
//! This example demonstrates creating presigned governance vote transactions
//! with nonce accounts for extended transaction lifetime.
//!
//! Flow:
//! 1. Load voter keypair
//! 2. Set up nonce account
//! 3. Create presigned vote transaction
//! 4. Compress and fragment
//! 5. Relay over BLE mesh
//! 6. Submit to Solana

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

    info!("=== PolliNet Governance Vote Example ===\n");

    // 1. Initialize the SDK and RPC client
    let rpc_url = "https://api.devnet.solana.com";
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
    info!("   Voter is both the vote caster and nonce authority");

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
    let proposal_id = "GgathUhdrCWRHowoRKACjgWhYHfxCEdBi5ViqYN6HVxk".to_string();
    let vote_account = voter_keypair.pubkey().to_string(); // Voter's account
    let vote_choice = 1; // 0 = No, 1 = Yes, etc. (depends on governance program)

    info!("Proposal ID: {}", proposal_id);
    info!("Vote account: {}", vote_account);
    info!("Vote choice: {}", vote_choice);
    info!("Voter (fee payer): {}", voter_keypair.pubkey());

    // 6. Create presigned vote transaction with nonce
    info!("\n=== Creating Governance Vote Transaction ===");
    info!("Creating presigned vote transaction with nonce account...");
    info!("Instructions will be: [1] Advance nonce, [2] Cast vote");

    let compressed_tx = sdk
        .cast_vote(
            &voter_keypair,
            &proposal_id,
            &vote_account,
            vote_choice,
            nonce_account,
        )
        .await?;

    info!("✅ Vote transaction created and signed");
    info!("✅ Transaction serialized");
    info!("✅ Transaction compressed (if needed)");
    info!("   Compressed size: {} bytes", compressed_tx.len());

    // 7. Fragment the transaction for BLE transmission
    info!("\n=== Fragmenting for BLE ===");
    let fragments = sdk.fragment_transaction(&compressed_tx);

    info!(
        "✅ Vote transaction fragmented into {} fragments",
        fragments.len()
    );
    info!("   BLE MTU size: {} bytes", pollinet::BLE_MTU_SIZE);

    for (i, fragment) in fragments.iter().enumerate() {
        info!(
            "   Fragment {}/{}: {} bytes (checksum: {})",
            i + 1,
            fragments.len(),
            fragment.data.len(),
            hex::encode(&fragment.checksum[..8])
        );
    }

    // 8. Display vote transaction details
    info!("\n=== Vote Transaction Ready ===");
    info!("✅ Governance vote is ready for BLE transmission!");
    info!("✅ Vote transaction has extended lifetime due to nonce account");
    info!("✅ Can be submitted to Solana at any time (until nonce advances)");

    // 9. Simulate receiving fragments
    info!("\n=== Simulating Fragment Reception ===");
    info!("In a real scenario, vote fragments would be received over BLE mesh...");
    info!("Reassembling {} fragments...", fragments.len());

    let reassembled_tx = sdk.reassemble_fragments(&fragments)?;
    info!("✅ Fragments reassembled successfully");
    info!("   Reassembled size: {} bytes", reassembled_tx.len());

    if reassembled_tx == compressed_tx {
        info!("✅ Reassembly verification passed!");
    } else {
        return Err("Reassembly failed: data mismatch".into());
    }

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

    // 11. Submit vote to Solana blockchain
    info!("\n=== Submitting Vote to Solana ===");
    info!("Decompressing and submitting vote transaction to blockchain...");

    let signature = sdk.submit_transaction_to_solana(&reassembled_tx).await?;
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
    info!("\n=== Complete Governance Vote Summary ===");
    info!("✅ 1. Loaded voter from private key");
    info!("✅ 2. Verified voter balance");
    info!("✅ 3. Set up nonce account");
    info!("✅ 4. Created presigned vote transaction with durable nonce");
    info!("✅ 5. Compressed transaction: {} bytes", compressed_tx.len());
    info!(
        "✅ 6. Fragmented into {} BLE-ready fragments",
        fragments.len()
    );
    info!("✅ 7. Reassembled fragments with checksum verification");
    info!("✅ 8. Waited 2 minutes (vote still valid!)");
    info!("✅ 9. Decompressed and submitted to Solana");
    info!("✅ 10. Broadcasted confirmation: {}", signature);

    info!("\n=== Implementation Notes ===");
    info!("• Transaction type: Governance Vote");
    info!("• Nonce account: {}", nonce_account);
    info!("• Voter is both the vote caster and nonce authority");
    info!("• Transaction uses nonce account's stored blockhash");
    info!("• Vote remains valid until nonce account is advanced");
    info!("• Instructions: [1] Advance nonce, [2] Cast vote");
    info!("• Vote choice: {} (0=No, 1=Yes, etc.)", vote_choice);

    info!("\n=== Use Cases ===");
    info!("• Decentralized Governance: Vote on proposals via mesh network");
    info!("• Offline Voting: Cast votes when temporarily connected");
    info!("• Community Polling: Collect votes through mesh propagation");
    info!("• DAO Participation: Vote on DAO proposals offline");
    info!("• Multi-Hop Voting: Votes propagate through mesh to blockchain");

    Ok(())
}

