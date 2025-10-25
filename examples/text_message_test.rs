//! Example: Text Message Test
//!
//! This example demonstrates the text message listening and sending functionality:
//!
//! 1. Start BLE advertising and text listener
//! 2. Send text messages to connected peers
//! 3. Check for incoming text messages
//!
//! Run with: cargo run --example text_message_test

use pollinet::PolliNetSDK;
use tracing::{info, warn, error};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("🚀 === PolliNet Text Message Test ===");
    info!("This example demonstrates text message listening and sending");

    // Initialize SDK
    let sdk = PolliNetSDK::new().await?;
    info!("✅ PolliNet SDK initialized");

    // Start BLE advertising
    sdk.start_ble_networking().await?;
    info!("📢 BLE advertising started");

    // Start text message listener
    sdk.start_text_listener().await?;
    info!("🎧 Text message listener started");

    // Send some test messages
    info!("\n📤 Sending test messages...");
    
    // Send a few test messages
    let test_messages = vec![
        "Hello from PolliNet!",
        "This is a test message",
        "BLE text messaging works!",
    ];

    for (i, message) in test_messages.iter().enumerate() {
        info!("Sending message {}: '{}'", i + 1, message);
        match sdk.send_text_message("broadcast", message).await {
            Ok(_) => {
                info!("✅ Message {} sent successfully", i + 1);
            }
            Err(e) => {
                error!("❌ Failed to send message {}: {}", i + 1, e);
            }
        }
        
        // Wait a bit between messages
        sleep(Duration::from_millis(500)).await;
    }

    // Check for incoming messages
    info!("\n📨 Checking for incoming messages...");
    
    for i in 0..10 {
        info!("Check {}: Looking for incoming messages...", i + 1);
        
        // Check if there are pending messages
        if sdk.has_pending_messages().await {
            info!("📬 Pending messages detected!");
            
            // Retrieve messages
            match sdk.check_incoming_messages().await {
                Ok(messages) => {
                    if !messages.is_empty() {
                        info!("📨 Retrieved {} message(s):", messages.len());
                        for (j, message) in messages.iter().enumerate() {
                            info!("   {}. '{}'", j + 1, message);
                        }
                    } else {
                        info!("📭 No messages retrieved");
                    }
                }
                Err(e) => {
                    error!("❌ Failed to check messages: {}", e);
                }
            }
        } else {
            info!("📭 No pending messages");
        }
        
        // Wait before next check
        sleep(Duration::from_secs(2)).await;
    }

    info!("\n🏁 Text message test completed!");
    info!("   Text message listening and sending functionality is now implemented");
    info!("   Messages are automatically buffered and can be retrieved with check_incoming_messages()");

    Ok(())
}
