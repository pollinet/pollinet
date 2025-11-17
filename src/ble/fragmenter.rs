//! Transaction Fragmentation and Reassembly
//!
//! Handles splitting large Solana transactions into BLE-friendly fragments
//! and reconstructing them on the receiving side.

use crate::ble::mesh::{TransactionFragment, MAX_FRAGMENT_DATA};
use sha2::{Digest, Sha256};

/// Fragment a signed Solana transaction for BLE transmission
///
/// Takes a complete signed transaction and splits it into fragments
/// that fit within BLE packet size constraints.
///
/// # Arguments
/// * `transaction_bytes` - Complete signed Solana transaction (serialized)
///
/// # Returns
/// Vector of TransactionFragment ready for mesh transmission
pub fn fragment_transaction(transaction_bytes: &[u8]) -> Vec<TransactionFragment> {
    tracing::info!("Fragmenting transaction: {} bytes", transaction_bytes.len());

    // Calculate transaction ID (SHA256 hash)
    let mut hasher = Sha256::new();
    hasher.update(transaction_bytes);
    let hash_result = hasher.finalize();
    let mut transaction_id = [0u8; 32];
    transaction_id.copy_from_slice(&hash_result);

    tracing::debug!("Transaction ID: {}", hex::encode(&transaction_id));

    // Calculate number of fragments needed
    let total_fragments = (transaction_bytes.len() + MAX_FRAGMENT_DATA - 1) / MAX_FRAGMENT_DATA;

    tracing::info!(
        "Transaction requires {} fragments ({} bytes per fragment max)",
        total_fragments,
        MAX_FRAGMENT_DATA
    );

    // Create fragments
    let mut fragments = Vec::new();
    for (index, chunk) in transaction_bytes.chunks(MAX_FRAGMENT_DATA).enumerate() {
        let fragment = TransactionFragment {
            transaction_id,
            fragment_index: index as u16,
            total_fragments: total_fragments as u16,
            data: chunk.to_vec(),
        };

        tracing::debug!(
            "Fragment {}/{}: {} bytes",
            index + 1,
            total_fragments,
            chunk.len()
        );

        fragments.push(fragment);
    }

    tracing::info!("✅ Created {} fragments", fragments.len());
    fragments
}

/// Reconstruct a complete transaction from fragments
///
/// Takes a collection of fragments and reconstructs the original transaction.
/// Fragments can be provided in any order.
///
/// # Arguments
/// * `fragments` - Collection of transaction fragments
///
/// # Returns
/// * `Ok(Vec<u8>)` - Reconstructed transaction bytes
/// * `Err(String)` - Error message if reconstruction fails
pub fn reconstruct_transaction(fragments: &[TransactionFragment]) -> Result<Vec<u8>, String> {
    if fragments.is_empty() {
        return Err("No fragments provided".to_string());
    }

    // All fragments must have the same transaction ID
    let transaction_id = fragments[0].transaction_id;
    let total_fragments = fragments[0].total_fragments;

    tracing::info!(
        "Reconstructing transaction from {} fragments (expected {})",
        fragments.len(),
        total_fragments
    );

    // Verify all fragments belong to the same transaction
    for fragment in fragments {
        if fragment.transaction_id != transaction_id {
            return Err("Fragment transaction ID mismatch".to_string());
        }
        if fragment.total_fragments != total_fragments {
            return Err("Fragment total count mismatch".to_string());
        }
    }

    // Check if we have all fragments
    if fragments.len() != total_fragments as usize {
        return Err(format!(
            "Missing fragments: have {}, need {}",
            fragments.len(),
            total_fragments
        ));
    }

    // Sort fragments by index
    let mut sorted_fragments = fragments.to_vec();
    sorted_fragments.sort_by_key(|f| f.fragment_index);

    // Verify we have all indices
    for (expected_index, fragment) in sorted_fragments.iter().enumerate() {
        if fragment.fragment_index != expected_index as u16 {
            return Err(format!(
                "Missing fragment at index {}, found {}",
                expected_index, fragment.fragment_index
            ));
        }
    }

    // Reconstruct the transaction
    let mut reconstructed = Vec::new();
    for fragment in &sorted_fragments {
        reconstructed.extend_from_slice(&fragment.data);
    }

    tracing::info!(
        "✅ Reconstructed transaction: {} bytes",
        reconstructed.len()
    );

    // Verify the transaction ID matches
    let mut hasher = Sha256::new();
    hasher.update(&reconstructed);
    let hash_result = hasher.finalize();
    let mut reconstructed_id = [0u8; 32];
    reconstructed_id.copy_from_slice(&hash_result);

    if reconstructed_id != transaction_id {
        return Err("Transaction hash mismatch after reconstruction".to_string());
    }

    tracing::info!("✅ Transaction hash verified");

    Ok(reconstructed)
}

/// Calculate statistics for transaction fragmentation
#[derive(Debug, Clone)]
pub struct FragmentationStats {
    pub original_size: usize,
    pub fragment_count: usize,
    pub max_fragment_size: usize,
    pub avg_fragment_size: usize,
    pub total_overhead: usize,
    pub efficiency: f32,
}

