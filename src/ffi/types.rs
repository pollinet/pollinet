//! FFI data types and JSON schemas (v1)
//! 
//! All data exchanged across FFI boundary uses JSON serialization for simplicity.
//! Each message includes a `version` field for future compatibility.

use serde::{Deserialize, Serialize};

/// Version 1 of the FFI protocol
pub const FFI_VERSION: u32 = 1;

// ============================================================================
// Result envelope
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FfiResult<T> {
    Ok { ok: bool, data: T },
    Err { ok: bool, code: String, message: String },
}

impl<T> FfiResult<T> {
    pub fn success(data: T) -> Self {
        FfiResult::Ok { ok: true, data }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        FfiResult::Err {
            ok: false,
            code: code.into(),
            message: message.into(),
        }
    }
}

// ============================================================================
// Transaction builder requests
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnsignedTransactionRequest {
    pub version: u32,
    pub sender: String,
    pub recipient: String,
    pub fee_payer: String,
    pub amount: u64,
    pub nonce_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnsignedSplTransactionRequest {
    pub version: u32,
    pub sender_wallet: String,
    pub recipient_wallet: String,
    pub fee_payer: String,
    pub mint_address: String,
    pub amount: u64,
    pub nonce_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastUnsignedVoteRequest {
    pub version: u32,
    pub voter: String,
    pub proposal_id: String,
    pub vote_account: String,
    pub choice: u8,
    pub fee_payer: String,
    pub nonce_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareOfflineBundleRequest {
    pub version: u32,
    pub count: u32,
    pub sender_keypair: String, // base64 encoded bytes
    pub bundle_file: Option<String>,
}

// ============================================================================
// Nonce and offline bundle responses
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedNonceData {
    pub version: u32,
    pub nonce_account: String,
    pub authority: String,
    pub blockhash: String,
    pub lamports_per_signature: u64,
    pub cached_at: u64,
    pub used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineTransactionBundle {
    pub version: u32,
    pub nonce_caches: Vec<CachedNonceData>,
    pub max_transactions: u32,
    pub created_at: u64,
}

// ============================================================================
// Fragmentation types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fragment {
    pub id: String,
    pub index: u32,
    pub total: u32,
    pub data: String, // base64
    pub fragment_type: String, // "FragmentStart" | "FragmentContinue" | "FragmentEnd"
    pub checksum: String, // base64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentList {
    pub fragments: Vec<Fragment>,
}

// ============================================================================
// Protocol events
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEvent {
    #[serde(rename = "type")]
    pub event_type: String, // "TransactionComplete" | "TextMessage" | "Error" | "Ack"
    pub tx_id: Option<String>,
    pub size: Option<u64>,
    pub message: Option<String>,
}

// ============================================================================
// Metrics
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub fragments_buffered: u32,
    pub transactions_complete: u32,
    pub reassembly_failures: u32,
    pub last_error: String,
    pub updated_at: u64,
}

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    pub version: u32,
    pub rpc_url: Option<String>,
    pub enable_logging: bool,
    pub log_level: Option<String>,
}

