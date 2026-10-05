//! Android JNI interface
//!
//! Thin JNI marshalling shims over [`super::common`], which holds the shared
//! FFI operation bodies used by both the Android (JNI) and iOS (C ABI)
//! bindings. Each function here only converts Java types to Rust types, calls
//! the corresponding `common::op_*`, and wraps the result back into a Java
//! type. Keep it that way — behavior belongs in `common.rs` so the platforms
//! cannot drift apart.

#[cfg(feature = "android")]
use jni::objects::{JByteArray, JClass, JString};
#[cfg(feature = "android")]
use jni::sys::{jbyteArray, jint, jlong, jstring};
#[cfg(feature = "android")]
use jni::JNIEnv;

#[cfg(feature = "android")]
use super::common;
#[cfg(feature = "android")]
use super::types::FfiResult;

// Initialize Android logger once
#[cfg(feature = "android")]
use std::sync::Once;

#[cfg(feature = "android")]
static ANDROID_LOGGER_INIT: Once = Once::new();

// =============================================================================
// JNI marshalling helpers
// =============================================================================

/// Initialize the Android logger exactly once — starts silent (Off); the level is
/// applied by `common::init_*` after the config is parsed.
#[cfg(feature = "android")]
fn init_android_logger_once() {
    ANDROID_LOGGER_INIT.call_once(|| {
        #[cfg(feature = "android_logger")]
        {
            android_logger::init_once(
                android_logger::Config::default()
                    .with_max_level(log::LevelFilter::Off)
                    .with_tag("PolliNet-Rust"),
            );
        }
    });
}

#[cfg(feature = "android")]
fn create_result_string(env: &mut JNIEnv, result: Result<String, String>) -> jstring {
    match result {
        Ok(json) => env
            .new_string(json)
            .expect("Failed to create Java string")
            .into_raw(),
        Err(e) => {
            log::error!("❌ FFI error: {}", e);
            let error_response: FfiResult<()> = FfiResult::error("ERR_INTERNAL", e);
            let error_json = serde_json::to_string(&error_response).unwrap_or_else(|_| {
                r#"{"ok":false,"code":"ERR_FATAL","message":"Serialization failed"}"#.to_string()
            });
            env.new_string(error_json)
                .expect("Failed to create error string")
                .into_raw()
        }
    }
}

/// Read a Java byte array into a Vec, with a labeled error.
#[cfg(feature = "android")]
fn read_bytes(env: &JNIEnv, bytes: &JByteArray, label: &str) -> Result<Vec<u8>, String> {
    env.convert_byte_array(bytes)
        .map_err(|e| format!("Failed to read {}: {}", label, e))
}

/// Read a Java string into a Rust String, with a labeled error.
#[cfg(feature = "android")]
fn read_string(env: &mut JNIEnv, s: &JString, label: &str) -> Result<String, String> {
    Ok(env
        .get_string(s)
        .map_err(|e| format!("Failed to read {}: {}", label, e))?
        .into())
}

// =============================================================================
// Initialization and lifecycle
// =============================================================================

/// Initialize the PolliNet SDK.
/// Returns a handle (index) to the initialized transport instance, or -1 on error.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_init(
    env: JNIEnv,
    _class: JClass,
    config_bytes: JByteArray,
) -> jlong {
    init_android_logger_once();
    let result = read_bytes(&env, &config_bytes, "config bytes")
        .and_then(|config| common::init_common(&config));
    match result {
        Ok(handle) => {
            log::info!("🎉 Returning handle {} to Kotlin", handle);
            handle
        }
        Err(e) => {
            log::error!("💥 SDK initialization failed: {}", e);
            -1
        }
    }
}

/// Initialize a Wi-Fi Direct transport handle.
///
/// Mirrors [`Java_xyz_pollinet_sdk_PolliNetFFI_init`] but creates a
/// `HostWifiDirectTransport` (same engine, larger default MTU). BLE-specific FFI
/// calls reject this handle by design.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_initWifiDirect(
    env: JNIEnv,
    _class: JClass,
    config_bytes: JByteArray,
) -> jlong {
    init_android_logger_once();
    let result = read_bytes(&env, &config_bytes, "config bytes")
        .and_then(|config| common::init_wifi_direct_common(&config));
    match result {
        Ok(handle) => handle,
        Err(e) => {
            log::error!("💥 Wi-Fi Direct init failed: {}", e);
            -1
        }
    }
}

