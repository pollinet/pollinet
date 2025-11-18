//! Helper module for loading and managing nonce bundles
//!
//! Provides utilities for examples to load nonces from .offline_bundle.json

use pollinet::transaction::{CachedNonceData, OfflineTransactionBundle};
use std::path::Path;
use tracing::info;

pub const BUNDLE_FILE: &str = ".offline_bundle.json";

/// Load nonce bundle from file
pub fn load_bundle() -> Result<OfflineTransactionBundle, Box<dyn std::error::Error>> {
    if !Path::new(BUNDLE_FILE).exists() {
        return Err(format!(
            "Bundle file not found: {}. Please run 'cargo run --example nonce_refresh_utility' first to create it.",
            BUNDLE_FILE
        ).into());
    }

    let bundle = OfflineTransactionBundle::load_from_file(BUNDLE_FILE)?;
    info!("✅ Loaded bundle from: {}", BUNDLE_FILE);
    info!("   Available nonces: {}", bundle.available_nonces());
    info!("   Total nonces: {}", bundle.total_nonces());

    if bundle.is_empty() {
        return Err(format!(
            "No available nonces in bundle. Please run 'cargo run --example nonce_refresh_utility' to refresh."
        ).into());
    }

    Ok(bundle)
}

/// Get the next available nonce and return both the nonce and a mutable bundle reference
/// Returns (nonce_account_string, cached_nonce_data, bundle_index)
pub fn get_next_nonce(
    bundle: &mut OfflineTransactionBundle,
) -> Result<(String, CachedNonceData, usize), Box<dyn std::error::Error>> {
    if let Some((index, cached_nonce)) = bundle.get_next_available_nonce() {
        let nonce_account = cached_nonce.nonce_account.clone();
        let nonce_data = cached_nonce.clone();
        Ok((nonce_account, nonce_data, index))
    } else {
        Err(
            "No available nonces in bundle. Run nonce_refresh_utility to create/refresh bundle."
                .into(),
        )
    }
}

/// Save bundle to file and mark nonce as used
pub fn save_bundle_after_use(
    bundle: &mut OfflineTransactionBundle,
    index: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    bundle.mark_used(index)?;
    bundle.save_to_file(BUNDLE_FILE)?;
    info!("✅ Marked nonce as used and saved bundle");
    info!("   Remaining nonces: {}", bundle.available_nonces());
    Ok(())
}

/// Main function for cargo example compilation
/// This is a helper module, not a standalone example.
/// Run `nonce_refresh_utility` instead to create/refresh bundles.
#[allow(dead_code)]
fn main() {
    println!("This is a helper module for other examples.");
    println!("To create/refresh nonce bundles, run: cargo run --example nonce_refresh_utility");
}
