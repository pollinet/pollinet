//! iOS C ABI interface
//!
//! Thin C marshalling shims over [`super::common`] — the exact counterpart of
//! `android.rs`, one `pollinet_*` function per JNI export, so Swift and Kotlin
//! speak the same contract: the same JSON `FfiResult` envelope
//! (`{"ok":true,"data":…}` / `{"ok":false,"code":…,"message":…}`), the same
//! handle semantics, and the same byte-pump behavior.
//!
//! Conventions:
//! - Strings/JSON cross as NUL-terminated UTF-8 (`char *`). Every returned
//!   `char *` is owned by the caller and MUST be released with
//!   [`pollinet_string_free`].
//! - Byte buffers cross as `(const uint8_t *, size_t)`. The buffer returned by
//!   [`pollinet_next_outbound`] MUST be released with [`pollinet_bytes_free`].
//! - Handles are `int64_t`; init functions return -1 on failure.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use super::common;
use super::types::FfiResult;

// =============================================================================
// C marshalling helpers
// =============================================================================

/// Borrow a C string as `&str`. Errors on null or invalid UTF-8.
///
/// # Safety
/// `ptr` must be null or a valid NUL-terminated string live for the call.
unsafe fn cstr<'a>(ptr: *const c_char, label: &str) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("{} must not be null", label));
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map_err(|e| format!("{} is not valid UTF-8: {}", label, e))
}

/// Borrow a (ptr, len) pair as `&[u8]`. A null pointer is only valid for len == 0.
///
/// # Safety
/// `ptr` must be valid for reads of `len` bytes for the duration of the call.
unsafe fn cbytes<'a>(ptr: *const u8, len: usize, label: &str) -> Result<&'a [u8], String> {
    if ptr.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        return Err(format!("{} must not be null", label));
    }
    Ok(std::slice::from_raw_parts(ptr, len))
}

/// Convert an op result into a caller-owned C string, wrapping errors into the
/// same error envelope Android's `create_result_string` produces.
fn json_out(result: Result<String, String>) -> *mut c_char {
    let json = match result {
        Ok(json) => json,
        Err(e) => {
            log::error!("❌ FFI error: {}", e);
            let error_response: FfiResult<()> = FfiResult::error("ERR_INTERNAL", e);
            serde_json::to_string(&error_response).unwrap_or_else(|_| {
                r#"{"ok":false,"code":"ERR_FATAL","message":"Serialization failed"}"#.to_string()
            })
        }
    };
    to_cstring(json)
}

/// Turn a Rust string into a caller-owned C string (interior NULs stripped —
/// they cannot occur in the JSON/base58/hex payloads this ABI carries).
fn to_cstring(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(e) => {
            let sanitized: Vec<u8> = e.into_vec().into_iter().filter(|&b| b != 0).collect();
            CString::new(sanitized)
                .expect("NUL-free string")
                .into_raw()
        }
    }
}

/// Release a string returned by any `pollinet_*` function.
///
/// # Safety
/// `ptr` must be a pointer previously returned by this library (or null).
#[no_mangle]
pub unsafe extern "C" fn pollinet_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Release a byte buffer returned by [`pollinet_next_outbound`].
///
/// # Safety
/// `(ptr, len)` must be a pair previously returned by this library (or null).
#[no_mangle]
pub unsafe extern "C" fn pollinet_bytes_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

// =============================================================================
// Initialization and lifecycle
// =============================================================================

/// Initialize the PolliNet SDK with a BLE-backed engine.
/// `config_json` is the SdkConfig JSON. Returns a handle, or -1 on failure.
///
/// # Safety
/// `config_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_init(config_json: *const c_char) -> i64 {
    let result =
        cstr(config_json, "config_json").and_then(|c| common::init_common(c.as_bytes()));
    match result {
        Ok(handle) => {
            log::info!("🎉 Returning handle {} to Swift", handle);
            handle
        }
        Err(e) => {
            log::error!("💥 SDK initialization failed: {}", e);
            -1
        }
    }
}

