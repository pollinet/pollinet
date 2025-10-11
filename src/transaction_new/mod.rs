use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, native_token::LAMPORTS_PER_SOL, signature::Signer,
    signer::keypair::Keypair, system_instruction, transaction::Transaction,
};

pub async fn transact() -> Result<()> {
    // Create a connection to cluster
    let connection = RpcClient::new_with_commitment(
        "http://localhost:8899".to_string(),
        CommitmentConfig::confirmed(),
    );

    // Generate sender and recipient keypairs
    let sender = Keypair::new();
    let recipient = Keypair::new();

    // Fund sender with airdrop
    let airdrop_signature = connection
        .request_airdrop(&sender.pubkey(), LAMPORTS_PER_SOL)
        .await?;
    loop {
        let confirmed = connection.confirm_transaction(&airdrop_signature).await?;
        if confirmed {
            break;
        }
    }

    // Check balance before transfer
    let pre_balance1 = connection.get_balance(&sender.pubkey()).await?;
    let pre_balance2 = connection.get_balance(&recipient.pubkey()).await?;

    // Define the amount to transfer
    let transfer_amount = LAMPORTS_PER_SOL / 100; // 0.01 SOL

    // Create a transfer instruction for transferring SOL from sender to recipient
    let transfer_instruction =
        system_instruction::transfer(&sender.pubkey(), &recipient.pubkey(), transfer_amount);

    // Add the transfer instruction to a new transaction
    let mut transaction =
        Transaction::new_with_payer(&[transfer_instruction], Some(&sender.pubkey()));
    let blockhash = connection.get_latest_blockhash().await?;
    transaction.sign(&[&sender], blockhash);

    // Send the transaction to the network
    let transaction_signature = connection
        .send_and_confirm_transaction(&transaction)
        .await?;

    // Check balance after transfer
    let post_balance1 = connection.get_balance(&sender.pubkey()).await?;
    let post_balance2 = connection.get_balance(&recipient.pubkey()).await?;

    println!(
        "Sender prebalance: {}",
        pre_balance1 as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Recipient prebalance: {}",
        pre_balance2 as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Sender postbalance: {}",
        post_balance1 as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Recipient postbalance: {}",
        post_balance2 as f64 / LAMPORTS_PER_SOL as f64
    );
    println!("Transaction Signature: {}", transaction_signature);

    Ok(())
}