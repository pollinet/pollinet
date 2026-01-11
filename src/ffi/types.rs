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
    #[serde(default = "default_version")]
    pub version: u32,
    pub sender: String,
    pub recipient: String,
    #[serde(rename = "feePayer")]
    pub fee_payer: String,
    pub amount: u64,
    #[serde(rename = "nonceAccount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_account: Option<String>,
    #[serde(rename = "nonceData")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_data: Option<CachedNonceData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnsignedSplTransactionRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "senderWallet")]
    pub sender_wallet: String,
    #[serde(rename = "recipientWallet")]
    pub recipient_wallet: String,
    #[serde(rename = "feePayer")]
    pub fee_payer: String,
    #[serde(rename = "mintAddress")]
    pub mint_address: String,
    pub amount: u64,
    #[serde(rename = "nonceAccount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_account: Option<String>,
    #[serde(rename = "nonceData")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_data: Option<CachedNonceData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastUnsignedVoteRequest {
    pub version: u32,
    pub voter: String,
    pub proposal_id: String,
    pub vote_account: String,
    pub choice: u8,
    pub fee_payer: String,
    #[serde(rename = "nonceAccount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_account: Option<String>,
    #[serde(rename = "nonceData")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_data: Option<CachedNonceData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareOfflineBundleRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    pub count: usize,
    #[serde(rename = "senderKeypairBase64")]
    pub sender_keypair_base64: String, // base64 encoded bytes
    #[serde(rename = "bundleFile")]
    pub bundle_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOfflineTransactionRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "senderKeypairBase64")]
    pub sender_keypair_base64: String,
    #[serde(rename = "nonceAuthorityKeypairBase64")]
    pub nonce_authority_keypair_base64: String,
    pub recipient: String,
    pub amount: u64,
    // NOTE: Nonce is picked automatically from stored bundle
    // No need to send cached_nonce - we manage it internally
}

// MWA-compatible: Create UNSIGNED transaction (no keypairs, only pubkeys)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnsignedOfflineTransactionRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "senderPubkey")]
    pub sender_pubkey: String,
    #[serde(rename = "nonceAuthorityPubkey")]
    pub nonce_authority_pubkey: String,
    pub recipient: String,
    pub amount: u64,
    #[serde(rename = "nonceData")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_data: Option<CachedNonceData>,
    // NOTE: If nonce_data is not provided, nonce is picked automatically from stored bundle
}

/// Request to create an UNSIGNED offline SPL token transfer for MWA signing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnsignedOfflineSplTransactionRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "senderWallet")]
    pub sender_wallet: String,
    #[serde(rename = "recipientWallet")]
    pub recipient_wallet: String,
    #[serde(rename = "mintAddress")]
    pub mint_address: String,
    pub amount: u64,
    #[serde(rename = "feePayer")]
    pub fee_payer: String,
    #[serde(rename = "nonceData")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_data: Option<CachedNonceData>,
    // NOTE: If nonce_data is not provided, nonce is picked automatically from stored bundle
}

// Get message to sign for MWA
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMessageToSignRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "unsignedTransactionBase64")]
    pub unsigned_transaction_base64: String,
}

// Get required signers for a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetRequiredSignersRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "unsignedTransactionBase64")]
    pub unsigned_transaction_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitOfflineTransactionRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "transactionBase64")]
    pub transaction_base64: String,
    #[serde(rename = "verifyNonce", default = "default_verify_nonce")]
    pub verify_nonce: bool,
}

fn default_verify_nonce() -> bool {
    true
}

// ============================================================================
// Nonce and offline bundle responses
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedNonceData {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "nonceAccount")]
    pub nonce_account: String,
    pub authority: String,
    pub blockhash: String,
    #[serde(rename = "lamportsPerSignature")]
    pub lamports_per_signature: u64,
    #[serde(rename = "cachedAt")]
    pub cached_at: u64,
    pub used: bool,
}

impl CachedNonceData {
    /// Convert from FFI type to transaction module's type
    pub fn to_transaction_type(&self) -> crate::transaction::CachedNonceData {
        crate::transaction::CachedNonceData {
            nonce_account: self.nonce_account.clone(),
            authority: self.authority.clone(),
            blockhash: self.blockhash.clone(),
            lamports_per_signature: self.lamports_per_signature,
            cached_at: self.cached_at,
            used: self.used,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineTransactionBundle {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "nonceCaches")]
    pub nonce_caches: Vec<CachedNonceData>,
    #[serde(rename = "maxTransactions")]
    pub max_transactions: u32,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
}

impl OfflineTransactionBundle {
    /// Convert from transaction module's bundle type to FFI bundle type
    pub fn from_transaction_bundle(bundle: &crate::transaction::OfflineTransactionBundle) -> Self {
        Self {
            version: 1,
            nonce_caches: bundle.nonce_caches.iter().map(|nc| CachedNonceData {
                version: 1,
                nonce_account: nc.nonce_account.clone(),
                authority: nc.authority.clone(),
                blockhash: nc.blockhash.clone(),
                lamports_per_signature: nc.lamports_per_signature,
                cached_at: nc.cached_at,
                used: nc.used,
            }).collect(),
            max_transactions: bundle.max_transactions as u32,
            created_at: bundle.created_at,
        }
    }
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
    #[serde(rename = "fragmentsBuffered")]
    pub fragments_buffered: u32,
    #[serde(rename = "transactionsComplete")]
    pub transactions_complete: u32,
    #[serde(rename = "reassemblyFailures")]
    pub reassembly_failures: u32,
    #[serde(rename = "lastError")]
    pub last_error: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentReassemblyInfo {
    #[serde(rename = "transactionId")]
    pub transaction_id: String,
    #[serde(rename = "totalFragments")]
    pub total_fragments: usize,
    #[serde(rename = "receivedFragments")]
    pub received_fragments: usize,
    #[serde(rename = "receivedIndices")]
    pub received_indices: Vec<usize>,
    #[serde(rename = "fragmentSizes")]
    pub fragment_sizes: Vec<usize>,
    #[serde(rename = "totalBytesReceived")]
    pub total_bytes_received: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentReassemblyInfoList {
    pub transactions: Vec<FragmentReassemblyInfo>,
}

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "rpcUrl", default)]
    pub rpc_url: Option<String>,
    #[serde(rename = "enableLogging", default = "default_enable_logging")]
    pub enable_logging: bool,
    #[serde(rename = "logLevel", default)]
    pub log_level: Option<String>,
    #[serde(rename = "storageDirectory", default)]
    pub storage_directory: Option<String>,
}