impl FragmentationStats {
    pub fn calculate(transaction_bytes: &[u8]) -> Self {
        let original_size = transaction_bytes.len();
        let fragment_count = (original_size + MAX_FRAGMENT_DATA - 1) / MAX_FRAGMENT_DATA;

        // Each fragment has overhead: mesh header (42) + fragment header (38)
        let per_fragment_overhead = 42 + 38;
        let total_overhead = per_fragment_overhead * fragment_count;

        let max_fragment_size = MAX_FRAGMENT_DATA;
        let avg_fragment_size = original_size / fragment_count;

        let total_bytes = original_size + total_overhead;
        let efficiency = (original_size as f32 / total_bytes as f32) * 100.0;

        Self {
            original_size,
            fragment_count,
            max_fragment_size,
            avg_fragment_size,
            total_overhead,
            efficiency,
        }
    }

    pub fn print(&self) {
        tracing::info!("Fragmentation Statistics:");
        tracing::info!("  Original size: {} bytes", self.original_size);
        tracing::info!("  Fragment count: {}", self.fragment_count);
        tracing::info!("  Max fragment size: {} bytes", self.max_fragment_size);
        tracing::info!("  Avg fragment size: {} bytes", self.avg_fragment_size);
        tracing::info!("  Total overhead: {} bytes", self.total_overhead);
        tracing::info!("  Efficiency: {:.1}%", self.efficiency);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragment_small_transaction() {
        // Small transaction that fits in one fragment
        let tx_bytes = vec![1u8; 200];

        let fragments = fragment_transaction(&tx_bytes);

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].fragment_index, 0);
        assert_eq!(fragments[0].total_fragments, 1);
        assert_eq!(fragments[0].data.len(), 200);
    }

    #[test]
    fn test_fragment_large_transaction() {
        // Transaction that requires multiple fragments
        let tx_bytes = vec![42u8; 1000];

        let fragments = fragment_transaction(&tx_bytes);

        // Should need 3 fragments (468 bytes max per fragment)
        assert_eq!(fragments.len(), 3);

        // All fragments should have the same transaction ID
        let tx_id = fragments[0].transaction_id;
        for fragment in &fragments {
            assert_eq!(fragment.transaction_id, tx_id);
            assert_eq!(fragment.total_fragments, 3);
        }

        // First two fragments should be full, last one smaller
        assert_eq!(fragments[0].data.len(), MAX_FRAGMENT_DATA);
        assert_eq!(fragments[1].data.len(), MAX_FRAGMENT_DATA);
        assert_eq!(fragments[2].data.len(), 1000 - (2 * MAX_FRAGMENT_DATA));
    }

    #[test]
    fn test_reconstruct_in_order() {
        let original = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let fragments = fragment_transaction(&original);
        let reconstructed = reconstruct_transaction(&fragments).unwrap();

        assert_eq!(original, reconstructed);
    }

    #[test]
    fn test_reconstruct_out_of_order() {
        // Create a larger transaction to ensure multiple fragments
        let mut original = Vec::new();
        for i in 0..1000 {
            original.push((i % 256) as u8);
        }

        let mut fragments = fragment_transaction(&original);

        // Shuffle fragments
        fragments.reverse();

        let reconstructed = reconstruct_transaction(&fragments).unwrap();

        assert_eq!(original, reconstructed);
    }

    #[test]
    fn test_reconstruct_missing_fragment() {
        let original = vec![1u8; 1000];

        let mut fragments = fragment_transaction(&original);

        // Remove one fragment
        fragments.remove(1);

        let result = reconstruct_transaction(&fragments);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing fragments"));
    }

    #[test]
    fn test_reconstruct_duplicate_fragment() {
        let original = vec![1u8; 1000];

        let mut fragments = fragment_transaction(&original);

        // Duplicate a fragment (but correct count)
        let dup = fragments[0].clone();
        fragments[1] = dup;

        let result = reconstruct_transaction(&fragments);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing fragment"));
    }

    #[test]
    fn test_fragmentation_stats() {
        let tx_bytes = vec![1u8; 1000];

        let stats = FragmentationStats::calculate(&tx_bytes);

        assert_eq!(stats.original_size, 1000);
        assert_eq!(stats.fragment_count, 3);
        assert!(stats.efficiency < 100.0);
        assert!(stats.efficiency > 80.0); // Should be reasonably efficient
    }

    #[test]
    fn test_realistic_solana_transaction() {
        // Typical Solana transaction size is ~300-500 bytes
        let realistic_tx = vec![42u8; 350];

        let fragments = fragment_transaction(&realistic_tx);

        // Should fit in 1 fragment
        assert_eq!(fragments.len(), 1);

        let reconstructed = reconstruct_transaction(&fragments).unwrap();
        assert_eq!(realistic_tx, reconstructed);
    }

    #[test]
    fn test_max_size_transaction() {
        // Solana max transaction size is ~1232 bytes
        let max_tx = vec![255u8; 1232];

        let fragments = fragment_transaction(&max_tx);

        // Should need 3 fragments
        assert_eq!(fragments.len(), 3);

        let reconstructed = reconstruct_transaction(&fragments).unwrap();
        assert_eq!(max_tx, reconstructed);

        let stats = FragmentationStats::calculate(&max_tx);
        stats.print();
    }

    #[test]
    fn test_hash_verification() {
        let original = vec![1u8; 500];

        let fragments = fragment_transaction(&original);

        // Corrupt a fragment's data
        let mut corrupted_fragments = fragments.clone();
        corrupted_fragments[0].data[0] = 255;

        let result = reconstruct_transaction(&corrupted_fragments);

        // Should fail hash verification
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("hash mismatch"));
    }
}