/// Initialize a Wi-Fi Direct handle that **shares the engine** of an existing BLE
/// handle, giving both radios one dedup set and queue. Returns -1 on invalid handle.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_initWifiDirectSharing(
    _env: JNIEnv,
    _class: JClass,
    ble_handle: jlong,
) -> jlong {
    match common::init_wifi_direct_sharing_common(ble_handle) {
        Ok(handle) => handle,
        Err(e) => {
            log::error!("💥 initWifiDirectSharing failed: {}", e);
            -1
        }
    }
}

/// Return the transport kind for a handle ("BLE" | "WIFI_DIRECT"), or "" if invalid.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_transportKind(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    env.new_string(common::transport_kind_common(handle))
        .expect("Failed to create Java string")
        .into_raw()
}

/// Get SDK version
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    env.new_string(env!("CARGO_PKG_VERSION"))
        .expect("Failed to create Java string")
        .into_raw()
}

/// Return the pollicore base URL baked in at compile time from POLLICORE_URL env var.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getPolliCoreUrl(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    env.new_string(option_env!("POLLICORE_URL").unwrap_or(""))
        .expect("Failed to create Java string")
        .into_raw()
}

/// Derive the Associated Token Account (ATA) address for a given owner wallet and
/// token mint. Stateless — returns the base58 ATA, or an empty string on bad input.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_deriveAssociatedTokenAccount(
    mut env: JNIEnv,
    _class: JClass,
    owner_j: JString,
    mint_j: JString,
) -> jstring {
    let result = (|| {
        let owner = read_string(&mut env, &owner_j, "owner")?;
        let mint = read_string(&mut env, &mint_j, "mint")?;
        common::derive_associated_token_account_common(&owner, &mint)
    })();
    let s = match result {
        Ok(addr) => addr,
        Err(e) => {
            log::error!("❌ deriveAssociatedTokenAccount error: {}", e);
            String::new()
        }
    };
    env.new_string(s)
        .expect("Failed to create Java string")
        .into_raw()
}

/// Shutdown the SDK and release resources
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_shutdown(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    common::shutdown_common(handle);
}

// =============================================================================
// Host-driven transport API
// =============================================================================

/// Push inbound data from GATT characteristic
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_pushInbound(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    data: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &data, "data").and_then(|d| common::op_push_inbound(handle, d));
    create_result_string(&mut env, result)
}

/// Get next outbound frame to send
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_nextOutbound(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    max_len: jlong,
) -> jbyteArray {
    match common::next_outbound_common(handle, max_len as usize) {
        Ok(Some(data)) => env
            .byte_array_from_slice(&data)
            .expect("Failed to create byte array")
            .into_raw(),
        Ok(None) => std::ptr::null_mut(),
        Err(e) => {
            tracing::error!("nextOutbound error: {}", e);
            std::ptr::null_mut()
        }
    }
}

/// Periodic tick for retry/timeout handling
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_tick(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    now_ms: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_tick(handle, now_ms as u64))
}

/// Get current metrics
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_metrics(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_metrics(handle))
}

/// Clear transaction from buffers
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_clearTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id: JString,
) -> jstring {
    let result = read_string(&mut env, &tx_id, "tx_id")
        .and_then(|id| common::op_clear_transaction(handle, &id));
    create_result_string(&mut env, result)
}

/// Remove all outbound queue fragments that belong to `tx_id`.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_clearOutboundTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id: JString,
) -> jstring {
    let result = read_string(&mut env, &tx_id, "tx_id")
        .and_then(|id| common::op_clear_outbound_transaction(handle, &id));
    create_result_string(&mut env, result)
}

// =============================================================================
// Fragmentation API (M6)
// =============================================================================

