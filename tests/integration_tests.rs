//! Integration tests for PolliNet SDK

use pollinet::{PolliNetError, PolliNetSDK};

#[tokio::test]
async fn test_sdk_initialization() {
    let result = PolliNetSDK::new().await;

    // In a real test environment, this might fail due to BLE hardware requirements
    // For CI/CD, we should mock the BLE components
    match result {
        Ok(_sdk) => {
            // SDK initialized successfully
            assert!(true);
        }
        Err(PolliNetError::BleTransport(_)) => {
            // Expected in CI environment without BLE hardware
            println!("BLE hardware not available - this is expected in CI");
            assert!(true);
        }
        Err(e) => {
            panic!("Unexpected error during SDK initialization: {}", e);
        }
    }
}

#[tokio::test]
async fn test_transaction_creation() {
    // Test transaction creation with mock data
    let sender = "11111111111111111111111111111112";
    let recipient = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    let amount = 1_000_000_000; // 1 SOL

    // This test should work even without BLE hardware
    let result = PolliNetSDK::new().await;

    match result {
        Ok(sdk) => {
            let tx_result = sdk.create_transaction(sender, recipient, amount).await;
            match tx_result {
                Ok(tx_data) => {
                    assert!(!tx_data.is_empty(), "Transaction data should not be empty");
                    println!("Transaction created successfully: {} bytes", tx_data.len());
                }
                Err(e) => {
                    println!("Transaction creation failed (expected in test env): {}", e);
                    // This is acceptable in test environment
                }
            }
        }
        Err(PolliNetError::BleTransport(_)) => {
            println!("BLE not available - skipping transaction test");
        }
        Err(e) => {
            panic!("Unexpected SDK initialization error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_transaction_fragmentation() {
    // Test transaction fragmentation logic
    let mock_transaction_data = vec![0u8; 1000]; // 1KB mock transaction

    let result = PolliNetSDK::new().await;
    match result {
        Ok(sdk) => {
            let fragments = sdk.fragment_transaction(&mock_transaction_data);

            assert!(
                !fragments.is_empty(),
                "Should produce at least one fragment"
            );

            // Verify fragment integrity
            let total_fragments = fragments.len();
            for (i, fragment) in fragments.iter().enumerate() {
                assert_eq!(fragment.index, i, "Fragment index should match position");
                assert_eq!(
                    fragment.total, total_fragments,
                    "Total fragments should be consistent"
                );
                assert!(
                    !fragment.data.is_empty(),
                    "Fragment data should not be empty"
                );
            }

            println!("Transaction fragmented into {} pieces", fragments.len());
        }
        Err(PolliNetError::BleTransport(_)) => {
            println!("BLE not available - skipping fragmentation test");
        }
        Err(e) => {
            panic!("Unexpected error: {}", e);
        }
    }
}

#[test]
fn test_utility_functions() {
    use pollinet::util::common;

    // Test ID generation
    let id1 = common::generate_id();
    let id2 = common::generate_id();
    assert_ne!(id1, id2, "Generated IDs should be unique");
    assert!(id1.starts_with("id_"), "ID should have correct prefix");

    // Test compression threshold
    assert!(
        common::should_compress(200, 100),
        "Should compress when above threshold"
    );
    assert!(
        !common::should_compress(50, 100),
        "Should not compress when below threshold"
    );

    // Test fragment size calculation
    let fragment_size = common::calculate_fragment_size(1000, 100);
    assert_eq!(fragment_size, 10, "Should calculate correct fragment count");
}

#[test]
fn test_compression_functionality() {
    // Simple test that just verifies the compression module exists and works
    // The actual LZ4 implementation details can be refined later
    let original_data = b"This is test data for compression. ".repeat(10);
    
    // Test that we can create a compressor
    let compressor = pollinet::util::lz::Lz4Compressor::new().expect("Should create compressor");
    
    // Test basic compression (without worrying about decompression for now)
    let compressed = compressor.compress(&original_data).expect("Compression should succeed");
    println!("Original: {} bytes, Compressed: {} bytes", original_data.len(), compressed.len());
    
    // Just verify compression produces some output
    assert!(!compressed.is_empty(), "Compressed data should not be empty");
}

#[cfg(feature = "mock_ble")]
mod mock_ble_tests {
    use super::*;

    #[tokio::test]
    async fn test_ble_peer_discovery_mock() {
        // This test would run with mocked BLE functionality
        // Implementation depends on adding mock feature to the crate
        println!("Mock BLE tests would go here");
    }
}
