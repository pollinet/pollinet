//! Simple Transaction Queue Flow Example
//!  
//! Demonstrates queue operations without BLE dependencies:
//! 1. Fragment a transaction
//! 2. Add to outbound queue
//! 3. Read from outbound queue
//! 4. Add to reassembly buffers
//! 5. Reassemble when complete
//! 6. Add to received queue
//! 7. Read from received queue
//! 8. Queue confirmation

use pollinet::ffi::transport::HostDrivenBleTransport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== PolliNet Transaction Queue Flow Test ===\n");

    // Initialize transport (queue manager)
    println!("Step 1: Initializing Transport (Queue Manager)...");
    let transport = HostDrivenBleTransport::new();
    println!("✅ Transport initialized\n");

    // Create a dummy transaction (just random bytes)
    println!("Step 2: Creating dummy transaction...");
    let tx_bytes: Vec<u8> = vec![
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Header
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Data
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, // More data
        0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x00, // Even more data
    ];
    
    // Pad to make it more realistic (typical transaction is ~200-400 bytes)
    let mut tx_bytes = tx_bytes;
    tx_bytes.extend(vec![0x42; 250]); // Pad to 282 bytes total
    
    println!("✅ Dummy transaction created: {} bytes", tx_bytes.len());
    println!("   First 16 bytes: {:02X?}", &tx_bytes[..16]);
    println!();

    // ========================================================================
    // SENDER SIDE: Fragment and Queue
    // ========================================================================
    
    println!("Step 3: Fragmenting transaction (SENDER)...");
    // Fragment with MTU=512 (typical BLE MTU)
    let mtu = 512;
    let max_payload = mtu - 10; // Reserve 10 bytes for overhead
    
    let fragments = transport.queue_transaction(tx_bytes.clone(), Some(max_payload))?;
    println!("✅ Fragmented and queued: {} fragments", fragments.len());
    for (idx, fragment) in fragments.iter().enumerate() {
        println!("   Fragment {}/{}: ID={}, index={}, total={}, data={} bytes", 
            idx + 1, fragments.len(),
            &fragment.id[..8],
            fragment.index,
            fragment.total,
            fragment.data.len());
    }
    println!();

    // Check outbound queue size
    let outbound_size = transport.outbound_queue_size();
    println!("📊 Outbound queue size: {} fragments\n", outbound_size);

    // ========================================================================
    // TRANSMISSION SIMULATION: Read from outbound queue
    // ========================================================================
    
    println!("Step 4: Reading from outbound queue...");
    let mut transmitted_fragments = Vec::new();
    
    while let Some(fragment_bytes) = transport.next_outbound() {
        let fragment_num = transmitted_fragments.len() + 1;
        println!("   📤 Dequeued fragment {}/{} ({} bytes)", 
            fragment_num, fragments.len(), fragment_bytes.len());
        transmitted_fragments.push(fragment_bytes);
    }
    
    println!("✅ All {} fragments dequeued", transmitted_fragments.len());
    
    // Check outbound queue is empty
    let outbound_after = transport.outbound_queue_size();
    println!("📊 Outbound queue after dequeue: {} fragments\n", outbound_after);

    // ========================================================================
    // RECEIVER SIDE: Reassembly
    // ========================================================================
    
    println!("Step 5: Adding fragments to reassembly buffers (RECEIVER)...");
    
    for (idx, fragment_bytes) in transmitted_fragments.iter().enumerate() {
        // Push to inbound buffers for reassembly
        match transport.push_inbound(fragment_bytes.clone()) {
            Ok(_) => {
                println!("   📥 Added fragment {}/{} to reassembly buffer", idx + 1, transmitted_fragments.len());
                
                // Check metrics after each fragment
                let metrics = transport.metrics();
                println!("      Fragments buffered: {}, Transactions complete: {}", 
                    metrics.fragments_buffered, metrics.transactions_complete);
            }
            Err(e) => {
                eprintln!("   ❌ Failed to add fragment {}: {}", idx + 1, e);
            }
        }
    }
    
    println!("✅ All fragments added to reassembly\n");

    // Check if transaction was reassembled
    println!("Step 6: Checking reassembly status...");
    let metrics = transport.metrics();
    println!("📊 Reassembly metrics:");
    println!("   Fragments buffered: {}", metrics.fragments_buffered);
    println!("   Transactions complete: {}", metrics.transactions_complete);
    println!("   Reassembly failures: {}", metrics.reassembly_failures);
    
    if !metrics.last_error.is_empty() {
        eprintln!("   ⚠️  Last error: {}", metrics.last_error);
    }
    println!();

    // ========================================================================
    // RECEIVED QUEUE: Read reassembled transaction
    // ========================================================================
    
    println!("Step 7: Reading from received queue...");
    let received_queue_size = transport.received_queue_size();
    println!("📊 Received queue size: {} transactions", received_queue_size);
    
    if received_queue_size > 0 {
        // Get the received transaction
        if let Some((tx_id, reassembled_bytes, received_at)) = transport.next_received_transaction() {
            println!("✅ Retrieved received transaction:");
            println!("   Transaction ID: {}", tx_id);
            println!("   Received at: {}", received_at);
            println!("   Size: {} bytes", reassembled_bytes.len());
            println!("   First 16 bytes: {:02X?}", &reassembled_bytes[..16.min(reassembled_bytes.len())]);
            
            // Verify it matches original
            if reassembled_bytes == tx_bytes {
                println!("   ✅ Transaction matches original!");
            } else {
                eprintln!("   ❌ Transaction does NOT match original!");
                eprintln!("      Original: {} bytes", tx_bytes.len());
                eprintln!("      Reassembled: {} bytes", reassembled_bytes.len());
            }
            println!();
            
            // ====================================================================
            // CONFIRMATION QUEUE: Simulate successful submission
            // ====================================================================
            
            println!("Step 8: Queueing confirmation...");
            
            // Simulate a transaction signature (what RPC would return)
            let mock_signature = "5j7s8K9L2m3n4p5q6r7s8t9u0v1w2x3y4z5a6b7c8d9e0f1g2h3i4j5k6l7m8n9o0p";
            println!("   Mock signature: {}...", &mock_signature[..32]);
            
            // Queue the confirmation
            transport.queue_confirmation(&tx_id, mock_signature)?;
            println!("✅ Confirmation queued\n");
            
            // Check confirmation queue
            let confirmation_queue_size = transport.confirmation_queue_size();
            println!("📊 Confirmation queue size: {} confirmations", confirmation_queue_size);
            
            // Get the confirmation
            if let Some((conf_tx_id, signature, confirmed_at)) = transport.next_confirmation() {
                println!("✅ Retrieved confirmation:");
                println!("   Transaction ID: {}", conf_tx_id);
                println!("   Signature: {}...", &signature[..32]);
                println!("   Confirmed at: {}", confirmed_at);
            }
            
        } else {
            eprintln!("❌ Failed to retrieve transaction (queue reported size > 0 but returned None)");
        }
    } else {
        eprintln!("❌ Received queue is empty! Transaction was not reassembled.");
        eprintln!("   Possible reasons:");
        eprintln!("   - Fragments were rejected as duplicates");
        eprintln!("   - Fragment format mismatch");
        eprintln!("   - Reassembly failed");
        eprintln!();
        eprintln!("   Metrics:");
        let final_metrics = transport.metrics();
        eprintln!("   - Fragments buffered: {}", final_metrics.fragments_buffered);
        eprintln!("   - Transactions complete: {}", final_metrics.transactions_complete);
        eprintln!("   - Last error: {}", final_metrics.last_error);
    }
    
    // ========================================================================
    // FINAL SUMMARY
    // ========================================================================
    
    println!("\n=== Queue Flow Summary ===");
    println!("✅ 1. Transaction created: {} bytes", tx_bytes.len());
    println!("✅ 2. Fragmented: {} fragments", fragments.len());
    println!("✅ 3. Queued to outbound queue");
    println!("✅ 4. Dequeued {} fragments", transmitted_fragments.len());
    println!("✅ 5. Added all fragments to reassembly");
    
    let final_metrics = transport.metrics();
    if final_metrics.transactions_complete > 0 {
        println!("✅ 6. Transaction reassembled successfully");
    } else {
        eprintln!("❌ 6. Transaction reassembly FAILED");
    }
    
    let final_received_size = transport.received_queue_size();
    if final_received_size > 0 || final_metrics.transactions_complete > 0 {
        println!("✅ 7. Added to received queue");
    } else {
        eprintln!("❌ 7. NOT added to received queue");
    }
    
    let final_confirmation_size = transport.confirmation_queue_size();
    if final_confirmation_size > 0 {
        println!("✅ 8. Confirmation queued");
    } else {
        eprintln!("❌ 8. Confirmation NOT queued");
    }
    
    println!("\n🎉 Queue flow test complete!");
    println!("\nThis example demonstrates:");
    println!("  1. Fragmenting a transaction");
    println!("  2. Queueing fragments (outbound queue)");
    println!("  3. Dequeuing fragments");
    println!("  4. Reassembling fragments (inbound buffers)");
    println!("  5. Moving to received queue");
    println!("  6. Queueing confirmations");
    println!("\nNo BLE dependencies - just pure queue operations!");

    Ok(())
}