/// Fragment a transaction for BLE transmission.
/// Optionally accepts max_payload (MTU - 10) for MTU-aware fragmentation.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_fragment(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_bytes: JByteArray,
    max_payload: jlong,
) -> jstring {
    let max_payload_opt = if max_payload > 0 {
        Some(max_payload as usize)
    } else {
        None
    };
    let result = read_bytes(&env, &tx_bytes, "tx bytes")
        .and_then(|tx| common::op_fragment(handle, tx, max_payload_opt));
    create_result_string(&mut env, result)
}

/// Reconstruct a transaction from fragments.
/// Takes JSON array of fragment objects with base64 data.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_reconstructTransaction(
    mut env: JNIEnv,
    _class: JClass,
    fragments_json: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &fragments_json, "fragments JSON")
        .and_then(|json| common::op_reconstruct_transaction(&json));
    create_result_string(&mut env, result)
}

/// Get fragmentation statistics for a transaction
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getFragmentationStats(
    mut env: JNIEnv,
    _class: JClass,
    transaction_bytes: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &transaction_bytes, "transaction")
        .and_then(|tx| common::op_get_fragmentation_stats(&tx));
    create_result_string(&mut env, result)
}

// =============================================================================
// Transaction Broadcasting
// =============================================================================

/// Prepare a transaction broadcast (fragments it and returns fragments with packets)
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_prepareBroadcast(
    mut env: JNIEnv,
    _class: JClass,
    _handle: jlong,
    transaction_bytes: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &transaction_bytes, "transaction")
        .and_then(|tx| common::op_prepare_broadcast(&tx));
    create_result_string(&mut env, result)
}

/// Get mesh health snapshot
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getHealthSnapshot(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_health_snapshot(handle))
}

/// Record peer heartbeat
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_recordPeerHeartbeat(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
) -> jstring {
    let result = read_string(&mut env, &peer_id, "peer_id")
        .and_then(|p| common::op_record_peer_heartbeat(handle, &p));
    create_result_string(&mut env, result)
}

/// Record peer latency measurement
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_recordPeerLatency(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
    latency_ms: jint,
) -> jstring {
    let result = read_string(&mut env, &peer_id, "peer_id")
        .and_then(|p| common::op_record_peer_latency(handle, &p, latency_ms as u32));
    create_result_string(&mut env, result)
}

/// Record peer RSSI (signal strength)
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_recordPeerRssi(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
    rssi: jint,
) -> jstring {
    let result = read_string(&mut env, &peer_id, "peer_id")
        .and_then(|p| common::op_record_peer_rssi(handle, &p, rssi as i8));
    create_result_string(&mut env, result)
}

/// Push a received transaction into the auto-submission queue
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_pushReceivedTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    transaction_bytes: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &transaction_bytes, "transaction bytes")
        .and_then(|tx| common::op_push_received_transaction(handle, tx));
    create_result_string(&mut env, result)
}

/// Get next received transaction for auto-submission
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_nextReceivedTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_next_received_transaction(handle))
}

/// Get count of transactions waiting for auto-submission
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getReceivedQueueSize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_received_queue_size(handle))
}

/// Get fragment reassembly info for all incomplete transactions
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getFragmentReassemblyInfo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_fragment_reassembly_info(handle))
}

/// Mark a transaction as successfully submitted
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_markTransactionSubmitted(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    transaction_bytes: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &transaction_bytes, "transaction bytes")
        .and_then(|tx| common::op_mark_transaction_submitted(handle, &tx));
    create_result_string(&mut env, result)
}

/// Clean up old submitted transaction hashes
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_cleanupOldSubmissions(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_cleanup_old_submissions(handle))
}

/// Get outbound queue size (non-destructive peek for debugging)
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getOutboundQueueSize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_outbound_queue_size(handle))
}

/// Get outbound queue debug info (non-destructive peek)
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_debugOutboundQueue(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_debug_outbound_queue(handle))
}

// =============================================================================
// Queue Persistence FFI Functions (Phase 5)
// =============================================================================

/// Save all queues to disk
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_saveQueues(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_save_queues(handle))
}

/// Trigger auto-save if needed (debounced)
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_autoSaveQueues(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_auto_save_queues(handle))
}

// =============================================================================
// Queue Management FFI Functions (Phase 2)
// =============================================================================