/// Initialize a standalone Wi-Fi transport handle (larger default MTU; on iOS this
/// backs the MultipeerConnectivity driver). Returns a handle, or -1 on failure.
///
/// # Safety
/// `config_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_init_wifi_direct(config_json: *const c_char) -> i64 {
    let result = cstr(config_json, "config_json")
        .and_then(|c| common::init_wifi_direct_common(c.as_bytes()));
    match result {
        Ok(handle) => handle,
        Err(e) => {
            log::error!("💥 Wi-Fi Direct init failed: {}", e);
            -1
        }
    }
}

/// Initialize a Wi-Fi transport handle that shares the engine of an existing BLE
/// handle (one dedup set / queue across both radios). Returns -1 on failure.
#[no_mangle]
pub extern "C" fn pollinet_init_wifi_direct_sharing(ble_handle: i64) -> i64 {
    match common::init_wifi_direct_sharing_common(ble_handle) {
        Ok(handle) => handle,
        Err(e) => {
            log::error!("💥 initWifiDirectSharing failed: {}", e);
            -1
        }
    }
}

/// Return the transport kind for a handle ("BLE" | "WIFI_DIRECT"), or "" if invalid.
#[no_mangle]
pub extern "C" fn pollinet_transport_kind(handle: i64) -> *mut c_char {
    to_cstring(common::transport_kind_common(handle).to_string())
}

/// Get SDK version.
#[no_mangle]
pub extern "C" fn pollinet_version() -> *mut c_char {
    to_cstring(env!("CARGO_PKG_VERSION").to_string())
}

/// Return the pollicore base URL baked in at compile time (POLLICORE_URL), or "".
#[no_mangle]
pub extern "C" fn pollinet_get_pollicore_url() -> *mut c_char {
    to_cstring(option_env!("POLLICORE_URL").unwrap_or("").to_string())
}

/// Derive the Associated Token Account (ATA) address for owner wallet + token mint.
/// Stateless. Returns the base58 ATA, or an empty string on invalid input.
///
/// # Safety
/// `owner` and `mint` must be valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn pollinet_derive_associated_token_account(
    owner: *const c_char,
    mint: *const c_char,
) -> *mut c_char {
    let result = (|| {
        let owner = cstr(owner, "owner")?;
        let mint = cstr(mint, "mint")?;
        common::derive_associated_token_account_common(owner, mint)
    })();
    let s = match result {
        Ok(addr) => addr,
        Err(e) => {
            log::error!("❌ deriveAssociatedTokenAccount error: {}", e);
            String::new()
        }
    };
    to_cstring(s)
}

/// Shutdown the SDK handle and release its resources.
#[no_mangle]
pub extern "C" fn pollinet_shutdown(handle: i64) {
    common::shutdown_common(handle);
}

// =============================================================================
// Host-driven transport API
// =============================================================================

/// Push inbound bytes received from the radio (GATT write/notify, MPC frame payload).
///
/// # Safety
/// `data` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_push_inbound(
    handle: i64,
    data: *const u8,
    len: usize,
) -> *mut c_char {
    json_out(
        cbytes(data, len, "data").and_then(|d| common::op_push_inbound(handle, d.to_vec())),
    )
}

/// Get the next outbound frame to send, at most `max_len` bytes.
/// Returns a caller-owned buffer (release with [`pollinet_bytes_free`]) and writes
/// its length to `out_len`; returns null (out_len = 0) when the queue is empty.
///
/// # Safety
/// `out_len` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn pollinet_next_outbound(
    handle: i64,
    max_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if !out_len.is_null() {
        *out_len = 0;
    }
    match common::next_outbound_common(handle, max_len) {
        Ok(Some(data)) => {
            let mut buf = data.into_boxed_slice();
            let len = buf.len();
            let ptr = buf.as_mut_ptr();
            std::mem::forget(buf);
            if !out_len.is_null() {
                *out_len = len;
            }
            ptr
        }
        Ok(None) => std::ptr::null_mut(),
        Err(e) => {
            tracing::error!("nextOutbound error: {}", e);
            std::ptr::null_mut()
        }
    }
}

/// Periodic tick for retry/timeout handling. Returns frames as a JSON array of
/// base64 strings inside the envelope.
#[no_mangle]
pub extern "C" fn pollinet_tick(handle: i64, now_ms: u64) -> *mut c_char {
    json_out(common::op_tick(handle, now_ms))
}

