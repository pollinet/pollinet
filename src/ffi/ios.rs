//! iOS C FFI interface
//! 
//! This module provides a C-compatible interface for Swift to interact with
//! the PolliNet Rust core. It handles:
//! - Host-driven BLE transport (push_inbound, next_outbound, tick)
//! - Transaction building and fragmentation
//! - Signature operations
//! - Metrics and diagnostics
//!
//! Unlike Android's JNI interface, iOS uses standard C FFI with manual memory management.
//! All string returns must be freed by the caller using `pollinet_free_string()`.

#[cfg(feature = "ios")]
use std::ffi::{CStr, CString};
#[cfg(feature = "ios")]
use std::os::raw::{c_char, c_int};
#[cfg(feature = "ios")]
use std::ptr;
#[cfg(feature = "ios")]
use std::sync::Arc;
#[cfg(feature = "ios")]
use parking_lot::Mutex;
#[cfg(feature = "ios")]
use std::str::FromStr;

use super::runtime;
use super::transport::HostBleTransport;
use super::types::*;

#[cfg(feature = "ios")]
use solana_sdk::pubkey::Pubkey;

// Global state for transport instances (same pattern as Android)
#[cfg(feature = "ios")]
lazy_static::lazy_static! {
    static ref TRANSPORTS: Arc<Mutex<Vec<Arc<HostBleTransport>>>> = Arc::new(Mutex::new(Vec::new()));
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Get transport instance by handle
#[cfg(feature = "ios")]
fn get_transport(handle: i64) -> Result<Arc<HostBleTransport>, String> {
    let transports = TRANSPORTS.lock();
    if handle < 0 || handle as usize >= transports.len() {
        return Err(format!("Invalid handle: {}", handle));
    }
    Ok(transports[handle as usize].clone())
}

/// Create a C string result from a Rust Result<String, String>
/// Returns a pointer to a heap-allocated C string (caller must free with pollinet_free_string)
#[cfg(feature = "ios")]
fn create_result_string(result: Result<String, String>) -> *mut c_char {
    let json_string = match result {
        Ok(json) => json,
        Err(e) => {
            let error_response: FfiResult<()> = FfiResult::error("ERR_INTERNAL", e);
            serde_json::to_string(&error_response)
                .unwrap_or_else(|_| r#"{"ok":false,"code":"ERR_FATAL","message":"Serialization failed"}"#.to_string())
        }
    };
    
    // Convert to CString and leak it (caller will free)
    match CString::new(json_string) {
        Ok(c_string) => c_string.into_raw(),
        Err(_) => {
            // Fallback error string if conversion fails
            CString::new(r#"{"ok":false,"code":"ERR_FATAL","message":"String conversion failed"}"#)
                .unwrap()
                .into_raw()
        }
    }
}

/// Parse log level string to tracing level
#[cfg(feature = "ios")]
fn parse_log_level(level: Option<&str>) -> tracing::Level {
    match level {
        Some("trace") => tracing::Level::TRACE,
        Some("debug") => tracing::Level::DEBUG,
        Some("info") => tracing::Level::INFO,
        Some("warn") => tracing::Level::WARN,
        Some("error") => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

/// Convert a C string pointer to a Rust String
#[cfg(feature = "ios")]
unsafe fn c_str_to_string(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("Null pointer".to_string());
    }
    let c_str = CStr::from_ptr(ptr);
    c_str.to_str()
        .map(|s| s.to_string())
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Convert a C byte array to a Rust Vec<u8>
#[cfg(feature = "ios")]
unsafe fn c_bytes_to_vec(ptr: *const u8, len: usize) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(ptr, len).to_vec()
}

// =============================================================================
// Memory Management
// =============================================================================

/// Free a string allocated by the FFI interface
/// This must be called for all strings returned by pollinet_* functions
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

// =============================================================================
// Initialization and Lifecycle
// =============================================================================

/// Initialize the PolliNet SDK
/// 
/// # Arguments
/// * `config_json` - Pointer to JSON config bytes
/// * `config_len` - Length of config bytes
/// 
/// # Returns
/// Handle (i64) to the initialized transport instance, or -1 on error
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_init(
    config_json: *const u8,
    config_len: usize,
) -> i64 {
    // Initialize logging for iOS (using tracing subscriber, no android_logger)
    tracing::info!("📱 iOS FFI init called");
    
    let result: Result<i64, String> = (|| {
        tracing::info!("Step 1: Initializing runtime...");
        
        // Initialize runtime if needed
        match runtime::init_runtime() {
            Ok(_) => {
                tracing::info!("✅ Runtime initialized successfully");
            },
            Err(e) if e.contains("already initialized") => {
                tracing::info!("ℹ️  Runtime already initialized");
            },
            Err(e) => {
                tracing::error!("❌ Runtime init failed: {}", e);
                return Err(format!("Failed to initialize runtime: {}", e));
            }
        }

        tracing::info!("Step 2: Parsing config...");
        
        // Parse config from C byte array
        let config_data = unsafe { c_bytes_to_vec(config_json, config_len) };
        if config_data.is_empty() {
            return Err("Empty config data".to_string());
        }

        tracing::info!("Step 3: Deserializing config ({} bytes)...", config_data.len());
        
        let config: SdkConfig = serde_json::from_slice(&config_data)
            .map_err(|e| {
                tracing::error!("❌ Failed to parse config: {}", e);
                format!("Failed to parse config: {}", e)
            })?;

        tracing::info!("Step 4: Config parsed - RPC: {:?}, logging: {}", config.rpc_url, config.enable_logging);

        // Initialize logging if requested
        if config.enable_logging {
            let _ = tracing_subscriber::fmt()
                .with_max_level(parse_log_level(config.log_level.as_deref()))
                .try_init();
            tracing::info!("✅ Tracing subscriber initialized");
        }

        tracing::info!("Step 5: Creating transport...");
        
        // Create transport instance
        let mut transport = runtime::block_on(async {
            if let Some(rpc_url) = &config.rpc_url {
                tracing::info!("Creating transport with RPC: {}", rpc_url);
                HostBleTransport::new_with_rpc(rpc_url).await
            } else {
                tracing::info!("Creating transport without RPC");
                HostBleTransport::new().await
            }
        }).map_err(|e| {
            tracing::error!("❌ Transport creation failed: {}", e);
            e
        })?;

        // Set secure storage if directory provided
        if let Some(storage_dir) = &config.storage_directory {
            tracing::info!("Step 5b: Setting up secure storage at: {}", storage_dir);
            transport.set_secure_storage(storage_dir)
                .map_err(|e| {
                    tracing::error!("❌ Failed to set secure storage: {}", e);
                    e
                })?;
            tracing::info!("✅ Secure storage configured");
            
            // Set queue storage directory (queues will persist to subdirectory)
            let queue_storage_dir = format!("{}/queues", storage_dir);
            std::env::set_var("POLLINET_QUEUE_STORAGE", &queue_storage_dir);
            tracing::info!("✅ Queue persistence enabled at: {}", queue_storage_dir);
        } else {
            tracing::info!("ℹ️  No storage directory provided - bundle persistence disabled");
        }

        tracing::info!("Step 6: Storing transport...");
        
        let transport_arc = Arc::new(transport);
        let mut transports = TRANSPORTS.lock();
        transports.push(transport_arc);
        let handle = (transports.len() - 1) as i64;

        tracing::info!("✅ PolliNet SDK initialized successfully with handle {}", handle);
        Ok(handle)
    })();

    match result {
        Ok(handle) => {
            tracing::info!("🎉 Returning handle {} to Swift", handle);
            handle
        },
        Err(e) => {
            tracing::error!("💥 SDK initialization failed: {}", e);
            tracing::error!("Returning error handle -1");
            -1 // Error handle
        }
    }
}

/// Get SDK version
/// Returns a pointer to a heap-allocated version string (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_version() -> *mut c_char {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    // Allocate on heap - caller must free
    match CString::new(VERSION) {
        Ok(c_string) => c_string.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Shutdown the SDK and release resources
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_shutdown(handle: i64) {
    let transports = TRANSPORTS.lock();
    if handle >= 0 && (handle as usize) < transports.len() {
        // Just mark as None; we'll keep the Vec stable for other handles
        tracing::info!("🛑 Shutting down SDK handle {}", handle);
    }
}

// =============================================================================
// Host-driven Transport API
// =============================================================================

/// Push inbound data from GATT characteristic
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `data` - Pointer to byte array
/// * `data_len` - Length of byte array
/// 
/// # Returns
/// JSON result string (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_push_inbound(
    handle: i64,
    data: *const u8,
    data_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let data_vec = unsafe { c_bytes_to_vec(data, data_len) };

        transport.push_inbound(data_vec)?;
        
        let response: FfiResult<()> = FfiResult::success(());
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Get next outbound frame to send
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `max_len` - Maximum length of data to return
/// * `out_data` - Output buffer (must be at least max_len bytes)
/// * `out_len` - Output parameter for actual data length
/// 
/// # Returns
/// 1 if data available and written to out_data, 0 if empty
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_next_outbound(
    handle: i64,
    max_len: usize,
    out_data: *mut u8,
    out_len: *mut usize,
) -> c_int {
    let result: Result<Option<Vec<u8>>, String> = (|| {
        let transport = get_transport(handle)?;
        Ok(transport.next_outbound(max_len))
    })();

    match result {
        Ok(Some(data)) => {
            if data.len() > max_len {
                tracing::error!("Data too large: {} > {}", data.len(), max_len);
                unsafe {
                    *out_len = 0;
                }
                return 0;
            }
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), out_data, data.len());
                *out_len = data.len();
            }
            1
        }
        Ok(None) => {
            unsafe {
                *out_len = 0;
            }
            0
        }
        Err(e) => {
            tracing::error!("nextOutbound error: {}", e);
            unsafe {
                *out_len = 0;
            }
            0
        }
    }
}

/// Periodic tick for retry/timeout handling
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `now_ms` - Current timestamp in milliseconds
/// 
/// # Returns
/// JSON result string containing array of base64-encoded frames (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_tick(
    handle: i64,
    now_ms: i64,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let frames = transport.tick(now_ms as u64);
        
        // Encode frames as JSON array of base64 strings
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        let encoded: Vec<String> = frames.iter().map(|f| BASE64.encode(f)).collect();
        
        let response: FfiResult<Vec<String>> = FfiResult::success(encoded);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Get current metrics
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing metrics snapshot (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_metrics(
    handle: i64,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let metrics = transport.metrics();
        
        let response: FfiResult<MetricsSnapshot> = FfiResult::success(metrics);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Clear transaction from buffers
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `tx_id` - Transaction ID to clear (C string, null-terminated)
/// 
/// # Returns
/// JSON result string (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_clear_transaction(
    handle: i64,
    tx_id: *const c_char,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let tx_id_str = unsafe { c_str_to_string(tx_id)? };

        transport.clear_transaction(&tx_id_str);
        
        let response: FfiResult<()> = FfiResult::success(());
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Transaction Builders
// =============================================================================

/// Create unsigned SOL transfer transaction
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_create_unsigned_transaction(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: CreateUnsignedTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Convert optional nonce data from FFI type to transaction type
        let nonce_data_opt = request.nonce_data.as_ref().map(|ffi_nonce| {
            crate::transaction::CachedNonceData {
                nonce_account: ffi_nonce.nonce_account.clone(),
                authority: ffi_nonce.authority.clone(),
                blockhash: ffi_nonce.blockhash.clone(),
                lamports_per_signature: ffi_nonce.lamports_per_signature,
                cached_at: ffi_nonce.cached_at,
                used: ffi_nonce.used,
            }
        });

        // Build unsigned transaction
        let base64_tx = runtime::block_on(async {
            transport
                .transaction_service()
                .create_unsigned_transaction(
                    &request.sender,
                    &request.recipient,
                    &request.fee_payer,
                    request.amount,
                    request.nonce_account.as_deref(),
                    nonce_data_opt.as_ref(),
                )
                .await
        })
        .map_err(|e| format!("Failed to create transaction: {}", e))?;
        
        tracing::info!("✅ Created unsigned transaction (base64 length: {})", base64_tx.len());
        
        let response: FfiResult<String> = FfiResult::success(base64_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Create unsigned SPL token transfer transaction
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_create_unsigned_spl_transaction(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: CreateUnsignedSplTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Convert optional nonce data from FFI type to transaction type
        let nonce_data_opt = request.nonce_data.as_ref().map(|ffi_nonce| {
            crate::transaction::CachedNonceData {
                nonce_account: ffi_nonce.nonce_account.clone(),
                authority: ffi_nonce.authority.clone(),
                blockhash: ffi_nonce.blockhash.clone(),
                lamports_per_signature: ffi_nonce.lamports_per_signature,
                cached_at: ffi_nonce.cached_at,
                used: ffi_nonce.used,
            }
        });

        // Build unsigned SPL transaction
        let base64_tx = runtime::block_on(async {
            transport
                .transaction_service()
                .create_unsigned_spl_transaction(
                    &request.sender_wallet,
                    &request.recipient_wallet,
                    &request.fee_payer,
                    &request.mint_address,
                    request.amount,
                    request.nonce_account.as_deref(),
                    nonce_data_opt.as_ref(),
                )
                .await
        })
        .map_err(|e| format!("Failed to create SPL transaction: {}", e))?;
        
        tracing::info!("✅ Created unsigned SPL transaction (base64 length: {})", base64_tx.len());
        
        let response: FfiResult<String> = FfiResult::success(base64_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Create unsigned governance vote transaction with durable nonce
/// Returns base64-encoded unsigned transaction (MWA-compatible)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_cast_unsigned_vote(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: CastUnsignedVoteRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("🗳️ Creating unsigned governance vote transaction");
        tracing::info!("   Voter: {}", request.voter);
        tracing::info!("   Proposal: {}", request.proposal_id);
        tracing::info!("   Vote account: {}", request.vote_account);
        tracing::info!("   Choice: {}", request.choice);
        if let Some(ref nonce_data) = request.nonce_data {
            tracing::info!("   Using cached nonce data (no RPC call)");
        } else if let Some(ref nonce_account) = request.nonce_account {
            tracing::info!("   Fetching nonce data from blockchain: {}", nonce_account);
        }

        // Build unsigned vote transaction (uses cached nonce data if provided, otherwise fetches from RPC)
        // Convert optional nonce data from FFI type to transaction type
        let nonce_data_opt = request.nonce_data.as_ref().map(|ffi_nonce| {
            crate::transaction::CachedNonceData {
                nonce_account: ffi_nonce.nonce_account.clone(),
                authority: ffi_nonce.authority.clone(),
                blockhash: ffi_nonce.blockhash.clone(),
                lamports_per_signature: ffi_nonce.lamports_per_signature,
                cached_at: ffi_nonce.cached_at,
                used: ffi_nonce.used,
            }
        });

        let base64_tx = runtime::block_on(async {
            transport
                .transaction_service()
                .cast_unsigned_vote(
                    &request.voter,
                    &request.proposal_id,
                    &request.vote_account,
                    request.choice,
                    &request.fee_payer,
                    request.nonce_account.as_deref(),
                    nonce_data_opt.as_ref(),
                )
                .await
        })
        .map_err(|e| format!("Failed to create unsigned vote transaction: {}", e))?;

        tracing::info!(
            "✅ Created unsigned vote transaction (base64 length: {})",
            base64_tx.len()
        );

        let response: FfiResult<String> = FfiResult::success(base64_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}
// =============================================================================
// Signature Operations
// =============================================================================

/// Prepare sign payload - Extract message bytes that need to be signed
/// 
/// # Arguments
/// * `base64_tx` - Base64-encoded transaction (C string, null-terminated)
/// * `out_payload` - Output buffer for payload bytes (must be large enough)
/// * `out_len` - Output parameter for actual payload length
/// 
/// # Returns
/// 1 if successful and payload written to out_payload, 0 on error
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_prepare_sign_payload(
    base64_tx: *const c_char,
    out_payload: *mut u8,
    out_len: *mut usize,
) -> c_int {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    
    let result: Result<Vec<u8>, String> = (|| {
        let tx_str = unsafe { c_str_to_string(base64_tx)? };

        // Decode from base64
        let tx_bytes = BASE64
            .decode(&tx_str)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        // Deserialize transaction
        let tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&tx_bytes)
            .map_err(|e| format!("Failed to deserialize transaction: {}", e))?;

        // Serialize the message (this is what needs to be signed)
        let message_bytes = bincode1::serialize(&tx.message)
            .map_err(|e| format!("Failed to serialize message: {}", e))?;

        tracing::info!("✅ Prepared sign payload: {} bytes", message_bytes.len());
        Ok(message_bytes)
    })();

    match result {
        Ok(payload) => {
            // Check if output buffer is large enough
            let payload_len = payload.len();
            unsafe {
                if out_payload.is_null() || out_len.is_null() {
                    tracing::error!("prepare_sign_payload: null output parameters");
                    return 0;
                }
                // Assume caller provides enough space (they should check returned length)
                ptr::copy_nonoverlapping(payload.as_ptr(), out_payload, payload_len);
                *out_len = payload_len;
            }
            1
        }
        Err(e) => {
            tracing::error!("prepare_sign_payload error: {}", e);
            unsafe {
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            0
        }
    }
}

/// Apply signature to unsigned transaction
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `base64_tx` - Base64-encoded unsigned transaction (C string, null-terminated)
/// * `signer_pubkey` - Signer public key (C string, null-terminated)
/// * `signature_bytes` - Signature bytes (64 bytes)
/// * `signature_len` - Length of signature bytes (must be 64)
/// 
/// # Returns
/// JSON result string containing signed transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_apply_signature(
    handle: i64,
    base64_tx: *const c_char,
    signer_pubkey: *const c_char,
    signature_bytes: *const u8,
    signature_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        
        let tx_str = unsafe { c_str_to_string(base64_tx)? };
        let pubkey_str = unsafe { c_str_to_string(signer_pubkey)? };
        let sig_bytes = unsafe { c_bytes_to_vec(signature_bytes, signature_len) };

        // Parse pubkey
        let pubkey = Pubkey::from_str(&pubkey_str)
            .map_err(|e| format!("Invalid signer pubkey: {}", e))?;

        // Convert signature bytes to Solana signature
        if sig_bytes.len() != 64 {
            return Err(format!("Invalid signature length: expected 64, got {}", sig_bytes.len()));
        }
        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);
        let signature = solana_sdk::signature::Signature::from(sig_array);

        // Apply signature
        let updated_tx = transport
            .transaction_service()
            .add_signature(&tx_str, &pubkey, &signature)
            .map_err(|e| format!("Failed to apply signature: {}", e))?;

        tracing::info!("✅ Applied signature for {}", pubkey_str);
        
        let response: FfiResult<String> = FfiResult::success(updated_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Verify and serialize transaction for submission
/// 
/// # Arguments
/// * `base64_tx` - Base64-encoded transaction (C string, null-terminated)
/// 
/// # Returns
/// JSON result string containing serialized transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_verify_and_serialize(
    base64_tx: *const c_char,
) -> *mut c_char {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    
    let result = (|| {
        let tx_str = unsafe { c_str_to_string(base64_tx)? };

        // Decode from base64
        let tx_bytes = BASE64
            .decode(&tx_str)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        // Deserialize transaction
        let tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&tx_bytes)
            .map_err(|e| format!("Failed to deserialize transaction: {}", e))?;

        // Verify transaction has valid signatures
        let valid_sigs = tx
            .signatures
            .iter()
            .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
            .count();

        if valid_sigs == 0 {
            return Err("Transaction has no valid signatures".to_string());
        }

        tracing::info!("✅ Transaction verified: {}/{} valid signatures", valid_sigs, tx.signatures.len());

        // Serialize for submission (bincode1 format)
        let wire_tx = bincode1::serialize(&tx)
            .map_err(|e| format!("Failed to serialize transaction: {}", e))?;

        // Return as base64 for consistency
        let wire_tx_base64 = BASE64.encode(&wire_tx);
        
        tracing::info!("Transaction ready for submission: {} bytes", wire_tx.len());
        
        let response: FfiResult<String> = FfiResult::success(wire_tx_base64);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Fragmentation API
// =============================================================================

/// Fragment a transaction for BLE transmission
/// 
/// Optionally accepts max_payload (MTU - 10) for MTU-aware fragmentation
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `tx_bytes` - Transaction bytes
/// * `tx_bytes_len` - Length of transaction bytes
/// * `max_payload` - Maximum payload size (0 = use default)
/// 
/// # Returns
/// JSON result string containing fragment list (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_fragment(
    handle: i64,
    tx_bytes: *const u8,
    tx_bytes_len: usize,
    max_payload: i64,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let tx_data = unsafe { c_bytes_to_vec(tx_bytes, tx_bytes_len) };
        
        let max_payload_opt = if max_payload > 0 {
            Some(max_payload as usize)
        } else {
            None
        };

        let fragments = transport.queue_transaction(tx_data, max_payload_opt)?;
        
        let fragment_list = FragmentList { fragments };
        let response: FfiResult<FragmentList> = FfiResult::success(fragment_list);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Offline Bundle Management
// =============================================================================

/// Prepare offline bundle with cached nonce accounts
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing bundle (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_prepare_offline_bundle(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: PrepareOfflineBundleRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Parse sender keypair from base64
        let keypair_bytes = base64::decode(&request.sender_keypair_base64)
            .map_err(|e| format!("Invalid keypair base64: {}", e))?;
        let sender_keypair = solana_sdk::signature::Keypair::from_bytes(&keypair_bytes)
            .map_err(|e| format!("Invalid keypair bytes: {}", e))?;

        tracing::info!("📦 Preparing offline bundle for {} transactions", request.count);

        // Use secure storage if available
        let bundle = if let Some(storage) = transport.secure_storage() {
            tracing::info!("🔒 Using secure storage for bundle persistence");
            
            // Load existing bundle if it exists
            let existing_bundle = storage.load_bundle()
                .map_err(|e| format!("Failed to load existing bundle: {}", e))?;
            
            if let Some(ref existing) = existing_bundle {
                tracing::info!("📂 Found existing bundle with {} nonces (available: {}, used: {})", 
                    existing.nonce_caches.len(), existing.available_nonces(), existing.used_nonces());
            } else {
                tracing::info!("📂 No existing bundle found - will create new one");
            }
            
            // Save to temp file so prepare_offline_bundle can load it
            let temp_path = std::env::temp_dir().join("pollinet_temp_bundle.json");
            if let Some(existing) = &existing_bundle {
                existing.save_to_file(temp_path.to_str().unwrap())
                    .map_err(|e| format!("Failed to save temp bundle: {}", e))?;
                tracing::info!("💾 Saved existing bundle to temp file for processing");
            }
            
            // Prepare bundle (will refresh used nonces or create new ones)
            let bundle = runtime::block_on(async {
                transport
                    .transaction_service()
                    .prepare_offline_bundle(
                        request.count,
                        &sender_keypair,
                        if existing_bundle.is_some() {
                            temp_path.to_str()
                        } else {
                            None
                        },
                    )
                    .await
            })
            .map_err(|e| format!("Failed to prepare bundle: {}", e))?;
            
            // Clean up temp file
            if temp_path.exists() {
                let _ = std::fs::remove_file(&temp_path);
            }
            
            // Save updated bundle to secure storage
            storage.save_bundle(&bundle)
                .map_err(|e| format!("Failed to save bundle: {}", e))?;
            
            tracing::info!("💾 Bundle saved to secure storage");
            tracing::info!("   Total nonces: {}, Available: {}, Used: {}", 
                bundle.nonce_caches.len(), bundle.available_nonces(), bundle.used_nonces());
            bundle
        } else {
            tracing::warn!("⚠️  No secure storage configured - bundle will not persist");
            
            // Fallback to traditional file-based approach
            runtime::block_on(async {
                transport
                    .transaction_service()
                    .prepare_offline_bundle(
                        request.count,
                        &sender_keypair,
                        request.bundle_file.as_deref(),
                    )
                    .await
            })
            .map_err(|e| format!("Failed to prepare bundle: {}", e))?
        };

        tracing::info!("✅ Bundle prepared with {} total nonces ({} available)", 
            bundle.nonce_caches.len(), bundle.available_nonces());

        // Convert to FFI bundle type (with proper camelCase serialization)
        let ffi_bundle = crate::ffi::types::OfflineTransactionBundle::from_transaction_bundle(&bundle);
        
        // Serialize bundle to JSON
        let bundle_json = serde_json::to_string(&ffi_bundle)
            .map_err(|e| format!("Failed to serialize bundle: {}", e))?;

        let response: FfiResult<String> = FfiResult::success(bundle_json);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Create offline transaction using cached nonce data
/// NO internet required - core PolliNet offline feature
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_create_offline_transaction(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: CreateOfflineTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Parse keypairs
        let sender_bytes = base64::decode(&request.sender_keypair_base64)
            .map_err(|e| format!("Invalid sender keypair: {}", e))?;
        let sender_keypair = solana_sdk::signature::Keypair::from_bytes(&sender_bytes)
            .map_err(|e| format!("Invalid sender keypair bytes: {}", e))?;

        let authority_bytes = base64::decode(&request.nonce_authority_keypair_base64)
            .map_err(|e| format!("Invalid authority keypair: {}", e))?;
        let authority_keypair = solana_sdk::signature::Keypair::from_bytes(&authority_bytes)
            .map_err(|e| format!("Invalid authority keypair bytes: {}", e))?;

        tracing::info!("📴 Creating OFFLINE transaction (no internet required)");

        // Load bundle from secure storage
        let storage = transport.secure_storage()
            .ok_or_else(|| "Secure storage not configured".to_string())?;
        
        let mut bundle = storage.load_bundle()
            .map_err(|e| format!("Failed to load bundle: {}", e))?
            .ok_or_else(|| "No bundle found - call prepareOfflineBundle first".to_string())?;
        
        tracing::info!("📂 Loaded bundle: {} total nonces, {} available", 
            bundle.nonce_caches.len(), bundle.available_nonces());
        
        // Find first available (unused) nonce
        let nonce_to_use = bundle.nonce_caches.iter_mut()
            .find(|n| !n.used)
            .ok_or_else(|| "No available nonces - all have been used. Call prepareOfflineBundle to refresh.".to_string())?;
        
        tracing::info!("📌 Using nonce account: {}", nonce_to_use.nonce_account);
        tracing::info!("   Blockhash: {}", nonce_to_use.blockhash);
        
        // Clone the nonce data before marking as used (for transaction creation)
        let cached_nonce = nonce_to_use.clone();
        
        // Mark nonce as used BEFORE creating transaction
        nonce_to_use.used = true;
        tracing::info!("✅ Marked nonce as used");
        
        // Save updated bundle immediately
        storage.save_bundle(&bundle)
            .map_err(|e| format!("Failed to save bundle: {}", e))?;
        tracing::info!("💾 Bundle saved with updated nonce status");
        tracing::info!("   Available nonces remaining: {}", bundle.available_nonces());

        // Create transaction offline using the selected nonce
        let compressed_tx = transport
            .transaction_service()
            .create_offline_transaction(
                &sender_keypair,
                &request.recipient,
                request.amount,
                &authority_keypair,
                &cached_nonce,
            )
            .map_err(|e| format!("Failed to create offline transaction: {}", e))?;

        tracing::info!("✅ Offline transaction created: {} bytes", compressed_tx.len());

        // Encode to base64
        let tx_base64 = base64::encode(&compressed_tx);

        let response: FfiResult<String> = FfiResult::success(tx_base64);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Submit offline-created transaction to blockchain
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing transaction signature (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_submit_offline_transaction(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: SubmitOfflineTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Decode transaction from base64
        let tx_bytes = base64::decode(&request.transaction_base64)
            .map_err(|e| format!("Invalid transaction base64: {}", e))?;

        tracing::info!("Submitting offline transaction to blockchain");

        // Submit transaction
        let signature = runtime::block_on(async {
            transport
                .transaction_service()
                .submit_offline_transaction(&tx_bytes, request.verify_nonce)
                .await
        })
        .map_err(|e| format!("Failed to submit transaction: {}", e))?;

        tracing::info!("✅ Transaction submitted: {}", signature);

        let response: FfiResult<String> = FfiResult::success(signature);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}
/// Create UNSIGNED offline transaction for MWA signing
/// Takes PUBLIC KEYS only (no private keys) - MWA-compatible
/// Returns unsigned transaction that MWA/Seed Vault can sign
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded unsigned transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_create_unsigned_offline_transaction(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: CreateUnsignedOfflineTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("🔓 Creating UNSIGNED offline transaction for MWA");
        tracing::info!("   Sender pubkey: {}", request.sender_pubkey);
        tracing::info!("   NO private keys involved - MWA will sign");

        // Load bundle from secure storage
        let storage = transport.secure_storage()
            .ok_or_else(|| "Secure storage not configured".to_string())?;
        
        let mut bundle = storage.load_bundle()
            .map_err(|e| format!("Failed to load bundle: {}", e))?
            .ok_or_else(|| "No bundle found - call prepareOfflineBundle first".to_string())?;
        
        tracing::info!("📂 Loaded bundle: {} total nonces, {} available", 
            bundle.nonce_caches.len(), bundle.available_nonces());
        
        // Find first available (unused) nonce
        let nonce_to_use = bundle.nonce_caches.iter_mut()
            .find(|n| !n.used)
            .ok_or_else(|| "No available nonces - all have been used. Call prepareOfflineBundle to refresh.".to_string())?;
        
        tracing::info!("📌 Using nonce account: {}", nonce_to_use.nonce_account);
        
        // Clone the nonce data before marking as used
        let cached_nonce = nonce_to_use.clone();
        
        // Mark nonce as used BEFORE creating transaction
        nonce_to_use.used = true;
        tracing::info!("✅ Marked nonce as used");
        
        // Save updated bundle immediately
        storage.save_bundle(&bundle)
            .map_err(|e| format!("Failed to save bundle: {}", e))?;
        tracing::info!("💾 Bundle saved with updated nonce status");
        tracing::info!("   Available nonces remaining: {}", bundle.available_nonces());

        // Create UNSIGNED transaction (always uses bundle nonce, matching Android implementation)
        let unsigned_tx = transport
            .transaction_service()
            .create_unsigned_offline_transaction(
                &request.sender_pubkey,
                &request.recipient,
                request.amount,
                &request.nonce_authority_pubkey,
                &cached_nonce,
            )
            .map_err(|e| format!("Failed to create unsigned transaction: {}", e))?;

        tracing::info!("✅ Unsigned transaction created for MWA signing");
        tracing::info!("   Transaction ready for Seed Vault signature");

        let response: FfiResult<String> = FfiResult::success(unsigned_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Create UNSIGNED offline SPL token transfer for MWA/Seed Vault signing
/// Uses cached nonce data from the offline bundle (no RPC required).
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded unsigned transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_create_unsigned_offline_spl_transaction(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;

        // Parse request
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: CreateUnsignedOfflineSplTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("🔓 Creating UNSIGNED offline SPL transaction for MWA");
        tracing::info!("   Sender wallet: {}", request.sender_wallet);
        tracing::info!("   Recipient wallet: {}", request.recipient_wallet);

        // Get nonce data: use provided cached data, or get from bundle
        let cached_nonce = if let Some(ref ffi_nonce) = request.nonce_data {
            // Use provided nonce data
            tracing::info!("Using provided cached nonce data");
            crate::transaction::CachedNonceData {
                nonce_account: ffi_nonce.nonce_account.clone(),
                authority: ffi_nonce.authority.clone(),
                blockhash: ffi_nonce.blockhash.clone(),
                lamports_per_signature: ffi_nonce.lamports_per_signature,
                cached_at: ffi_nonce.cached_at,
                used: ffi_nonce.used,
            }
        } else {
            // Load bundle from secure storage and get available nonce
            let storage = transport
                .secure_storage()
                .ok_or_else(|| "Secure storage not configured".to_string())?;

            let mut bundle = storage
                .load_bundle()
                .map_err(|e| format!("Failed to load bundle: {}", e))?
                .ok_or_else(|| "No bundle found - call prepareOfflineBundle first".to_string())?;

            tracing::info!(
                "📂 Loaded bundle: {} total nonces, {} available",
                bundle.nonce_caches.len(),
                bundle.available_nonces()
            );

            // Find first available (unused) nonce
            let nonce_to_use = bundle
                .nonce_caches
                .iter_mut()
                .find(|n| !n.used)
                .ok_or_else(|| {
                    "No available nonces - all have been used. Call prepareOfflineBundle to refresh."
                        .to_string()
                })?;

            tracing::info!("📌 Using nonce account: {}", nonce_to_use.nonce_account);

            // Clone the nonce data
            let cached_nonce = nonce_to_use.clone();

            // Mark as used and save bundle
            nonce_to_use.used = true;
            storage
                .save_bundle(&bundle)
                .map_err(|e| format!("Failed to save bundle: {}", e))?;

            tracing::info!(
                "💾 Bundle saved (available nonces remaining: {})",
                bundle.available_nonces()
            );
            
            cached_nonce
        };

        // Create UNSIGNED offline SPL transaction
        let unsigned_tx = transport
            .transaction_service()
            .create_unsigned_offline_spl_transaction(
                &request.sender_wallet,
                &request.recipient_wallet,
                &request.fee_payer,
                &request.mint_address,
                request.amount,
                &cached_nonce,
            )
            .map_err(|e| format!("Failed to create unsigned offline SPL transaction: {}", e))?;

        tracing::info!("✅ Unsigned offline SPL transaction created for MWA signing");

        let response: FfiResult<String> = FfiResult::success(unsigned_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Get the message bytes that need to be signed by MWA
/// Extracts the raw message from unsigned transaction for MWA/Seed Vault
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded message bytes (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_transaction_message_to_sign(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: GetMessageToSignRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("📝 Extracting message to sign for MWA");

        // Get message bytes
        let message_bytes = transport
            .transaction_service()
            .get_transaction_message_to_sign(&request.unsigned_transaction_base64)
            .map_err(|e| format!("Failed to extract message: {}", e))?;

        // Encode to base64 for transport
        let message_base64 = base64::encode(&message_bytes);

        tracing::info!("✅ Message extracted: {} bytes", message_bytes.len());

        let response: FfiResult<String> = FfiResult::success(message_base64);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Get list of public keys that need to sign this transaction
/// Returns array of public key strings in signing order
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing array of signer pubkeys (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_required_signers(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        let request: GetRequiredSignersRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("👥 Getting required signers for transaction");

        // Get signers
        let signers = transport
            .transaction_service()
            .get_required_signers(&request.unsigned_transaction_base64)
            .map_err(|e| format!("Failed to get signers: {}", e))?;

        tracing::info!("✅ Found {} required signers", signers.len());

        let response: FfiResult<Vec<String>> = FfiResult::success(signers);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Nonce Management
// =============================================================================

/// Create unsigned nonce account creation transactions for MWA signing
/// 
/// This creates N nonce account creation transactions that can be signed by MWA.
/// Each transaction includes:
/// 1. Instructions to create a nonce account
/// 2. The ephemeral nonce keypair (to be signed locally before MWA)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing array of unsigned nonce transactions (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_create_unsigned_nonce_transactions(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("🎯 FFI createUnsignedNonceTransactions called with handle={}", handle);

        // Convert request
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        tracing::debug!("📥 Request data size: {} bytes", request_data.len());

        // Get transport
        let transport = get_transport(handle)?;

        // Parse request
        let request: CreateUnsignedNonceTransactionsRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("📋 Creating {} unsigned nonce transactions for payer: {}", 
            request.count, request.payer_pubkey);
        tracing::debug!("   Request version: {}", request.version);

        // Call the transaction service
        let transactions = runtime::block_on(async {
            transport
                .transaction_service()
                .create_unsigned_nonce_transactions(request.count, &request.payer_pubkey)
                .await
                .map_err(|e| format!("Failed to create nonce transactions: {}", e))
        })?;

        tracing::info!("✅ Created {} unsigned nonce transactions", transactions.len());
        
        // Log details about each transaction
        for (i, tx) in transactions.iter().enumerate() {
            tracing::debug!("   Transaction {}: {} nonce account(s), tx size: {} bytes", 
                i + 1, 
                tx.nonce_pubkey.len(),
                tx.unsigned_transaction_base64.len() * 3 / 4  // Approximate base64 decoded size
            );
            for (j, pubkey) in tx.nonce_pubkey.iter().enumerate() {
                tracing::debug!("     Nonce {}: {}", j + 1, pubkey);
            }
        }

        let response: FfiResult<Vec<UnsignedNonceTransaction>> = FfiResult::success(transactions);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Cache nonce data from existing on-chain nonce accounts
/// 
/// This fetches nonce data from the blockchain and adds it to secure storage.
/// Useful after creating nonce accounts via MWA - call this to cache the newly created nonces.
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing cached count (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_cache_nonce_accounts(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("🗄️  FFI cacheNonceAccounts called with handle={}", handle);

        // Convert request
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        // Get transport
        let transport = get_transport(handle)?;

        // Parse request
        let request: CacheNonceAccountsRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("Caching {} nonce accounts", request.nonce_accounts.len());

        // Fetch and save nonce data to secure storage
        let cached_count = runtime::block_on(async {
            if let Some(secure_storage) = transport.secure_storage() {
                use crate::transaction::OfflineTransactionBundle;
                
                // Load existing bundle or create new one
                let mut bundle = match secure_storage.load_bundle() {
                    Ok(Some(existing)) => existing,
                    Ok(None) | Err(_) => {
                        // Create new bundle if none exists
                        tracing::info!("Creating new bundle");
                        OfflineTransactionBundle::new()
                    }
                };

                let mut count = 0;
                // Fetch and add the new nonce data
                for nonce_account in &request.nonce_accounts {
                    match transport
                        .transaction_service()
                        .prepare_offline_nonce_data(nonce_account)
                        .await
                    {
                        Ok(cached_nonce) => {
                            bundle.add_nonce(cached_nonce);
                            count += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to fetch nonce data for {}: {}", nonce_account, e);
                        }
                    }
                }

                // Save the updated bundle
                secure_storage.save_bundle(&bundle)
                    .map_err(|e| format!("Failed to save bundle: {}", e))?;

                tracing::info!("✅ Saved bundle with {} new nonces to secure storage", count);
                Ok::<usize, String>(count)
            } else {
                Err("Secure storage not initialized".to_string())
            }
        })?;

        #[derive(serde::Serialize)]
        struct CacheResponse {
            #[serde(rename = "cachedCount")]
            cached_count: usize,
        }

        let response_data = CacheResponse { cached_count };
        let response: FfiResult<CacheResponse> = FfiResult::success(response_data);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Refresh all cached nonce data in the offline bundle
/// 
/// This:
/// - Loads the existing OfflineTransactionBundle from secure storage
/// - For each nonce account, fetches the latest on-chain nonce state
/// - Updates blockhash / fee data and marks all nonces as available (used = false)
/// - Saves the refreshed bundle back to secure storage
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing refreshed count (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_refresh_offline_bundle(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        tracing::info!("♻️  FFI refreshOfflineBundle called with handle={}", handle);

        let transport = get_transport(handle)?;

        let refreshed_count = runtime::block_on(async {
            if let Some(secure_storage) = transport.secure_storage() {
                // Load existing bundle
                let mut bundle = match secure_storage.load_bundle() {
                    Ok(Some(existing)) => existing,
                    Ok(None) => {
                        tracing::info!("📂 No existing bundle to refresh");
                        return Ok::<usize, String>(0);
                    }
                    Err(e) => {
                        return Err(format!("Failed to load bundle: {}", e));
                    }
                };

                if bundle.nonce_caches.is_empty() {
                    tracing::info!("📂 Bundle is empty - nothing to refresh");
                    return Ok(0);
                }

                tracing::info!(
                    "📂 Refreshing bundle: {} total nonces ({} available, {} used)",
                    bundle.total_nonces(),
                    bundle.available_nonces(),
                    bundle.used_nonces()
                );

                let mut refreshed = 0usize;

                for nonce in bundle.nonce_caches.iter_mut() {
                    let account = nonce.nonce_account.clone();
                    match transport
                        .transaction_service()
                        .prepare_offline_nonce_data(&account)
                        .await
                    {
                        Ok(fresh) => {
                            nonce.authority = fresh.authority;
                            nonce.blockhash = fresh.blockhash;
                            nonce.lamports_per_signature = fresh.lamports_per_signature;
                            nonce.cached_at = fresh.cached_at;
                            nonce.used = false; // Make available again
                            refreshed += 1;
                            tracing::info!("   ✅ Refreshed nonce {}", account);
                        }
                        Err(e) => {
                            tracing::warn!("   ⚠️  Failed to refresh nonce {}: {}", account, e);
                        }
                    }
                }

                // Save updated bundle
                secure_storage
                    .save_bundle(&bundle)
                    .map_err(|e| format!("Failed to save refreshed bundle: {}", e))?;

                tracing::info!(
                    "✅ Refreshed {} nonce accounts (bundle now has {} available)",
                    refreshed,
                    bundle.available_nonces()
                );

                Ok::<usize, String>(refreshed)
            } else {
                tracing::info!("ℹ️  Secure storage not initialized - cannot refresh bundle");
                Ok(0)
            }
        })?;

        #[derive(serde::Serialize)]
        struct RefreshResponse {
            #[serde(rename = "refreshedCount")]
            refreshed_count: usize,
        }

        let response_data = RefreshResponse { refreshed_count };
        let response: FfiResult<RefreshResponse> = FfiResult::success(response_data);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Get an available nonce account from cached bundle
/// 
/// Loads the bundle from secure storage and returns the first available
/// (unused) nonce account data. This allows users to either manage their
/// own nonce accounts or let PolliNet manage them automatically.
/// 
/// Returns None if:
/// - Secure storage not configured
/// - Bundle doesn't exist
/// - Bundle has no available nonces (all are used)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing CachedNonceData or null (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_available_nonce(
    handle: i64,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("🔍 FFI getAvailableNonce called with handle={}", handle);

        // Get transport
        let transport = get_transport(handle)?;

        // Get secure storage
        let storage = transport.secure_storage()
            .ok_or_else(|| "Secure storage not configured".to_string())?;

        // Load bundle from secure storage
        let bundle = storage.load_bundle()
            .map_err(|e| format!("Failed to load bundle: {}", e))?
            .ok_or_else(|| "No bundle found - call prepareOfflineBundle or cacheNonceAccounts first".to_string())?;

        tracing::info!("📂 Loaded bundle: {} total nonces, {} available", 
            bundle.nonce_caches.len(), bundle.available_nonces());

        // Get next available nonce
        let available_nonce = bundle.get_available_nonce();

        // Convert to FFI type and return as Option
        let ffi_nonce = available_nonce.map(|nonce| {
            tracing::info!("✅ Found available nonce account: {}", nonce.nonce_account);
            crate::ffi::types::CachedNonceData {
                version: 1,
                nonce_account: nonce.nonce_account.clone(),
                authority: nonce.authority.clone(),
                blockhash: nonce.blockhash.clone(),
                lamports_per_signature: nonce.lamports_per_signature,
                cached_at: nonce.cached_at,
                used: nonce.used,
            }
        });

        if ffi_nonce.is_none() {
            tracing::warn!("⚠️  No available nonces in bundle (all are used)");
        }

        let response: FfiResult<Option<crate::ffi::types::CachedNonceData>> = FfiResult::success(ffi_nonce);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Add nonce signature to a payer-signed transaction
/// This is called after MWA has added the payer signature (first signature)
/// to add the nonce keypair signature (second signature)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request bytes
/// * `request_len` - Length of request bytes
/// 
/// # Returns
/// JSON result string containing fully-signed transaction base64 (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_add_nonce_signature(
    handle: i64,
    request_json: *const u8,
    request_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("✍️  FFI addNonceSignature called with handle={}", handle);

        // Convert request
        let request_data = unsafe { c_bytes_to_vec(request_json, request_len) };

        tracing::debug!("📥 Request data size: {} bytes", request_data.len());

        // Parse request - use the type from types.rs
        let request: crate::ffi::types::AddNonceSignatureRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!("✍️  Adding {} nonce signature(s) to payer-signed transaction", 
            request.nonce_keypair_base64.len());
        tracing::debug!("   Request version: {}", request.version);
        tracing::debug!("   Payer-signed transaction size: {} bytes (base64)", 
            request.payer_signed_transaction_base64.len());

        // Decode payer-signed transaction
        tracing::debug!("🔓 Decoding payer-signed transaction from base64...");
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        let payer_signed_tx_bytes = BASE64.decode(&request.payer_signed_transaction_base64)
            .map_err(|e| {
                tracing::error!("❌ Failed to decode payer-signed transaction: {}", e);
                format!("Failed to decode payer-signed transaction: {}", e)
            })?;

        tracing::debug!("   Decoded transaction size: {} bytes", payer_signed_tx_bytes.len());

        // Deserialize transaction
        tracing::debug!("📦 Deserializing transaction...");
        let mut tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&payer_signed_tx_bytes)
            .map_err(|e| {
                tracing::error!("❌ Failed to deserialize transaction: {}", e);
                format!("Failed to deserialize transaction: {}", e)
            })?;

        tracing::info!("📋 Transaction has {} signature(s) before adding nonce signatures", tx.signatures.len());
        tracing::debug!("   Transaction has {} account(s) in message", tx.message.account_keys.len());
        tracing::debug!("   Blockhash: {}", tx.message.recent_blockhash);
        tracing::info!("🔐 Adding signatures for {} nonce account(s)", request.nonce_keypair_base64.len());

        // Decode all nonce keypairs
        tracing::debug!("🔑 Decoding {} nonce keypair(s)...", request.nonce_keypair_base64.len());
        let mut nonce_keypairs = Vec::new();
        for (i, keypair_base64) in request.nonce_keypair_base64.iter().enumerate() {
            tracing::debug!("   Decoding keypair {} (base64 size: {} bytes)...", i + 1, keypair_base64.len());
            let nonce_keypair_bytes = BASE64.decode(keypair_base64)
                .map_err(|e| {
                    tracing::error!("❌ Failed to decode nonce keypair {}: {}", i, e);
                    format!("Failed to decode nonce keypair {}: {}", i, e)
                })?;

            if nonce_keypair_bytes.len() != 64 {
                tracing::error!("❌ Invalid nonce keypair {} length: expected 64, got {}", i, nonce_keypair_bytes.len());
                return Err(format!("Invalid nonce keypair {} length: expected 64, got {}", i, nonce_keypair_bytes.len()));
            }

            let nonce_keypair = solana_sdk::signature::Keypair::from_bytes(&nonce_keypair_bytes)
                .map_err(|e| {
                    tracing::error!("❌ Failed to create keypair {} from bytes: {}", i, e);
                    format!("Failed to create keypair {} from bytes: {}", i, e)
                })?;

            tracing::info!("  🔑 Nonce keypair {} pubkey: {}", i + 1, nonce_keypair.pubkey());
            nonce_keypairs.push(nonce_keypair);
        }
        
        tracing::debug!("✅ Decoded {} nonce keypair(s)", nonce_keypairs.len());

        // Get the blockhash from the transaction
        let blockhash = tx.message.recent_blockhash;
        tracing::debug!("   Using blockhash: {}", blockhash);

        // Add all nonce signatures (each nonce account needs to sign)
        // Convert Vec<Keypair> to Vec<&Keypair> for try_partial_sign
        tracing::debug!("✍️  Signing transaction with {} nonce keypair(s)...", nonce_keypairs.len());
        let nonce_keypair_refs: Vec<&solana_sdk::signature::Keypair> = nonce_keypairs.iter().collect();
        tx.try_partial_sign(&nonce_keypair_refs, blockhash)
            .map_err(|e| {
                tracing::error!("❌ Failed to add nonce signatures: {}", e);
                format!("Failed to add nonce signatures: {}", e)
            })?;

        tracing::info!("✅ Transaction now has {} signature(s) (payer + {} nonce account(s))", 
            tx.signatures.len(), nonce_keypairs.len());
        tracing::debug!("   Signature breakdown: 1 payer + {} nonce = {} total", 
            nonce_keypairs.len(), tx.signatures.len());

        // Serialize the fully-signed transaction
        tracing::debug!("💾 Serializing fully-signed transaction...");
        let fully_signed_bytes = bincode1::serialize(&tx)
            .map_err(|e| {
                tracing::error!("❌ Failed to serialize fully-signed transaction: {}", e);
                format!("Failed to serialize fully-signed transaction: {}", e)
            })?;

        tracing::debug!("   Serialized size: {} bytes", fully_signed_bytes.len());

        let fully_signed_base64 = BASE64.encode(&fully_signed_bytes);

        tracing::info!("✅ Fully-signed transaction ready for submission ({} bytes, base64: {} bytes)", 
            fully_signed_bytes.len(), fully_signed_base64.len());

        let response: FfiResult<String> = FfiResult::success(fully_signed_base64);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Transaction Refresh
// =============================================================================

/// Refresh blockhash in an unsigned transaction
/// 
/// Use this right before sending an unsigned transaction to MWA for signing
/// to ensure the blockhash is fresh and won't expire during the signing process.
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `unsigned_tx_base64` - Base64-encoded unsigned transaction string
/// 
/// # Returns
/// JSON result string containing refreshed unsigned transaction base64 (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_refresh_blockhash_in_unsigned_transaction(
    handle: i64,
    unsigned_tx_base64: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("🔄 FFI refreshBlockhashInUnsignedTransaction called with handle={}", handle);

        let transport = get_transport(handle)?;

        // Get the base64 string from C string
        let tx_base64_str = unsafe { c_str_to_string(unsigned_tx_base64)? };

        tracing::debug!("📥 Unsigned transaction size: {} chars (base64)", tx_base64_str.len());

        // Refresh blockhash
        let refreshed_tx = runtime::block_on(async {
            transport
                .transaction_service()
                .refresh_blockhash_in_unsigned_transaction(&tx_base64_str)
                .await
        })
        .map_err(|e| format!("Failed to refresh blockhash: {}", e))?;

        tracing::info!("✅ Blockhash refreshed successfully");
        tracing::debug!("   Refreshed transaction size: {} chars (base64)", refreshed_tx.len());

        let response: FfiResult<String> = FfiResult::success(refreshed_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// BLE Mesh Operations
// =============================================================================

/// Fragment a signed transaction for BLE transmission
/// Returns JSON with array of fragment bytes (base64 encoded)
/// 
/// # Arguments
/// * `transaction_bytes` - Transaction bytes as C byte array
/// * `transaction_len` - Length of transaction bytes
/// 
/// # Returns
/// JSON result string containing array of fragment data (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_fragment_transaction(
    transaction_bytes: *const u8,
    transaction_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("🔄 FFI fragmentTransaction called");
        
        let tx_bytes = unsafe { c_bytes_to_vec(transaction_bytes, transaction_len) };
        
        tracing::info!("Fragmenting transaction of {} bytes", tx_bytes.len());
        
        // Fragment the transaction
        let fragments = crate::ble::fragment_transaction(&tx_bytes);
        
        // Convert fragments to FFI-friendly format
        #[derive(serde::Serialize)]
        struct FragmentData {
            #[serde(rename = "transactionId")]
            transaction_id: String,
            #[serde(rename = "fragmentIndex")]
            fragment_index: u16,
            #[serde(rename = "totalFragments")]
            total_fragments: u16,
            #[serde(rename = "dataBase64")]
            data_base64: String,
        }
        
        let fragment_data: Vec<FragmentData> = fragments.iter().map(|f| {
            FragmentData {
                transaction_id: hex::encode(&f.transaction_id),
                fragment_index: f.fragment_index,
                total_fragments: f.total_fragments,
                data_base64: {
                    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
                    BASE64.encode(&f.data)
                },
            }
        }).collect();
        
        tracing::info!("✅ Created {} fragments", fragment_data.len());
        
        let response: FfiResult<Vec<FragmentData>> = FfiResult::success(fragment_data);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    
    create_result_string(result)
}

/// Reconstruct a transaction from fragments
/// Takes JSON array of fragment objects with base64 data
/// 
/// # Arguments
/// * `fragments_json` - JSON bytes containing array of fragment data
/// * `fragments_len` - Length of JSON bytes
/// 
/// # Returns
/// JSON result string containing base64-encoded reconstructed transaction (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_reconstruct_transaction(
    fragments_json: *const u8,
    fragments_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("🔗 FFI reconstructTransaction called");
        
        let json_data = unsafe { c_bytes_to_vec(fragments_json, fragments_len) };
        
        // Parse fragment data from JSON
        #[derive(serde::Deserialize)]
        struct FragmentData {
            #[serde(rename = "transactionId")]
            transaction_id: String,
            #[serde(rename = "fragmentIndex")]
            fragment_index: u16,
            #[serde(rename = "totalFragments")]
            total_fragments: u16,
            #[serde(rename = "dataBase64")]
            data_base64: String,
        }
        
        let fragment_data: Vec<FragmentData> = serde_json::from_slice(&json_data)
            .map_err(|e| format!("Failed to parse fragments JSON: {}", e))?;
        
        tracing::info!("Reconstructing from {} fragments", fragment_data.len());
        
        // Convert to internal fragment format
        let fragments: Vec<crate::ble::mesh::TransactionFragment> = fragment_data.iter().map(|f| {
            let mut tx_id = [0u8; 32];
            let tx_id_bytes = hex::decode(&f.transaction_id)
                .map_err(|e| format!("Invalid transaction ID: {}", e))?;
            if tx_id_bytes.len() != 32 {
                return Err(format!("Invalid transaction ID length: expected 32, got {}", tx_id_bytes.len()));
            }
            tx_id.copy_from_slice(&tx_id_bytes);
            
            use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
            let data = BASE64.decode(&f.data_base64)
                .map_err(|e| format!("Invalid fragment data: {}", e))?;
            
            Ok(crate::ble::mesh::TransactionFragment {
                transaction_id: tx_id,
                fragment_index: f.fragment_index,
                total_fragments: f.total_fragments,
                data,
            })
        }).collect::<Result<Vec<_>, String>>()?;
        
        // Reconstruct the transaction
        let reconstructed = crate::ble::reconstruct_transaction(&fragments)
            .map_err(|e| format!("Reconstruction failed: {}", e))?;
        
        tracing::info!("✅ Reconstructed transaction: {} bytes", reconstructed.len());
        
        // Return base64-encoded transaction
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        let tx_base64 = BASE64.encode(&reconstructed);
        
        let response: FfiResult<String> = FfiResult::success(tx_base64);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    
    create_result_string(result)
}

/// Get fragmentation statistics for a transaction
/// 
/// # Arguments
/// * `transaction_bytes` - Transaction bytes as C byte array
/// * `transaction_len` - Length of transaction bytes
/// 
/// # Returns
/// JSON result string containing fragmentation stats (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_fragmentation_stats(
    transaction_bytes: *const u8,
    transaction_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("📊 FFI getFragmentationStats called");
        
        let tx_bytes = unsafe { c_bytes_to_vec(transaction_bytes, transaction_len) };
        
        let stats = crate::ble::FragmentationStats::calculate(&tx_bytes);
        
        #[derive(serde::Serialize)]
        struct StatsResponse {
            #[serde(rename = "originalSize")]
            original_size: usize,
            #[serde(rename = "fragmentCount")]
            fragment_count: usize,
            #[serde(rename = "maxFragmentSize")]
            max_fragment_size: usize,
            #[serde(rename = "avgFragmentSize")]
            avg_fragment_size: usize,
            #[serde(rename = "totalOverhead")]
            total_overhead: usize,
            efficiency: f32,
        }
        
        let stats_response = StatsResponse {
            original_size: stats.original_size,
            fragment_count: stats.fragment_count,
            max_fragment_size: stats.max_fragment_size,
            avg_fragment_size: stats.avg_fragment_size,
            total_overhead: stats.total_overhead,
            efficiency: stats.efficiency,
        };
        
        let response: FfiResult<StatsResponse> = FfiResult::success(stats_response);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    
    create_result_string(result)
}

/// Prepare a transaction broadcast (fragments it and returns fragments with packets)
/// Takes transaction bytes and returns fragments ready for BLE transmission
/// 
/// # Arguments
/// * `handle` - SDK handle (unused but kept for API consistency)
/// * `transaction_bytes` - Transaction bytes as C byte array
/// * `transaction_len` - Length of transaction bytes
/// 
/// # Returns
/// JSON result string containing broadcast preparation with fragment packets (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_prepare_broadcast(
    _handle: i64,
    transaction_bytes: *const u8,
    transaction_len: usize,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("📡 FFI prepareBroadcast called");
        
        let tx_bytes = unsafe { c_bytes_to_vec(transaction_bytes, transaction_len) };
        
        tracing::info!("Preparing broadcast for {} byte transaction", tx_bytes.len());
        
        // Fragment the transaction
        let fragments = crate::ble::fragment_transaction(&tx_bytes);
        if fragments.is_empty() {
            return Err("No fragments created from transaction".to_string());
        }
        let transaction_id = fragments[0].transaction_id;
        
        // Create broadcaster to prepare packets
        let broadcaster = crate::ble::TransactionBroadcaster::new(uuid::Uuid::new_v4());
        
        // Prepare packet for each fragment
        #[derive(serde::Serialize)]
        struct FragmentPacket {
            #[serde(rename = "transactionId")]
            transaction_id: String,
            #[serde(rename = "fragmentIndex")]
            fragment_index: u16,
            #[serde(rename = "totalFragments")]
            total_fragments: u16,
            #[serde(rename = "packetBytes")]
            packet_bytes: String, // Base64-encoded mesh packet
        }
        
        let mut fragment_packets = Vec::new();
        for fragment in &fragments {
            let packet_bytes = broadcaster.prepare_fragment_packet(fragment)
                .map_err(|e| format!("Failed to prepare fragment packet: {}", e))?;
            
            use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
            fragment_packets.push(FragmentPacket {
                transaction_id: hex::encode(&fragment.transaction_id),
                fragment_index: fragment.fragment_index,
                total_fragments: fragment.total_fragments,
                packet_bytes: BASE64.encode(&packet_bytes),
            });
        }
        
        tracing::info!("✅ Prepared {} fragment packets for broadcast", fragment_packets.len());
        
        #[derive(serde::Serialize)]
        struct BroadcastPreparation {
            #[serde(rename = "transactionId")]
            transaction_id: String,
            #[serde(rename = "fragmentPackets")]
            fragment_packets: Vec<FragmentPacket>,
        }
        
        let preparation = BroadcastPreparation {
            transaction_id: hex::encode(&transaction_id),
            fragment_packets,
        };
        
        let response: FfiResult<BroadcastPreparation> = FfiResult::success(preparation);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    
    create_result_string(result)
}

// =============================================================================
// Health Monitoring
// =============================================================================

/// Get mesh health snapshot
/// Returns current health metrics, peer status, and network topology
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing health snapshot (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_health_snapshot(
    handle: i64,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("💚 FFI getHealthSnapshot called");

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        let snapshot = monitor.get_snapshot();

        tracing::info!("✅ Health snapshot: {} peers, health score: {}", 
            snapshot.metrics.total_peers, snapshot.metrics.health_score);

        #[derive(serde::Serialize)]
        struct HealthSnapshotResponse {
            snapshot: crate::ble::HealthSnapshot,
        }

        let response: FfiResult<HealthSnapshotResponse> = FfiResult::success(HealthSnapshotResponse { snapshot });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Record peer heartbeat
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `peer_id` - Peer identifier string
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_record_peer_heartbeat(
    handle: i64,
    peer_id: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("💓 FFI recordPeerHeartbeat called");

        let peer_id_str = unsafe { c_str_to_string(peer_id)? };

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        monitor.record_heartbeat(&peer_id_str);

        tracing::info!("✅ Recorded heartbeat for peer: {}", peer_id_str);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Record peer latency measurement
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `peer_id` - Peer identifier string
/// * `latency_ms` - Latency in milliseconds (as c_int)
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_record_peer_latency(
    handle: i64,
    peer_id: *const c_char,
    latency_ms: c_int,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("⏱️ FFI recordPeerLatency called");

        let peer_id_str = unsafe { c_str_to_string(peer_id)? };

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        monitor.record_latency(&peer_id_str, latency_ms as u32);

        tracing::info!("✅ Recorded {}ms latency for peer: {}", latency_ms, peer_id_str);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Record peer RSSI (signal strength)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `peer_id` - Peer identifier string
/// * `rssi` - RSSI value in dBm (as c_int)
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_record_peer_rssi(
    handle: i64,
    peer_id: *const c_char,
    rssi: c_int,
) -> *mut c_char {
    let result = (|| -> Result<String, String> {
        tracing::info!("📶 FFI recordPeerRssi called");

        let peer_id_str = unsafe { c_str_to_string(peer_id)? };

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        monitor.record_rssi(&peer_id_str, rssi as i8);

        tracing::info!("✅ Recorded {}dBm RSSI for peer: {}", rssi, peer_id_str);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Received Queue Operations
// =============================================================================

/// Push a received transaction into the auto-submission queue
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `transaction_bytes` - Transaction bytes as C byte array
/// * `transaction_len` - Length of transaction bytes
/// 
/// # Returns
/// JSON result string containing push result and queue size (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_push_received_transaction(
    handle: i64,
    transaction_bytes: *const u8,
    transaction_len: usize,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let tx_bytes = unsafe { c_bytes_to_vec(transaction_bytes, transaction_len) };
        
        let transport = get_transport(handle)?;
        let added = transport.push_received_transaction(tx_bytes);
        
        #[derive(serde::Serialize)]
        struct PushResponse {
            added: bool,
            #[serde(rename = "queueSize")]
            queue_size: usize,
        }
        
        let queue_size = transport.received_queue_size();
        let response: FfiResult<PushResponse> = FfiResult::success(PushResponse { added, queue_size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Get next received transaction for auto-submission
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing transaction data or None (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_next_received_transaction(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        tracing::debug!("🔍 FFI nextReceivedTransaction called with handle: {}", handle);
        let transport = get_transport(handle)?;
        tracing::debug!("✅ Got transport instance for handle {}", handle);
        
        let queue_size_before = transport.received_queue_size();
        tracing::debug!("📊 Queue size before pop: {}", queue_size_before);
        
        match transport.next_received_transaction() {
            Some((tx_id, tx_bytes, received_at)) => {
                tracing::debug!("✅ Popped transaction {} ({} bytes) from queue", tx_id, tx_bytes.len());
                use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
                
                #[derive(serde::Serialize)]
                struct ReceivedTransaction {
                    #[serde(rename = "txId")]
                    tx_id: String,
                    #[serde(rename = "transactionBase64")]
                    transaction_base64: String,
                    #[serde(rename = "receivedAt")]
                    received_at: u64,
                }
                
                let response: FfiResult<ReceivedTransaction> = FfiResult::success(ReceivedTransaction {
                    tx_id: tx_id.clone(),
                    transaction_base64: BASE64.encode(&tx_bytes),
                    received_at,
                });
                
                let json_response = serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))?;
                tracing::debug!("📤 FFI nextReceivedTransaction returning transaction {} (JSON length: {})", tx_id, json_response.len());
                Ok(json_response)
            }
            None => {
                tracing::debug!("📭 No transaction in queue, returning None");
                let response: FfiResult<Option<String>> = FfiResult::success(None);
                let json_response = serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))?;
                tracing::debug!("📤 FFI nextReceivedTransaction returning None (JSON: {})", json_response);
                Ok(json_response)
            }
        }
    })();

    create_result_string(result)
}

/// Get count of transactions waiting for auto-submission
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing queue size (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_received_queue_size(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        tracing::debug!("🔍 FFI getReceivedQueueSize called with handle: {}", handle);
        let transport = get_transport(handle)?;
        tracing::debug!("✅ Got transport instance for handle {}", handle);
        
        let queue_size = transport.received_queue_size();
        tracing::debug!("📊 Transport received_queue_size() returned: {}", queue_size);
        
        #[derive(serde::Serialize)]
        struct QueueSizeResponse {
            #[serde(rename = "queueSize")]
            queue_size: usize,
        }
        
        let response: FfiResult<QueueSizeResponse> = FfiResult::success(QueueSizeResponse { queue_size });
        let json_response = serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))?;
        tracing::debug!("📤 FFI getReceivedQueueSize returning JSON: {}", json_response);
        Ok(json_response)
    })();

    create_result_string(result)
}

/// Get fragment reassembly info for all incomplete transactions
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing fragment reassembly info list (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_fragment_reassembly_info(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let info_list = transport.get_fragment_reassembly_info();
        
        let response: FfiResult<FragmentReassemblyInfoList> = FfiResult::success(
            FragmentReassemblyInfoList {
                transactions: info_list,
            }
        );
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Mark a transaction as successfully submitted
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `transaction_bytes` - Transaction bytes as C byte array
/// * `transaction_len` - Length of transaction bytes
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_mark_transaction_submitted(
    handle: i64,
    transaction_bytes: *const u8,
    transaction_len: usize,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let tx_bytes = unsafe { c_bytes_to_vec(transaction_bytes, transaction_len) };
        
        let transport = get_transport(handle)?;
        transport.mark_transaction_submitted(&tx_bytes);
        
        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }
        
        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Clean up old submitted transaction hashes
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_cleanup_old_submissions(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        transport.cleanup_old_submissions();
        
        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }
        
        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

// =============================================================================
// Queue Management Operations
// =============================================================================

/// Get outbound queue debug info (non-destructive peek)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing queue debug info (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_debug_outbound_queue(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let queue_info = transport.outbound_queue_debug();

        #[derive(serde::Serialize)]
        struct FragmentInfo {
            index: usize,
            size: usize,
        }

        #[derive(serde::Serialize)]
        struct QueueDebugResponse {
            #[serde(rename = "totalFragments")]
            total_fragments: usize,
            fragments: Vec<FragmentInfo>,
        }

        let fragments: Vec<FragmentInfo> = queue_info
            .iter()
            .map(|(idx, size)| FragmentInfo { index: *idx, size: *size })
            .collect();

        let total_bytes: usize = fragments.iter().map(|f| f.size).sum();

        tracing::info!("🔍 Queue debug: {} fragments, {} total bytes", fragments.len(), total_bytes);

        let response = QueueDebugResponse {
            total_fragments: fragments.len(),
            fragments,
        };

        let ffi_response: FfiResult<QueueDebugResponse> = FfiResult::success(response);
        serde_json::to_string(&ffi_response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Save all queues to disk
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_save_queues(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        runtime::block_on(async {
            // Save queue manager queues (outbound, retry, confirmation)
            transport.sdk.queue_manager().force_save().await
                .map_err(|e| format!("Failed to save queues: {}", e))?;

            // Save received queue if storage directory is available
            if let Ok(queue_storage_dir) = std::env::var("POLLINET_QUEUE_STORAGE") {
                if let Err(e) = transport.save_received_queue(&queue_storage_dir) {
                    tracing::warn!("⚠️ Failed to save received queue: {}", e);
                    // Don't fail the entire operation if received queue save fails
                }
            }

            Ok::<(), String>(())
        })?;

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Trigger auto-save if needed (debounced)
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_auto_save_queues(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        runtime::block_on(async {
            // Auto-save queue manager queues (outbound, retry, confirmation)
            transport.sdk.queue_manager().save_if_needed().await
                .map_err(|e| format!("Failed to auto-save queues: {}", e))?;

            // Auto-save received queue if storage directory is available
            // Note: Received queue uses the same debouncing as queue manager
            if let Ok(queue_storage_dir) = std::env::var("POLLINET_QUEUE_STORAGE") {
                if let Err(e) = transport.save_received_queue(&queue_storage_dir) {
                    tracing::warn!("⚠️ Failed to auto-save received queue: {}", e);
                    // Don't fail the entire operation if received queue save fails
                }
            }

            Ok::<(), String>(())
        })?;

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Push transaction to outbound queue
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request string
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_push_outbound_transaction(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let request_str = unsafe { c_str_to_string(request_json)? };

        let request: PushOutboundRequest = serde_json::from_str(&request_str)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Convert FFI fragments to mesh fragments
        let fragments: Result<Vec<crate::ble::mesh::TransactionFragment>, String> = request.fragments
            .iter()
            .map(|f| {
                let tx_id = hex::decode(&f.transaction_id)
                    .map_err(|e| format!("Invalid transaction ID: {}", e))?;
                if tx_id.len() != 32 {
                    return Err("Transaction ID must be 32 bytes".to_string());
                }
                let mut tx_id_array = [0u8; 32];
                tx_id_array.copy_from_slice(&tx_id);

                use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
                let data = BASE64.decode(&f.data_base64)
                    .map_err(|e| format!("Invalid fragment data: {}", e))?;

                Ok(crate::ble::mesh::TransactionFragment {
                    transaction_id: tx_id_array,
                    fragment_index: f.fragment_index,
                    total_fragments: f.total_fragments,
                    data,
                })
            })
            .collect();

        let fragments = fragments?;
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        let tx_bytes = BASE64.decode(&request.tx_bytes)
            .map_err(|e| format!("Invalid transaction bytes: {}", e))?;

        // Convert priority
        let priority = match request.priority {
            PriorityFFI::High => crate::queue::Priority::High,
            PriorityFFI::Normal => crate::queue::Priority::Normal,
            PriorityFFI::Low => crate::queue::Priority::Low,
        };

        // Create outbound transaction
        let outbound_tx = crate::queue::OutboundTransaction::new(
            request.tx_id,
            tx_bytes,
            fragments,
            priority,
        );

        // Push to queue
        runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.push(outbound_tx)
                .map_err(|e| format!("Failed to push to queue: {}", e))?;
            Ok::<(), String>(())
        })?;

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Pop next transaction from outbound queue
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing outbound transaction or None (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_pop_outbound_transaction(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let tx_opt = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.pop()
        });

        if let Some(tx) = tx_opt {
            use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
            let tx_ffi = OutboundTransactionFFI {
                tx_id: tx.tx_id,
                original_bytes: BASE64.encode(&tx.original_bytes),
                fragment_count: tx.fragments.len(),
                priority: match tx.priority {
                    crate::queue::Priority::High => PriorityFFI::High,
                    crate::queue::Priority::Normal => PriorityFFI::Normal,
                    crate::queue::Priority::Low => PriorityFFI::Low,
                },
                created_at: tx.created_at,
                retry_count: tx.retry_count,
            };

            let response: FfiResult<Option<OutboundTransactionFFI>> = FfiResult::success(Some(tx_ffi));
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        } else {
            let response: FfiResult<Option<OutboundTransactionFFI>> = FfiResult::success(None);
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        }
    })();

    create_result_string(result)
}

/// Get outbound queue size
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing queue size (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_outbound_queue_size(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let size = runtime::block_on(async {
            let queue = transport.sdk.queue_manager().outbound.read().await;
            queue.len()
        });

        #[derive(serde::Serialize)]
        struct QueueSizeResponse {
            #[serde(rename = "queueSize")]
            queue_size: usize,
        }

        let response: FfiResult<QueueSizeResponse> = FfiResult::success(QueueSizeResponse { queue_size: size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Add transaction to retry queue
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request string
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_add_to_retry_queue(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let request_str = unsafe { c_str_to_string(request_json)? };

        let request: AddToRetryRequest = serde_json::from_str(&request_str)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        let tx_bytes = BASE64.decode(&request.tx_bytes)
            .map_err(|e| format!("Invalid transaction bytes: {}", e))?;

        let retry_item = crate::queue::RetryItem::new(
            tx_bytes,
            request.tx_id,
            request.error,
        );

        runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().retries.write().await;
            queue.push(retry_item)
                .map_err(|e| format!("Failed to push to retry queue: {}", e))?;
            Ok::<(), String>(())
        })?;

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Pop next ready retry item
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing retry item or None (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_pop_ready_retry(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let retry_opt = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().retries.write().await;
            queue.pop_ready()
        });

        if let Some(retry) = retry_opt {
            use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
            let retry_ffi = RetryItemFFI {
                tx_bytes: BASE64.encode(&retry.tx_bytes),
                tx_id: retry.tx_id.clone(),
                attempt_count: retry.attempt_count,
                last_error: retry.last_error.clone(),
                next_retry_in_secs: retry.time_until_retry().as_secs(),
                age_seconds: retry.age().as_secs(),
            };

            let response: FfiResult<Option<RetryItemFFI>> = FfiResult::success(Some(retry_ffi));
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        } else {
            let response: FfiResult<Option<RetryItemFFI>> = FfiResult::success(None);
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        }
    })();

    create_result_string(result)
}

/// Get retry queue size
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing queue size (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_get_retry_queue_size(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let size = runtime::block_on(async {
            let queue = transport.sdk.queue_manager().retries.read().await;
            queue.len()
        });

        #[derive(serde::Serialize)]
        struct QueueSizeResponse {
            #[serde(rename = "queueSize")]
            queue_size: usize,
        }

        let response: FfiResult<QueueSizeResponse> = FfiResult::success(QueueSizeResponse { queue_size: size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Cleanup expired confirmations and retry items
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing cleanup counts (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_cleanup_expired(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let (confirmations_cleaned, retries_cleaned) = runtime::block_on(async {
            let mut conf_queue = transport.sdk.queue_manager().confirmations.write().await;
            let conf_cleaned = conf_queue.cleanup_expired();

            let mut retry_queue = transport.sdk.queue_manager().retries.write().await;
            let retry_cleaned = retry_queue.cleanup_expired();

            (conf_cleaned, retry_cleaned)
        });

        #[derive(serde::Serialize)]
        struct CleanupExpiredResponse {
            #[serde(rename = "confirmationsCleaned")]
            confirmations_cleaned: usize,
            #[serde(rename = "retriesCleaned")]
            retries_cleaned: usize,
        }

        let response: FfiResult<CleanupExpiredResponse> = FfiResult::success(CleanupExpiredResponse {
            confirmations_cleaned,
            retries_cleaned,
        });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Queue a confirmation for relay back to origin device
/// 
/// # Arguments
/// * `handle` - SDK handle
/// * `request_json` - JSON request string
/// 
/// # Returns
/// JSON result string containing success status (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_queue_confirmation(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        // Parse request JSON
        let request_str = unsafe { c_str_to_string(request_json)? };

        let request: QueueConfirmationRequest =
            serde_json::from_str(&request_str)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        tracing::info!(
            "📨 Queueing confirmation for tx {} with signature {}...",
            request.tx_id,
            &request.signature[..std::cmp::min(16, request.signature.len())]
        );

        // Push into confirmation queue (auto-relay subsystem)
        runtime::block_on(async {
            let mut conf_queue = transport.sdk.queue_manager().confirmations.write().await;
            // Confirmation queue expects tx_id as [u8; 32]
            let tx_id_bytes = hex::decode(&request.tx_id)
                .map_err(|e| format!("Invalid txId hex: {}", e))?;
            if tx_id_bytes.len() != 32 {
                return Err(format!(
                    "Invalid txId length: expected 32 bytes, got {}",
                    tx_id_bytes.len()
                ));
            }
            let mut tx_id_array = [0u8; 32];
            tx_id_array.copy_from_slice(&tx_id_bytes);

            let confirmation = crate::queue::confirmation::Confirmation::success(
                tx_id_array,
                request.signature.clone(),
            );

            conf_queue
                .push(confirmation)
                .map_err(|e| format!("Failed to queue confirmation: {:?}", e))?;

            Ok::<(), String>(())
        })?;

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}

/// Pop next confirmation from queue
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing confirmation or None (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_pop_confirmation(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let confirmation = runtime::block_on(async {
            let mut conf_queue = transport.sdk.queue_manager().confirmations.write().await;
            conf_queue.pop()
        });

        if let Some(conf) = confirmation {
            // Convert Rust Confirmation to FFI format
            let tx_id_hex = hex::encode(&conf.original_tx_id);
            let status_ffi = match &conf.status {
                crate::queue::confirmation::ConfirmationStatus::Success { signature } => {
                    ConfirmationStatusFFI::Success {
                        signature: signature.clone(),
                    }
                }
                crate::queue::confirmation::ConfirmationStatus::Failed { error } => {
                    ConfirmationStatusFFI::Failed {
                        error: error.clone(),
                    }
                }
            };

            let conf_ffi = ConfirmationFFI {
                tx_id: tx_id_hex,
                status: status_ffi,
                timestamp: conf.timestamp,
                relay_count: conf.relay_count,
            };

            let response: FfiResult<Option<ConfirmationFFI>> =
                FfiResult::success(Some(conf_ffi));
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        } else {
            let response: FfiResult<Option<ConfirmationFFI>> =
                FfiResult::success(None);
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        }
    })();

    create_result_string(result)
}

/// Cleanup stale fragments from the transaction cache
/// 
/// # Arguments
/// * `handle` - SDK handle
/// 
/// # Returns
/// JSON result string containing cleanup count (must be freed with pollinet_free_string)
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_cleanup_stale_fragments(
    handle: i64,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        // Cleanup stale fragments (older than 5 minutes = 300 seconds)
        // cleanup_stale_fragments is on TransactionCache, accessed via SDK's local_cache
        let cleaned = runtime::block_on(async {
            let mut cache = transport.sdk.local_cache.write().await;
            cache.cleanup_stale_fragments(300)
        });

        #[derive(serde::Serialize)]
        struct CleanupResponse {
            #[serde(rename = "fragmentsCleaned")]
            fragments_cleaned: usize,
        }

        let response: FfiResult<CleanupResponse> = FfiResult::success(CleanupResponse {
            fragments_cleaned: cleaned,
        });

        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(result)
}