/// Push transaction to outbound queue
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_pushOutboundTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JString,
) -> jstring {
    let result = read_string(&mut env, &request_json, "request string")
        .and_then(|req| common::op_push_outbound_transaction(handle, &req));
    create_result_string(&mut env, result)
}

/// Accept and queue a pre-signed transaction from external partners
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_acceptAndQueueExternalTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JString,
) -> jstring {
    let result = read_string(&mut env, &request_json, "request string")
        .and_then(|req| common::op_accept_and_queue_external_transaction(handle, &req));
    create_result_string(&mut env, result)
}

/// Pop next transaction from outbound queue
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_popOutboundTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_pop_outbound_transaction(handle))
}

/// Add transaction to retry queue
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_addToRetryQueue(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JString,
) -> jstring {
    let result = read_string(&mut env, &request_json, "request string")
        .and_then(|req| common::op_add_to_retry_queue(handle, &req));
    create_result_string(&mut env, result)
}

/// Pop next ready retry item
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_popReadyRetry(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_pop_ready_retry(handle))
}

/// Get retry queue size
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getRetryQueueSize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_retry_queue_size(handle))
}

/// Cleanup expired confirmations and retry items
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_cleanupExpired(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_cleanup_expired(handle))
}

/// Confirm that all fragments for `tx_id` were delivered to the current peer.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_confirmDelivered(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id_j: JString,
) -> jstring {
    let result = read_string(&mut env, &tx_id_j, "tx_id")
        .and_then(|id| common::op_confirm_delivered(handle, &id));
    create_result_string(&mut env, result)
}

/// Peek at the highest-relevance transaction and load its fragments for sending.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_loadForSending(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_load_for_sending(handle))
}

/// Purge outbound transactions older than max_age_secs from all priority queues.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_purgeStaleOutbound(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    max_age_secs: jlong,
) -> jstring {
    create_result_string(
        &mut env,
        common::op_purge_stale_outbound(handle, max_age_secs.max(0) as u64),
    )
}

/// Queue a confirmation for relay back to origin device
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_queueConfirmation(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JString,
) -> jstring {
    let result = read_string(&mut env, &request_json, "request")
        .and_then(|req| common::op_queue_confirmation(handle, &req));
    create_result_string(&mut env, result)
}

/// Pop next confirmation from queue
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_popConfirmation(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_pop_confirmation(handle))
}

/// Cleanup stale fragments from the transaction cache
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_cleanupStaleFragments(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_cleanup_stale_fragments(handle))
}

/// Relay a received confirmation (increment hop count and re-queue for relay)
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_relayConfirmation(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    confirmation_json: JString,
) -> jstring {
    let result = read_string(&mut env, &confirmation_json, "confirmation JSON")
        .and_then(|conf| common::op_relay_confirmation(handle, &conf));
    create_result_string(&mut env, result)
}

/// Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_clearAllQueues(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_clear_all_queues(handle))
}

// =============================================================================
// Wallet address — reward attribution
// =============================================================================

/// Set the wallet address for this node session.
/// Pass an empty string to clear a previously-set address.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_setWalletAddress(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    address: JString,
) -> jstring {
    let result = read_string(&mut env, &address, "address string")
        .and_then(|addr| common::op_set_wallet_address(handle, &addr));
    create_result_string(&mut env, result)
}

/// Get the wallet address currently set for this node session.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getWalletAddress(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_wallet_address(handle))
}

// =============================================================================
// Intent protocol — stateless helpers (no SDK transport handle needed)
// =============================================================================

/// Returns the executor PDA address for the pollinet-executor Anchor program.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getExecutorPda(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    create_result_string(&mut env, common::op_get_executor_pda())
}

/// Builds a single unsigned transaction containing one `approve_checked` instruction
/// per entry in the request.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createApproveTransaction(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &request_json, "request bytes")
        .and_then(|req| common::op_create_approve_transaction(&req));
    create_result_string(&mut env, result)
}

/// Builds a single unsigned transaction with one `revoke` instruction per token account.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createRevokeTransaction(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &request_json, "request bytes")
        .and_then(|req| common::op_create_revoke_transaction(&req));
    create_result_string(&mut env, result)
}

