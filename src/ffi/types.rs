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
    Ok {
        ok: bool,
        data: T,
    },
    Err {
        ok: bool,
        code: String,
        message: String,
    },
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
// Fragmentation types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fragment {
    pub id: String,
    pub index: u32,
    pub total: u32,
    pub data: String,          // base64
    pub fragment_type: String, // "FragmentStart" | "FragmentContinue" | "FragmentEnd"
    pub checksum: String,      // base64
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
    /// AES-256-GCM encryption key for nonce bundle storage (any string; hashed with SHA-256 internally).
    /// Required when `storageDirectory` is set. Falls back to the `POLLINET_ENCRYPTION_KEY`
    /// environment variable when absent (useful for CLI/server usage).
    #[serde(rename = "encryptionKey", default)]
    pub encryption_key: Option<String>,
    /// Base58-encoded Solana wallet address that owns this node session.
    /// When provided it is stored on the transport and will be used to attribute
    /// uptime, relay and submission rewards to the correct wallet.
    /// Omitting this field (or passing null) is valid — the node still participates
    /// in the mesh but rewards cannot be allocated until a wallet is associated.
    #[serde(rename = "walletAddress", default)]
    pub wallet_address: Option<String>,
}

// SubmitIntentRequest / SubmitIntentResponse live in crate::submission — see src/submission/mod.rs

pub(crate) fn default_version() -> u32 {
    1
}

fn default_enable_logging() -> bool {
    true
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

/// Request to accept and queue external pre-signed transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptExternalTransactionRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(rename = "base64SignedTx")]
    pub base64_signed_tx: String,
    #[serde(rename = "maxPayload")]
    pub max_payload: Option<usize>,
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

// =============================================================================
// Intent protocol types
// =============================================================================

/// One token approval entry inside [CreateApproveTransactionRequest].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenApprovalRequest {
    pub mint_address: String,
    pub amount: u64,
    pub decimals: u8,
    /// Owner's token account for this mint.
    pub token_account: String,
    /// "spl-token" (default) or "token-2022".
    #[serde(default = "default_spl_token")]
    pub token_program: String,
}

fn default_spl_token() -> String {
    "spl-token".to_string()
}

/// Builds a batch `approve_checked` transaction (one instruction per token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApproveTransactionRequest {
    /// Wallet that owns the token accounts and will sign the transaction.
    pub owner_wallet: String,
    /// Fee payer (may equal owner_wallet).
    pub fee_payer: String,
    /// Recent blockhash (base58).
    pub recent_blockhash: String,
    /// One entry per token account to approve.
    pub tokens: Vec<TokenApprovalRequest>,
}

/// Response for [CreateApproveTransactionRequest]: base64-encoded unsigned transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveTransactionResponse {
    /// Base64-encoded unsigned transaction containing all approve instructions.
    pub transaction: String,
    /// Executor PDA that was granted delegate authority.
    pub executor_pda: String,
}

/// Builds the canonical 169-byte borsh Intent and returns it as base64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateIntentBytesRequest {
    pub from: String,
    pub to: String,
    pub token_mint: String,
    pub amount: u64,
    pub expires_at: i64,
    pub gas_fee_amount: u64,
    pub gas_fee_payee: String,
    /// 16-byte nonce as lowercase hex (32 chars). Random if omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce_hex: Option<String>,
}

/// Response for [CreateIntentBytesRequest].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentBytesResponse {
    /// Base64-encoded 169-byte intent (ready to sign with Ed25519).
    pub intent_bytes: String,
    /// The nonce used (hex, 32 lowercase chars) — store this for deduplication.
    pub nonce_hex: String,
}

/// Response for the executor PDA query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorPdaResponse {
    pub pda: String,
    pub bump: u8,
}

/// Revokes executor PDA delegate authority from a list of token accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRevokeTransactionRequest {
    pub owner_wallet: String,
    pub fee_payer: String,
    pub recent_blockhash: String,
    pub token_accounts: Vec<String>,
    #[serde(default = "default_spl_token")]
    pub token_program: String,
}

/// Response for [CreateRevokeTransactionRequest].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeTransactionResponse {
    /// Base64-encoded unsigned transaction; sign with owner_wallet before submitting.
    pub transaction: String,
}
