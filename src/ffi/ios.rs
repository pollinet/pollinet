//! iOS C FFI interface
//!
//! Plain `extern "C"` functions that Swift can call directly via a bridging header.
//! No JNI types — strings cross the boundary as null-terminated C strings,
//! raw byte buffers cross as pointer + length pairs.
//!
//! Memory contract:
//!   - Every `*mut c_char` returned by these functions MUST be freed by the caller
//!     using `pollinet_free_string`.
//!   - Every `*mut u8` returned by `pollinet_next_outbound` MUST be freed by the
//!     caller using `pollinet_free_bytes`.
//!   - All other pointer arguments are borrowed for the duration of the call only.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Arc;

use parking_lot::Mutex;
use log::info;

use super::runtime;
use super::transport::HostBleTransport;
use super::types::*;

// =============================================================================
// Transport registry (identical pattern to android.rs)
// =============================================================================

lazy_static::lazy_static! {
    static ref TRANSPORTS: Arc<Mutex<Vec<Option<Arc<HostBleTransport>>>>> =
        Arc::new(Mutex::new(Vec::new()));
}

// =============================================================================
// Internal helpers
// =============================================================================

fn get_transport(handle: i64) -> Result<Arc<HostBleTransport>, String> {
    let transports = TRANSPORTS.lock();
    let idx = handle as usize;
    transports
        .get(idx)
        .and_then(|slot| slot.as_ref())
        .cloned()
        .ok_or_else(|| format!("Invalid handle: {}", handle))
}

