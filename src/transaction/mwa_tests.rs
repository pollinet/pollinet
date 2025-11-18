#[cfg(test)]
mod mwa_tests {
    use crate::transaction::{CachedNonceData, TransactionService};

    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use solana_sdk::signature::{Keypair, Signer};

    #[tokio::test]
    async fn test_create_unsigned_offline_transaction() {
        // Create test service without RPC
        let service = TransactionService::new().await.unwrap();

        // Generate test keypairs
        let sender = Keypair::new();
        let nonce_authority = Keypair::new();
        let recipient = Keypair::new();

        // Create mock cached nonce
        let cached_nonce = CachedNonceData {
            nonce_account: Keypair::new().pubkey().to_string(),
            authority: nonce_authority.pubkey().to_string(),
            blockhash: "11111111111111111111111111111111".to_string(),
            lamports_per_signature: 5000,
            cached_at: 1234567890,
            used: false,
        };

        // Create unsigned transaction using PUBLIC KEYS only
        let result = service.create_unsigned_offline_transaction(
            &sender.pubkey().to_string(),
            &recipient.pubkey().to_string(),
            1_000_000, // 0.001 SOL
            &nonce_authority.pubkey().to_string(),
            &cached_nonce,
        );

        assert!(result.is_ok(), "Should create unsigned transaction");
        let unsigned_tx_base64 = result.unwrap();

        // Verify it's valid base64
        let tx_bytes = general_purpose::STANDARD.decode(&unsigned_tx_base64);
        assert!(tx_bytes.is_ok(), "Should be valid base64");

        println!(
            "✅ Created unsigned transaction: {} bytes",
            tx_bytes.unwrap().len()
        );
    }

    #[tokio::test]
    async fn test_get_transaction_message_to_sign() {
        let service = TransactionService::new().await.unwrap();

        // Create test data
        let sender = Keypair::new();
        let nonce_authority = Keypair::new();
        let recipient = Keypair::new();

        let cached_nonce = CachedNonceData {
            nonce_account: Keypair::new().pubkey().to_string(),
            authority: nonce_authority.pubkey().to_string(),
            blockhash: "11111111111111111111111111111111".to_string(),
            lamports_per_signature: 5000,
            cached_at: 1234567890,
            used: false,
        };

        // Create unsigned transaction
        let unsigned_tx = service
            .create_unsigned_offline_transaction(
                &sender.pubkey().to_string(),
                &recipient.pubkey().to_string(),
                1_000_000,
                &nonce_authority.pubkey().to_string(),
                &cached_nonce,
            )
            .unwrap();

        // Extract message to sign
        let result = service.get_transaction_message_to_sign(&unsigned_tx);
        assert!(result.is_ok(), "Should extract message");

        let message_bytes = result.unwrap();
        assert!(!message_bytes.is_empty(), "Message should not be empty");

        println!(
            "✅ Extracted message to sign: {} bytes",
            message_bytes.len()
        );
    }

    #[tokio::test]
    async fn test_get_required_signers() {
        let service = TransactionService::new().await.unwrap();

        // Create test data
        let sender = Keypair::new();
        let nonce_authority = Keypair::new();
        let recipient = Keypair::new();

        let cached_nonce = CachedNonceData {
            nonce_account: Keypair::new().pubkey().to_string(),
            authority: nonce_authority.pubkey().to_string(),
            blockhash: "11111111111111111111111111111111".to_string(),
            lamports_per_signature: 5000,
            cached_at: 1234567890,
            used: false,
        };

        // Create unsigned transaction
        let unsigned_tx = service
            .create_unsigned_offline_transaction(
                &sender.pubkey().to_string(),
                &recipient.pubkey().to_string(),
                1_000_000,
                &nonce_authority.pubkey().to_string(),
                &cached_nonce,
            )
            .unwrap();

        // Get required signers
        let result = service.get_required_signers(&unsigned_tx);
        assert!(result.is_ok(), "Should get signers");

        let signers = result.unwrap();
        assert!(!signers.is_empty(), "Should have at least one signer");

        println!("✅ Required signers: {:?}", signers);
        println!("   Total: {} signers", signers.len());
    }

    #[tokio::test]
    async fn test_mwa_flow_without_signing() {
        // This tests the complete MWA flow without actual signing
        let service = TransactionService::new().await.unwrap();

        let sender = Keypair::new();
        let nonce_authority = Keypair::new();
        let recipient = Keypair::new();

        let cached_nonce = CachedNonceData {
            nonce_account: Keypair::new().pubkey().to_string(),
            authority: nonce_authority.pubkey().to_string(),
            blockhash: "11111111111111111111111111111111".to_string(),
            lamports_per_signature: 5000,
            cached_at: 1234567890,
            used: false,
        };

        println!("\n🔐 Testing MWA Flow (Public Keys Only)");
        println!("======================================");

        // Step 1: Create unsigned transaction (public keys only)
        println!("\n1️⃣  Creating unsigned transaction with PUBLIC KEYS...");
        let unsigned_tx = service
            .create_unsigned_offline_transaction(
                &sender.pubkey().to_string(),
                &recipient.pubkey().to_string(),
                1_000_000,
                &nonce_authority.pubkey().to_string(),
                &cached_nonce,
            )
            .unwrap();
        println!("   ✅ Unsigned transaction created");
        println!("   📦 Size: {} chars (base64)", unsigned_tx.len());

        // Step 2: Get message to sign
        println!("\n2️⃣  Extracting message for MWA signing...");
        let message = service
            .get_transaction_message_to_sign(&unsigned_tx)
            .unwrap();
        println!("   ✅ Message extracted: {} bytes", message.len());
        println!("   🔐 This would be signed by Seed Vault");

        // Step 3: Get required signers
        println!("\n3️⃣  Getting required signers...");
        let signers = service.get_required_signers(&unsigned_tx).unwrap();
        println!("   ✅ Required signers:");
        for (i, signer) in signers.iter().enumerate() {
            println!("      {}. {}", i + 1, signer);
        }

        println!("\n✅ MWA flow test complete!");
        println!("   Private keys never touched Rust code ✓");
        println!("   Ready for Seed Vault signing ✓");
    }
}