/// Get current metrics.
#[no_mangle]
pub extern "C" fn pollinet_metrics(handle: i64) -> *mut c_char {
    json_out(common::op_metrics(handle))
}

/// Clear transaction from buffers.
///
/// # Safety
/// `tx_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_clear_transaction(
    handle: i64,
    tx_id: *const c_char,
) -> *mut c_char {
    json_out(cstr(tx_id, "tx_id").and_then(|id| common::op_clear_transaction(handle, id)))
}

/// Remove all outbound queue fragments that belong to `tx_id`.
///
/// # Safety
/// `tx_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_clear_outbound_transaction(
    handle: i64,
    tx_id: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(tx_id, "tx_id").and_then(|id| common::op_clear_outbound_transaction(handle, id)),
    )
}

// =============================================================================
// Fragmentation API
// =============================================================================

/// Fragment a transaction for radio transmission. `max_payload` <= 0 means default.
///
/// # Safety
/// `tx_bytes` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_fragment(
    handle: i64,
    tx_bytes: *const u8,
    len: usize,
    max_payload: i64,
) -> *mut c_char {
    let max_payload_opt = if max_payload > 0 {
        Some(max_payload as usize)
    } else {
        None
    };
    json_out(
        cbytes(tx_bytes, len, "tx bytes")
            .and_then(|tx| common::op_fragment(handle, tx.to_vec(), max_payload_opt)),
    )
}

/// Reconstruct a transaction from a JSON array of fragment objects with base64 data.
///
/// # Safety
/// `fragments_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_reconstruct_transaction(
    fragments_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(fragments_json, "fragments JSON")
            .and_then(|json| common::op_reconstruct_transaction(json.as_bytes())),
    )
}

/// Get fragmentation statistics for a transaction.
///
/// # Safety
/// `tx_bytes` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_get_fragmentation_stats(
    tx_bytes: *const u8,
    len: usize,
) -> *mut c_char {
    json_out(cbytes(tx_bytes, len, "transaction").and_then(common::op_get_fragmentation_stats))
}

/// Prepare a transaction broadcast (fragments + mesh packets). `handle` is unused
/// (kept for parity with the Android signature).
///
/// # Safety
/// `tx_bytes` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_prepare_broadcast(
    _handle: i64,
    tx_bytes: *const u8,
    len: usize,
) -> *mut c_char {
    json_out(cbytes(tx_bytes, len, "transaction").and_then(common::op_prepare_broadcast))
}

// =============================================================================
// Mesh health
// =============================================================================

/// Get mesh health snapshot.
#[no_mangle]
pub extern "C" fn pollinet_get_health_snapshot(handle: i64) -> *mut c_char {
    json_out(common::op_get_health_snapshot(handle))
}

/// Record peer heartbeat.
///
/// # Safety
/// `peer_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_record_peer_heartbeat(
    handle: i64,
    peer_id: *const c_char,
) -> *mut c_char {
    json_out(cstr(peer_id, "peer_id").and_then(|p| common::op_record_peer_heartbeat(handle, p)))
}

/// Record peer latency measurement (milliseconds).
///
/// # Safety
/// `peer_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_record_peer_latency(
    handle: i64,
    peer_id: *const c_char,
    latency_ms: u32,
) -> *mut c_char {
    json_out(
        cstr(peer_id, "peer_id")
            .and_then(|p| common::op_record_peer_latency(handle, p, latency_ms)),
    )
}

/// Record peer RSSI (dBm, typically negative).
///
/// # Safety
/// `peer_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_record_peer_rssi(
    handle: i64,
    peer_id: *const c_char,
    rssi: i32,
) -> *mut c_char {
    json_out(
        cstr(peer_id, "peer_id").and_then(|p| common::op_record_peer_rssi(handle, p, rssi as i8)),
    )
}

// =============================================================================
// Received (auto-submission) queue
// =============================================================================

/// Push a received transaction into the auto-submission queue.
///
/// # Safety
/// `tx_bytes` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_push_received_transaction(
    handle: i64,
    tx_bytes: *const u8,
    len: usize,
) -> *mut c_char {
    json_out(
        cbytes(tx_bytes, len, "transaction bytes")
            .and_then(|tx| common::op_push_received_transaction(handle, tx.to_vec())),
    )
}