/// Allocate a CString from a Result<String, String>.
/// On Ok  → returns the JSON payload.
/// On Err → returns a JSON FfiResult error envelope.
fn result_to_cstring(result: Result<String, String>) -> *mut c_char {
    let json = match result {
        Ok(s) => s,
        Err(e) => {
            let err: FfiResult<()> = FfiResult::error("FFI_ERROR", e);
            serde_json::to_string(&err).unwrap_or_else(|_| r#"{"ok":false,"code":"SERIALIZE","message":"serialization failed"}"#.into())
        }
    };
    CString::new(json)
        .unwrap_or_else(|_| CString::new(r#"{"ok":false,"code":"ENCODE","message":"nul byte in response"}"#).unwrap())
        .into_raw()
}

fn c_str(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("null pointer".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(|s| s.to_owned())
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

fn parse_log_level(level: Option<&str>) -> tracing::Level {
    match level {
        Some("error") => tracing::Level::ERROR,
        Some("warn")  => tracing::Level::WARN,
        Some("debug") => tracing::Level::DEBUG,
        Some("trace") => tracing::Level::TRACE,
        _             => tracing::Level::INFO,
    }
}

// =============================================================================
// Memory management — Swift must call these to free heap allocations
// =============================================================================

/// Free a string returned by any pollinet FFI function.
#[no_mangle]
pub extern "C" fn pollinet_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Free a byte buffer returned by `pollinet_next_outbound`.
#[no_mangle]
pub extern "C" fn pollinet_free_bytes(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        unsafe { drop(Vec::from_raw_parts(ptr, len, len)) };
    }
}

// =============================================================================
// Initialization and lifecycle
// =============================================================================

/// Initialize the PolliNet SDK.
/// `config_json` — null-terminated JSON string (SdkConfig).
/// Returns a non-negative handle on success, or -1 on failure.
#[no_mangle]
pub extern "C" fn pollinet_init(config_json: *const c_char) -> i64 {
    let result: Result<i64, String> = (|| {
        match runtime::init_runtime() {
            Ok(_) => {}
            Err(e) if e.contains("already initialized") => {}
            Err(e) => return Err(format!("Failed to initialize runtime: {}", e)),
        }

        let config_str = c_str(config_json)?;
        let config: SdkConfig = serde_json::from_str(&config_str)
            .map_err(|e| format!("Failed to parse config: {}", e))?;

        if config.enable_logging {
            let _ = tracing_subscriber::fmt()
                .with_max_level(parse_log_level(config.log_level.as_deref()))
                .try_init();
        }

        let mut transport = runtime::block_on(async {
            if let Some(ref rpc_url) = config.rpc_url {
                HostBleTransport::new_with_rpc(rpc_url).await
            } else {
                HostBleTransport::new().await
            }
        })
        .map_err(|e| format!("Transport creation failed: {}", e))?;

        if let Some(ref storage_dir) = config.storage_directory {
            transport
                .set_secure_storage(storage_dir, config.encryption_key.clone())
                .map_err(|e| format!("Secure storage error: {}", e))?;
            let queue_dir = format!("{}/queues", storage_dir);
            transport.set_queue_storage_dir(queue_dir);
        }

        if let Some(ref addr) = config.wallet_address {
            transport.set_wallet_address(Some(addr.clone()));
            info!("✅ Wallet address set: {}", addr);
        }

        let arc = Arc::new(transport);
        let mut transports = TRANSPORTS.lock();
        transports.push(Some(arc));
        Ok((transports.len() - 1) as i64)
    })();

    result.unwrap_or(-1)
}

/// Return a null-terminated version string. Caller must free with `pollinet_free_string`.
#[no_mangle]
pub extern "C" fn pollinet_version() -> *mut c_char {
    CString::new(env!("CARGO_PKG_VERSION"))
        .unwrap()
        .into_raw()
}

/// Shut down the SDK instance identified by `handle` and release its resources.
#[no_mangle]
pub extern "C" fn pollinet_shutdown(handle: i64) {
    let mut transports = TRANSPORTS.lock();
    let idx = handle as usize;
    if idx < transports.len() {
        transports[idx] = None;
    }
}

// =============================================================================
// Host-driven transport API
// =============================================================================

/// Push inbound BLE data into the Rust state machine.
/// Returns JSON `FfiResult`. Caller must free with `pollinet_free_string`.
#[no_mangle]
pub extern "C" fn pollinet_push_inbound(
    handle: i64,
    data: *const u8,
    len: usize,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        transport.push_inbound(bytes)?;
        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Get the next outbound BLE frame.
/// Writes the byte count into `*out_len`.
/// Returns a pointer to the frame bytes (caller frees with `pollinet_free_bytes`),
/// or null if the queue is empty.
#[no_mangle]
pub extern "C" fn pollinet_next_outbound(
    handle: i64,
    max_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    let transport = match get_transport(handle) {
        Ok(t) => t,
        Err(_) => return std::ptr::null_mut(),
    };

    match transport.next_outbound(max_len) {
        Some(bytes) => {
            let len = bytes.len();
            let mut boxed = bytes.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            if !out_len.is_null() {
                unsafe { *out_len = len };
            }
            ptr
        }
        None => {
            if !out_len.is_null() {
                unsafe { *out_len = 0 };
            }
            std::ptr::null_mut()
        }
    }
}

/// Advance the protocol state machine (retry timers, expiry).
/// Returns JSON `FfiResult`. Caller must free with `pollinet_free_string`.
#[no_mangle]
pub extern "C" fn pollinet_tick(handle: i64, now_ms: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let _ = now_ms; // Rust uses SystemTime internally; now_ms reserved for future use
        runtime::block_on(async {
            transport.sdk.tick().await.map_err(|e| e.to_string())
        })?;
        let response: FfiResult<SuccessResponse> = FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Return current transport metrics as JSON.
/// Caller must free with `pollinet_free_string`.
#[no_mangle]
pub extern "C" fn pollinet_metrics(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let snapshot = transport.get_metrics();
        let response: FfiResult<MetricsSnapshot> = FfiResult::success(snapshot);
        serde_json::to_string(&response).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

// =============================================================================
// Queue management
// =============================================================================

/// Pop the next received transaction (reassembled from BLE fragments).
/// Returns JSON `FfiResult<ReceivedTransaction?>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_next_received_transaction(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let tx = runtime::block_on(async {
            transport.sdk.next_received_transaction().await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(tx)).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Submit a transaction to the Solana RPC and return the signature.
/// `request_json` — JSON-encoded SubmitOfflineTransactionRequest.
/// Returns JSON `FfiResult<String>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_submit_offline_transaction(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let req_str = c_str(request_json)?;
        let req: SubmitOfflineTransactionRequest = serde_json::from_str(&req_str)
            .map_err(|e| format!("Parse error: {}", e))?;
        let signature = runtime::block_on(async {
            transport
                .sdk
                .submit_offline_transaction(&req.transaction_base64, req.verify_nonce)
                .await
                .map_err(|e| e.to_string())
        })?;
        let response: FfiResult<String> = FfiResult::success(signature);
        serde_json::to_string(&response).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Mark a transaction as submitted (deduplication).
/// `tx_data` — raw transaction bytes.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_mark_transaction_submitted(
    handle: i64,
    tx_data: *const u8,
    tx_len: usize,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let bytes = unsafe { std::slice::from_raw_parts(tx_data, tx_len) }.to_vec();
        runtime::block_on(async {
            transport.sdk.mark_transaction_submitted(&bytes).await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Add a transaction to the retry queue.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_add_to_retry_queue(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let req_str = c_str(request_json)?;
        let req: AddToRetryRequest = serde_json::from_str(&req_str)
            .map_err(|e| format!("Parse error: {}", e))?;
        let tx_bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &req.tx_bytes,
        )
        .map_err(|e| format!("Base64 decode error: {}", e))?;
        runtime::block_on(async {
            transport
                .sdk
                .add_to_retry_queue(&tx_bytes, &req.tx_id, &req.error)
                .await
                .map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Pop the next retry item that is ready (back-off expired).
/// Returns JSON `FfiResult<RetryItemFFI?>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_pop_ready_retry(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let item = runtime::block_on(async {
            transport.sdk.pop_ready_retry().await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(item)).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Queue a success confirmation for relay back to the origin node.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_queue_confirmation(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let req_str = c_str(request_json)?;
        let req: QueueConfirmationRequest = serde_json::from_str(&req_str)
            .map_err(|e| format!("Parse error: {}", e))?;
        runtime::block_on(async {
            transport
                .sdk
                .queue_confirmation(&req.tx_id, &req.signature)
                .await
                .map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Pop the next outbound confirmation.
/// Returns JSON `FfiResult<ConfirmationFFI?>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_pop_confirmation(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let conf = runtime::block_on(async {
            transport.sdk.pop_confirmation().await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(conf)).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Relay a received confirmation (increment hop count and re-queue).
/// `confirmation_json` — JSON-encoded Confirmation.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_relay_confirmation(
    handle: i64,
    confirmation_json: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let conf_str = c_str(confirmation_json)?;
        runtime::block_on(async {
            transport
                .sdk
                .relay_confirmation(&conf_str)
                .await
                .map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

// =============================================================================
// Fragmentation
// =============================================================================

/// Fragment a signed transaction for BLE transmission.
/// `tx_data` — raw transaction bytes.
/// Returns JSON `FfiResult<FragmentList>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_fragment_transaction(
    handle: i64,
    tx_data: *const u8,
    tx_len: usize,
    max_payload: usize,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let bytes = unsafe { std::slice::from_raw_parts(tx_data, tx_len) }.to_vec();
        let max = if max_payload == 0 { None } else { Some(max_payload) };
        let fragments = runtime::block_on(async {
            transport.sdk.fragment_transaction(&bytes, max).await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(fragments)).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Push a reassembled inbound transaction into the received queue.
/// `tx_data` — raw reassembled transaction bytes.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_push_received_transaction(
    handle: i64,
    tx_data: *const u8,
    tx_len: usize,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let bytes = unsafe { std::slice::from_raw_parts(tx_data, tx_len) }.to_vec();
        runtime::block_on(async {
            transport.sdk.push_received_transaction(bytes).await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

// =============================================================================
// Cleanup
// =============================================================================

/// Remove stale fragments from the reassembly buffer.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_cleanup_stale_fragments(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        runtime::block_on(async {
            transport.sdk.cleanup_stale_fragments().await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Remove expired confirmations and retry items.
/// Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_cleanup_expired(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        runtime::block_on(async {
            transport.sdk.cleanup_expired().await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

// =============================================================================
// Wallet address
// =============================================================================

/// Update the wallet address for reward attribution.
/// Pass an empty string to clear. Returns JSON `FfiResult`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_set_wallet_address(
    handle: i64,
    address: *const c_char,
) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let addr = c_str(address)?;
        let addr_opt = if addr.is_empty() { None } else { Some(addr.clone()) };
        transport.set_wallet_address(addr_opt);
        info!("✅ Wallet address updated: {}", if addr.is_empty() { "<cleared>" } else { &addr });
        serde_json::to_string(&FfiResult::success(SuccessResponse { success: true }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

/// Return the wallet address for this session (empty string if not set).
/// Returns JSON `FfiResult<{address:String}>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_get_wallet_address(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let addr = transport.get_wallet_address().unwrap_or_default();
        #[derive(serde::Serialize)]
        struct Resp { address: String }
        serde_json::to_string(&FfiResult::success(Resp { address: addr }))
            .map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}

// =============================================================================
// Queue metrics
// =============================================================================

/// Return queue size metrics.
/// Returns JSON `FfiResult<QueueMetricsFFI>`. Caller must free.
#[no_mangle]
pub extern "C" fn pollinet_get_queue_metrics(handle: i64) -> *mut c_char {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let metrics = runtime::block_on(async {
            transport.sdk.get_queue_metrics().await.map_err(|e| e.to_string())
        })?;
        serde_json::to_string(&FfiResult::success(metrics)).map_err(|e| e.to_string())
    })();
    result_to_cstring(result)
}