/// Serializes an Intent into the canonical 169-byte borsh layout (base64).
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createIntentBytes(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &request_json, "request bytes")
        .and_then(|req| common::op_create_intent_bytes(&req));
    create_result_string(&mut env, result)
}

/// Submit a signed intent to pollicore.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_submitIntent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &request_json, "request bytes")
        .and_then(|req| common::op_submit_intent(handle, &req));
    create_result_string(&mut env, result)
}

// =============================================================================
// Subsystem 1 — Density-adaptive rotation
// =============================================================================

/// Record a scan observation for density estimation.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_recordScanResult(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
) -> jstring {
    let result = read_string(&mut env, &peer_id, "peer_id")
        .and_then(|p| common::op_record_scan_result(handle, &p));
    create_result_string(&mut env, result)
}

/// Recompute and return adaptive BLE session/cooldown parameters.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getAdaptiveParams(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_adaptive_params(handle))
}

/// Add `peer_id` to the cooldown list for `cooldown_ms` milliseconds.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_addPeerToCooldown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
    cooldown_ms: jlong,
) -> jstring {
    let result = read_string(&mut env, &peer_id, "peer_id")
        .and_then(|p| common::op_add_peer_to_cooldown(handle, &p, cooldown_ms as u64));
    create_result_string(&mut env, result)
}

/// Returns true if `peer_id` is currently in cooldown.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_isPeerInCooldown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
) -> jstring {
    let result = read_string(&mut env, &peer_id, "peer_id")
        .and_then(|p| common::op_is_peer_in_cooldown(handle, &p));
    create_result_string(&mut env, result)
}

/// Sparse-network safety net: expire the oldest cooldown entry early.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_expireOldestCooldown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_expire_oldest_cooldown(handle))
}

/// Log a session telemetry record.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_logSessionTelemetry(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    telemetry_json: JString,
) -> jstring {
    let result = read_string(&mut env, &telemetry_json, "telemetry_json")
        .and_then(|t| common::op_log_session_telemetry(handle, &t));
    create_result_string(&mut env, result)
}

// =============================================================================
// Subsystem 2 — Per-peer materialized queue
// =============================================================================

/// Returns the list of tx_ids that should be sent to `peer_id` (4-byte hex compact ID).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_outboundForPeer(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id_hex: JString,
) -> jstring {
    let result = read_string(&mut env, &peer_id_hex, "peer_id_hex")
        .and_then(|p| common::op_outbound_for_peer(handle, &p));
    create_result_string(&mut env, result)
}

/// Drain-conditional delivery confirmation (Subsystem 2). Call ONLY on mutual drain.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_confirmDeliveredByPeer(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id: JString,
    peer_id_hex: JString,
) -> jstring {
    let result = (|| {
        let tx = read_string(&mut env, &tx_id, "tx_id")?;
        let peer = read_string(&mut env, &peer_id_hex, "peer_id_hex")?;
        common::op_confirm_delivered_by_peer(handle, &tx, &peer)
    })();
    create_result_string(&mut env, result)
}

// =============================================================================
// Subsystem 3 — Confirmation-driven purge
// =============================================================================

/// Ingest a received (or locally generated) confirmation.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_ingestConfirmation(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    confirmation_bytes: JByteArray,
) -> jstring {
    let result = read_bytes(&env, &confirmation_bytes, "confirmation_bytes")
        .and_then(|raw| common::op_ingest_confirmation(handle, &raw));
    create_result_string(&mut env, result)
}

/// Check if a tx_id_hash (hex) has an active tombstone.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_isTombstoned(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id_hash_hex: JString,
) -> jstring {
    let result = read_string(&mut env, &tx_id_hash_hex, "tx_id_hash_hex")
        .and_then(|h| common::op_is_tombstoned(handle, &h));
    create_result_string(&mut env, result)
}

/// Evict expired tombstones and expired cooldowns. Call in the periodic 10s tick.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_periodicMaintenance(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_periodic_maintenance(handle))
}

/// Get the number of active tombstones (diagnostic only).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getTombstoneCount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    create_result_string(&mut env, common::op_get_tombstone_count(handle))
}