/// Get next received transaction for auto-submission (data = null when empty).
#[no_mangle]
pub extern "C" fn pollinet_next_received_transaction(handle: i64) -> *mut c_char {
    json_out(common::op_next_received_transaction(handle))
}

/// Get count of transactions waiting for auto-submission.
#[no_mangle]
pub extern "C" fn pollinet_get_received_queue_size(handle: i64) -> *mut c_char {
    json_out(common::op_get_received_queue_size(handle))
}

/// Get fragment reassembly info for all incomplete transactions.
#[no_mangle]
pub extern "C" fn pollinet_get_fragment_reassembly_info(handle: i64) -> *mut c_char {
    json_out(common::op_get_fragment_reassembly_info(handle))
}

/// Mark a transaction as successfully submitted (dedup).
///
/// # Safety
/// `tx_bytes` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_mark_transaction_submitted(
    handle: i64,
    tx_bytes: *const u8,
    len: usize,
) -> *mut c_char {
    json_out(
        cbytes(tx_bytes, len, "transaction bytes")
            .and_then(|tx| common::op_mark_transaction_submitted(handle, tx)),
    )
}

/// Clean up old submitted transaction hashes.
#[no_mangle]
pub extern "C" fn pollinet_cleanup_old_submissions(handle: i64) -> *mut c_char {
    json_out(common::op_cleanup_old_submissions(handle))
}

/// Get outbound queue size (non-destructive peek for debugging).
#[no_mangle]
pub extern "C" fn pollinet_get_outbound_queue_size(handle: i64) -> *mut c_char {
    json_out(common::op_get_outbound_queue_size(handle))
}

/// Get outbound queue debug info (non-destructive peek).
#[no_mangle]
pub extern "C" fn pollinet_debug_outbound_queue(handle: i64) -> *mut c_char {
    json_out(common::op_debug_outbound_queue(handle))
}

// =============================================================================
// Queue persistence
// =============================================================================

/// Save all queues to disk.
#[no_mangle]
pub extern "C" fn pollinet_save_queues(handle: i64) -> *mut c_char {
    json_out(common::op_save_queues(handle))
}

/// Trigger auto-save if needed (debounced).
#[no_mangle]
pub extern "C" fn pollinet_auto_save_queues(handle: i64) -> *mut c_char {
    json_out(common::op_auto_save_queues(handle))
}

// =============================================================================
// Queue management
// =============================================================================

/// Push transaction to outbound queue (PushOutboundRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_push_outbound_transaction(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request string")
            .and_then(|req| common::op_push_outbound_transaction(handle, req)),
    )
}

/// Accept and queue a pre-signed external transaction
/// (AcceptExternalTransactionRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_accept_and_queue_external_transaction(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request string")
            .and_then(|req| common::op_accept_and_queue_external_transaction(handle, req)),
    )
}

/// Pop next transaction from outbound queue (data = null when empty).
#[no_mangle]
pub extern "C" fn pollinet_pop_outbound_transaction(handle: i64) -> *mut c_char {
    json_out(common::op_pop_outbound_transaction(handle))
}

/// Add transaction to retry queue (AddToRetryRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_add_to_retry_queue(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request string")
            .and_then(|req| common::op_add_to_retry_queue(handle, req)),
    )
}

/// Pop next ready retry item (data = null when none ready).
#[no_mangle]
pub extern "C" fn pollinet_pop_ready_retry(handle: i64) -> *mut c_char {
    json_out(common::op_pop_ready_retry(handle))
}

/// Get retry queue size.
#[no_mangle]
pub extern "C" fn pollinet_get_retry_queue_size(handle: i64) -> *mut c_char {
    json_out(common::op_get_retry_queue_size(handle))
}

/// Cleanup expired confirmations and retry items.
#[no_mangle]
pub extern "C" fn pollinet_cleanup_expired(handle: i64) -> *mut c_char {
    json_out(common::op_cleanup_expired(handle))
}