pub(crate) fn default_version() -> u32 {
    1
}

fn default_enable_logging() -> bool {
    true
}

// ============================================================================
// Nonce creation for MWA
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUnsignedNonceTransactionsRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    pub count: usize,
    #[serde(rename = "payerPubkey")]
    pub payer_pubkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedNonceTransaction {
    #[serde(rename = "unsignedTransactionBase64")]
    pub unsigned_transaction_base64: String,
    #[serde(rename = "nonceKeypairBase64")]
    pub nonce_keypair_base64: Vec<String>,  // Multiple keypairs for batched transactions
    #[serde(rename = "noncePubkey")]
    pub nonce_pubkey: Vec<String>,  // Multiple pubkeys for batched transactions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheNonceAccountsRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "nonceAccounts")]
    pub nonce_accounts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddNonceSignatureRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "payerSignedTransactionBase64")]
    pub payer_signed_transaction_base64: String,
    #[serde(rename = "nonceKeypairBase64")]
    pub nonce_keypair_base64: Vec<String>,  // Multiple keypairs for batched transactions
}

// ============================================================================
// Queue Management Types (Phase 2)
// ============================================================================

/// Priority levels for outbound queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PriorityFFI {
    #[serde(rename = "HIGH")]
    High,
    #[serde(rename = "NORMAL")]
    Normal,
    #[serde(rename = "LOW")]
    Low,
}

/// Outbound transaction for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundTransactionFFI {
    #[serde(rename = "txId")]
    pub tx_id: String,
    #[serde(rename = "originalBytes")]
    pub original_bytes: String, // base64
    #[serde(rename = "fragmentCount")]
    pub fragment_count: usize,
    pub priority: PriorityFFI,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    #[serde(rename = "retryCount")]
    pub retry_count: u8,
}

/// Retry item for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryItemFFI {
    #[serde(rename = "txBytes")]
    pub tx_bytes: String, // base64
    #[serde(rename = "txId")]
    pub tx_id: String,
    #[serde(rename = "attemptCount")]
    pub attempt_count: usize,
    #[serde(rename = "lastError")]
    pub last_error: String,
    #[serde(rename = "nextRetryInSecs")]
    pub next_retry_in_secs: u64,
    #[serde(rename = "ageSeconds")]
    pub age_seconds: u64,
}

/// Confirmation status for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConfirmationStatusFFI {
    #[serde(rename = "SUCCESS")]
    Success { signature: String },
    #[serde(rename = "FAILED")]
    Failed { error: String },
}

/// Confirmation for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationFFI {
    #[serde(rename = "txId")]
    pub tx_id: String, // hex string
    pub status: ConfirmationStatusFFI,
    pub timestamp: u64,
    #[serde(rename = "relayCount")]
    pub relay_count: u8,
}

/// Queue metrics for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMetricsFFI {
    #[serde(rename = "outboundSize")]
    pub outbound_size: usize,
    #[serde(rename = "outboundHighPriority")]
    pub outbound_high_priority: usize,
    #[serde(rename = "outboundNormalPriority")]
    pub outbound_normal_priority: usize,
    #[serde(rename = "outboundLowPriority")]
    pub outbound_low_priority: usize,
    #[serde(rename = "confirmationSize")]
    pub confirmation_size: usize,
    #[serde(rename = "retrySize")]
    pub retry_size: usize,
    #[serde(rename = "retryAvgAttempts")]
    pub retry_avg_attempts: f32,
}

/// Request to push outbound transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushOutboundRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "txBytes")]
    pub tx_bytes: String, // base64
    #[serde(rename = "txId")]
    pub tx_id: String,
    #[serde(rename = "fragments")]
    pub fragments: Vec<FragmentFFI>,
    pub priority: PriorityFFI,
}

/// Fragment for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentFFI {
    #[serde(rename = "transactionId")]
    pub transaction_id: String, // hex
    #[serde(rename = "fragmentIndex")]
    pub fragment_index: u16,
    #[serde(rename = "totalFragments")]
    pub total_fragments: u16,
    #[serde(rename = "dataBase64")]
    pub data_base64: String,
}

/// Request to add to retry queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddToRetryRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "txBytes")]
    pub tx_bytes: String, // base64
    #[serde(rename = "txId")]
    pub tx_id: String,
    pub error: String,
}

/// Request to queue confirmation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfirmationRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "txId")]
    pub tx_id: String, // hex
    pub signature: String,
}

/// Simple success response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResponse {
    pub success: bool,
}

/// Queue size response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSizeResponse {
    #[serde(rename = "queueSize")]
    pub queue_size: usize,
}

