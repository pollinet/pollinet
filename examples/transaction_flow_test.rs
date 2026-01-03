//! Complete transaction flow example (using FFI Transport)
//!  
//! Demonstrates the full lifecycle of a transaction through PolliNet:
//! 1. Fragment a transaction
//! 2. Add to outbound queue
//! 3. Read from outbound queue (simulate BLE transmission)
//! 4. Add to reassembly buffers
//! 5. Reassemble when complete
//! 6. Add to received queue
//! 7. Read from received queue
//! 8. Send to confirmation queue

use pollinet::ffi::transport::HostDrivenBleTransport;
use pollinet::transaction;
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
    system_instruction,
};
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("=== PolliNet Transaction Flow Test ===\n");

    // Initialize transport (mimics Android host-driven mode)
    info!("Step 1: Initializing Host-Driven BLE Transport...");
    let transport = HostDrivenBleTransport::new();
    info!("✅ Transport initialized\n");

    // Create a sample transaction
    info!("Step 2: Creating sample transaction...");
    let sender = Keypair::new();
    let recipient = Keypair::new();
    let amount = 1_000_000; // 0.001 SOL

    let instruction = system_instruction::transfer(
        &sender.pubkey(),
        &recipient.pubkey(),
        amount,
    );

    let mut transaction = Transaction::new_with_payer(
        &[instruction],
        Some(&sender.pubkey()),
    );

    // Use a dummy blockhash for testing
    transaction.message.recent_blockhash = solana_sdk::hash::Hash::default();
    transaction.sign(&[&sender], transaction.message.recent_blockhash);

    // Serialize the transaction (this is what would be sent over BLE)
    let tx_bytes = bincode1::serialize(&transaction)?;
    info!("✅ Transaction created: {} bytes", tx_bytes.len());
    info!("   Sender: {}", sender.pubkey());
    info!("   Recipient: {}", recipient.pubkey());
    info!("   Amount: {} lamports\n", amount);

    // ========================================================================
    // SENDER SIDE: Fragment and Queue
    // ========================================================================
    
    info!("Step 3: Fragmenting transaction (SENDER)...");
    // Fragment with MTU=512 (typical BLE MTU after negotiation)
    let mtu = 512;
    let max_payload = mtu - 10; // Reserve 10 bytes for overhead
    
    let fragments = transport.queue_transaction(tx_bytes.clone(), Some(max_payload))?;
    info!("✅ Fragmented and queued: {} fragments", fragments.len());
    for (idx, fragment) in fragments.iter().enumerate() {
        info!("   Fragment {}/{}: {} bytes (data: {} bytes)", 
            idx + 1, fragments.len(), 
            fragment.data.len() + 50, // Approximate total size with metadata
            fragment.data.len());
    }
    info!("");

    // Check outbound queue size
    let outbound_size = transport.outbound_queue_size();
    info!("📊 Outbound queue size: {} fragments\n", outbound_size);

    // ========================================================================
    // TRANSMISSION SIMULATION: Read from outbound, send to inbound
    // ========================================================================
    
    info!("Step 4: Simulating BLE transmission...");
    let mut transmitted_fragments = Vec::new();
    
    while let Some(fragment_bytes) = transport.next_outbound() {
        let fragment_num = transmitted_fragments.len() + 1;
        info!("   📤 Transmitted fragment {}/{} ({} bytes)", 
            fragment_num, fragments.len(), fragment_bytes.len());
        transmitted_fragments.push(fragment_bytes);
    }
    
    info!("✅ All {} fragments transmitted", transmitted_fragments.len());
    
    // Check outbound queue is empty
    let outbound_after = transport.outbound_queue_size();
    info!("📊 Outbound queue after transmission: {} fragments\n", outbound_after);

    // ========================================================================
    // RECEIVER SIDE: Reassembly
    // ========================================================================
    
    info!("Step 5: Receiving and reassembling (RECEIVER)...");
    
    for (idx, fragment_bytes) in transmitted_fragments.iter().enumerate() {
        // Push to inbound buffers (simulates receiving over BLE)
        match transport.push_inbound(fragment_bytes.clone()) {
            Ok(_) => {
                info!("   📥 Received fragment {}/{}", idx + 1, transmitted_fragments.len());
                
                // Check metrics after each fragment
                let metrics = transport.metrics();
                info!("      Fragments buffered: {}, Transactions complete: {}", 
                    metrics.fragments_buffered, metrics.transactions_complete);
            }
            Err(e) => {
                error!("   ❌ Failed to receive fragment {}: {}", idx + 1, e);
            }
        }
    }
    
    info!("✅ All fragments received\n");

    // Check if transaction was reassembled
    info!("Step 6: Checking reassembly status...");
    let metrics = transport.metrics();
    info!("📊 Reassembly metrics:");
    info!("   Fragments buffered: {}", metrics.fragments_buffered);
    info!("   Transactions complete: {}", metrics.transactions_complete);
    info!("   Reassembly failures: {}", metrics.reassembly_failures);
    
    if !metrics.last_error.is_empty() {
        error!("   ⚠️  Last error: {}", metrics.last_error);
    }
    info!("");

    // ========================================================================
    // RECEIVED QUEUE: Read reassembled transaction
    // ========================================================================
    
    info!("Step 7: Reading from received queue...");
    let received_queue_size = transport.received_queue_size();
    info!("📊 Received queue size: {} transactions", received_queue_size);
    
    if received_queue_size > 0 {
        // Get the received transaction
        if let Some((tx_id, reassembled_bytes, received_at)) = transport.next_received_transaction() {
            info!("✅ Retrieved received transaction:");
            info!("   Transaction ID: {}", tx_id);
            info!("   Received at: {}", received_at);
            info!("   Size: {} bytes", reassembled_bytes.len());
            
            // Verify it matches original
            if reassembled_bytes == tx_bytes {
                info!("   ✅ Transaction matches original!");
            } else {
                error!("   ❌ Transaction does NOT match original!");
                error!("      Original: {} bytes", tx_bytes.len());
                error!("      Reassembled: {} bytes", reassembled_bytes.len());
            }
            info!("");
            
            // ====================================================================
            // CONFIRMATION QUEUE: Simulate successful submission
            // ====================================================================
            
            info!("Step 8: Simulating RPC submission and queueing confirmation...");
            
            // Simulate a transaction signature from RPC
            let mock_signature = "5j7s8K9L2m3n4p5q6r7s8t9u0v1w2x3y4z5a6b7c8d9e0f1g2h3i4j5k6l7m8n9o0p";
            info!("   Mock RPC signature: {}", mock_signature);
            
            // Queue the confirmation for relay back to sender
            transport.queue_confirmation(&tx_id, mock_signature)?;
            info!("✅ Confirmation queued for relay\n");
            
            // Check confirmation queue
            let confirmation_queue_size = transport.confirmation_queue_size();
            info!("📊 Confirmation queue size: {} confirmations", confirmation_queue_size);
            
            // Get the confirmation
            if let Some((conf_tx_id, signature, confirmed_at)) = transport.next_confirmation() {
                info!("✅ Retrieved confirmation:");
                info!("   Transaction ID: {}", conf_tx_id);
                info!("   Signature: {}", signature);
                info!("   Confirmed at: {}", confirmed_at);
            }
            
        } else {
            error!("❌ Failed to retrieve received transaction (queue reported size > 0 but returned None)");
        }
    } else {
        error!("❌ Received queue is empty! Transaction was not reassembled.");
        error!("   This could mean:");
        error!("   - Fragments were rejected as duplicates");
        error!("   - Fragment format mismatch");
        error!("   - Reassembly failed");
        error!("");
        error!("   Checking metrics...");
        let final_metrics = transport.metrics();
        error!("   Fragments buffered: {}", final_metrics.fragments_buffered);
        error!("   Transactions complete: {}", final_metrics.transactions_complete);
        error!("   Last error: {}", final_metrics.last_error);
    }
    
    // ========================================================================
    // FINAL SUMMARY
    // ========================================================================
    
    info!("\n=== Transaction Flow Summary ===");
    info!("✅ 1. Transaction created: {} bytes", tx_bytes.len());
    info!("✅ 2. Fragmented: {} fragments", fragments.len());
    info!("✅ 3. Queued to outbound queue");
    info!("✅ 4. Transmitted {} fragments", transmitted_fragments.len());
    info!("✅ 5. Received all fragments");
    
    let final_metrics = transport.metrics();
    if final_metrics.transactions_complete > 0 {
        info!("✅ 6. Transaction reassembled successfully");
    } else {
        error!("❌ 6. Transaction reassembly FAILED");
    }
    
    let final_received_size = transport.received_queue_size();
    if final_received_size > 0 || final_metrics.transactions_complete > 0 {
        info!("✅ 7. Added to received queue");
    } else {
        error!("❌ 7. NOT added to received queue (likely duplicate)");
    }
    
    let final_confirmation_size = transport.confirmation_queue_size();
    if final_confirmation_size > 0 {
        info!("✅ 8. Confirmation queued for relay");
    } else {
        error!("❌ 8. Confirmation NOT queued");
    }
    
    info!("\n🎉 Transaction flow test complete!");
    info!("\nThis example demonstrates the complete flow:");
    info!("  - Sender fragments and queues transaction");
    info!("  - Fragments are transmitted over BLE");
    info!("  - Receiver reassembles fragments");
    info!("  - Complete transaction is queued for submission");
    info!("  - Confirmation is queued for relay back to sender");

    Ok(())
}