/// Confirm delivery of all fragments for `tx_id` to the current peer.
///
/// # Safety
/// `tx_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_confirm_delivered(
    handle: i64,
    tx_id: *const c_char,
) -> *mut c_char {
    json_out(cstr(tx_id, "tx_id").and_then(|id| common::op_confirm_delivered(handle, id)))
}

/// Load the highest-relevance outbound transaction's fragments for sending.
#[no_mangle]
pub extern "C" fn pollinet_load_for_sending(handle: i64) -> *mut c_char {
    json_out(common::op_load_for_sending(handle))
}

/// Purge outbound transactions older than `max_age_secs`.
#[no_mangle]
pub extern "C" fn pollinet_purge_stale_outbound(handle: i64, max_age_secs: i64) -> *mut c_char {
    json_out(common::op_purge_stale_outbound(handle, max_age_secs.max(0) as u64))
}

/// Queue a confirmation for relay back to origin (QueueConfirmationRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_queue_confirmation(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request").and_then(|req| common::op_queue_confirmation(handle, req)),
    )
}

/// Pop next confirmation from queue (data = null when empty).
#[no_mangle]
pub extern "C" fn pollinet_pop_confirmation(handle: i64) -> *mut c_char {
    json_out(common::op_pop_confirmation(handle))
}

/// Cleanup stale fragments from the transaction cache.
#[no_mangle]
pub extern "C" fn pollinet_cleanup_stale_fragments(handle: i64) -> *mut c_char {
    json_out(common::op_cleanup_stale_fragments(handle))
}

/// Relay a received confirmation (ConfirmationFFI JSON; increments hop count).
///
/// # Safety
/// `confirmation_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_relay_confirmation(
    handle: i64,
    confirmation_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(confirmation_json, "confirmation JSON")
            .and_then(|conf| common::op_relay_confirmation(handle, conf)),
    )
}

/// Clear all queues (outbound, retry, confirmation, received) and reassembly buffers.
#[no_mangle]
pub extern "C" fn pollinet_clear_all_queues(handle: i64) -> *mut c_char {
    json_out(common::op_clear_all_queues(handle))
}

// =============================================================================
// Wallet address — reward attribution
// =============================================================================

/// Set the wallet address for this node session. Empty string clears it.
///
/// # Safety
/// `address` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_set_wallet_address(
    handle: i64,
    address: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(address, "address string").and_then(|addr| common::op_set_wallet_address(handle, addr)),
    )
}

/// Get the wallet address currently set for this node session.
#[no_mangle]
pub extern "C" fn pollinet_get_wallet_address(handle: i64) -> *mut c_char {
    json_out(common::op_get_wallet_address(handle))
}

// =============================================================================
// Intent protocol
// =============================================================================

/// Returns the executor PDA address for the pollinet-executor Anchor program.
#[no_mangle]
pub extern "C" fn pollinet_get_executor_pda() -> *mut c_char {
    json_out(common::op_get_executor_pda())
}

/// Build a batch `approve_checked` transaction (CreateApproveTransactionRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_create_approve_transaction(
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request")
            .and_then(|req| common::op_create_approve_transaction(req.as_bytes())),
    )
}

/// Build a batch `revoke` transaction (CreateRevokeTransactionRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_create_revoke_transaction(
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request")
            .and_then(|req| common::op_create_revoke_transaction(req.as_bytes())),
    )
}

/// Serialize an Intent into the canonical 169-byte borsh layout
/// (CreateIntentBytesRequest JSON → base64 bytes + nonce).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_create_intent_bytes(
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request")
            .and_then(|req| common::op_create_intent_bytes(req.as_bytes())),
    )
}

/// Submit a signed intent to pollicore (SubmitIntentRequest JSON).
///
/// # Safety
/// `request_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_submit_intent(
    handle: i64,
    request_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(request_json, "request")
            .and_then(|req| common::op_submit_intent(handle, req.as_bytes())),
    )
}

// =============================================================================
// Subsystem 1 — Density-adaptive rotation
// =============================================================================

/// Record a scan observation for density estimation.
///
/// # Safety
/// `peer_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_record_scan_result(
    handle: i64,
    peer_id: *const c_char,
) -> *mut c_char {
    json_out(cstr(peer_id, "peer_id").and_then(|p| common::op_record_scan_result(handle, p)))
}

/// Recompute and return adaptive session/cooldown parameters.
#[no_mangle]
pub extern "C" fn pollinet_get_adaptive_params(handle: i64) -> *mut c_char {
    json_out(common::op_get_adaptive_params(handle))
}

/// Add `peer_id` to the cooldown list for `cooldown_ms` milliseconds.
///
/// # Safety
/// `peer_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_add_peer_to_cooldown(
    handle: i64,
    peer_id: *const c_char,
    cooldown_ms: u64,
) -> *mut c_char {
    json_out(
        cstr(peer_id, "peer_id")
            .and_then(|p| common::op_add_peer_to_cooldown(handle, p, cooldown_ms)),
    )
}

/// Returns true if `peer_id` is currently in cooldown.
///
/// # Safety
/// `peer_id` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_is_peer_in_cooldown(
    handle: i64,
    peer_id: *const c_char,
) -> *mut c_char {
    json_out(cstr(peer_id, "peer_id").and_then(|p| common::op_is_peer_in_cooldown(handle, p)))
}

/// Sparse-network safety net: expire the oldest cooldown entry early.
#[no_mangle]
pub extern "C" fn pollinet_expire_oldest_cooldown(handle: i64) -> *mut c_char {
    json_out(common::op_expire_oldest_cooldown(handle))
}

/// Log a session telemetry record (SessionTelemetry JSON).
///
/// # Safety
/// `telemetry_json` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_log_session_telemetry(
    handle: i64,
    telemetry_json: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(telemetry_json, "telemetry_json")
            .and_then(|t| common::op_log_session_telemetry(handle, t)),
    )
}

// =============================================================================
// Subsystem 2 — Per-peer materialized queue
// =============================================================================

/// Returns the tx_ids to send to `peer_id_hex` (4-byte compact ID, 8 hex chars).
///
/// # Safety
/// `peer_id_hex` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_outbound_for_peer(
    handle: i64,
    peer_id_hex: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(peer_id_hex, "peer_id_hex").and_then(|p| common::op_outbound_for_peer(handle, p)),
    )
}

/// Drain-conditional delivery confirmation (Subsystem 2). Call ONLY on mutual drain.
///
/// # Safety
/// `tx_id` and `peer_id_hex` must be valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn pollinet_confirm_delivered_by_peer(
    handle: i64,
    tx_id: *const c_char,
    peer_id_hex: *const c_char,
) -> *mut c_char {
    json_out((|| {
        let tx = cstr(tx_id, "tx_id")?;
        let peer = cstr(peer_id_hex, "peer_id_hex")?;
        common::op_confirm_delivered_by_peer(handle, tx, peer)
    })())
}

// =============================================================================
// Subsystem 3 — Confirmation-driven purge
// =============================================================================

/// Ingest a received (or locally generated) MeshConfirmation frame.
///
/// # Safety
/// `confirmation_bytes` must be valid for reads of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn pollinet_ingest_confirmation(
    handle: i64,
    confirmation_bytes: *const u8,
    len: usize,
) -> *mut c_char {
    json_out(
        cbytes(confirmation_bytes, len, "confirmation_bytes")
            .and_then(|raw| common::op_ingest_confirmation(handle, raw)),
    )
}

/// Check if a tx_id_hash (hex) has an active tombstone.
///
/// # Safety
/// `tx_id_hash_hex` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pollinet_is_tombstoned(
    handle: i64,
    tx_id_hash_hex: *const c_char,
) -> *mut c_char {
    json_out(
        cstr(tx_id_hash_hex, "tx_id_hash_hex").and_then(|h| common::op_is_tombstoned(handle, h)),
    )
}

/// Evict expired tombstones and expired cooldowns. Call in the periodic 10s tick.
#[no_mangle]
pub extern "C" fn pollinet_periodic_maintenance(handle: i64) -> *mut c_char {
    json_out(common::op_periodic_maintenance(handle))
}

/// Get the number of active tombstones (diagnostic only).
#[no_mangle]
pub extern "C" fn pollinet_get_tombstone_count(handle: i64) -> *mut c_char {
    json_out(common::op_get_tombstone_count(handle))
}
