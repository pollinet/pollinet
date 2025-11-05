//! Android JNI interface
//! 
//! This module provides the JNI bindings that Kotlin can call from Android.
//! All functions follow the JNI naming convention and handle marshalling
//! between Java types and Rust types.

#[cfg(feature = "android")]
use jni::objects::{JByteArray, JClass, JString};
#[cfg(feature = "android")]
use jni::sys::{jbyteArray, jlong, jstring};
#[cfg(feature = "android")]
use jni::JNIEnv;
#[cfg(feature = "android")]
use std::sync::Arc;
#[cfg(feature = "android")]
use parking_lot::Mutex;
#[cfg(feature = "android")]
use std::str::FromStr;

use super::runtime;
use super::transport::HostBleTransport;
use super::types::*;
use crate::PolliNetSDK;

#[cfg(feature = "android")]
use solana_sdk::pubkey::Pubkey;

// Global state for SDK instances
#[cfg(feature = "android")]
lazy_static::lazy_static! {
    static ref TRANSPORTS: Arc<Mutex<Vec<Arc<HostBleTransport>>>> = Arc::new(Mutex::new(Vec::new()));
    static ref SDK_INSTANCES: Arc<Mutex<Vec<Arc<PolliNetSDK>>>> = Arc::new(Mutex::new(Vec::new()));
}

// =============================================================================
// Initialization and lifecycle
// =============================================================================

/// Initialize the PolliNet SDK
/// Returns a handle (index) to the initialized transport instance
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_init(
    env: JNIEnv,
    _class: JClass,
    config_bytes: JByteArray,
) -> jlong {
    let result: Result<jlong, String> = (|| {
        // Initialize runtime if needed
        runtime::init_runtime().ok(); // Ignore if already initialized

        // Parse config
        let config_data: Vec<u8> = env
            .convert_byte_array(&config_bytes)
            .map_err(|e| format!("Failed to read config bytes: {}", e))?;

        let config: SdkConfig = serde_json::from_slice(&config_data)
            .map_err(|e| format!("Failed to parse config: {}", e))?;

        // Initialize logging if requested
        if config.enable_logging {
            let _ = tracing_subscriber::fmt()
                .with_max_level(parse_log_level(config.log_level.as_deref()))
                .try_init();
        }

        // Create transport instance
        let transport = runtime::block_on(async {
            if let Some(rpc_url) = &config.rpc_url {
                HostBleTransport::new_with_rpc(rpc_url).await
            } else {
                HostBleTransport::new().await
            }
        })?;

        let transport_arc = Arc::new(transport);
        let mut transports = TRANSPORTS.lock();
        transports.push(transport_arc);
        let handle = (transports.len() - 1) as jlong;

        tracing::info!("✅ PolliNet SDK initialized with handle {}", handle);
        Ok(handle)
    })();

    match result {
        Ok(handle) => handle,
        Err(e) => {
            tracing::error!("Failed to initialize SDK: {}", e);
            -1 // Error handle
        }
    }
}

/// Get SDK version
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let version = env!("CARGO_PKG_VERSION");
    env.new_string(version)
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
    let transports = TRANSPORTS.lock();
    if handle >= 0 && (handle as usize) < transports.len() {
        // Just mark as None; we'll keep the Vec stable for other handles
        tracing::info!("🛑 Shutting down SDK handle {}", handle);
    }
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
    let result = (|| {
        let transport = get_transport(handle)?;
        let data_vec: Vec<u8> = env
            .convert_byte_array(&data)
            .map_err(|e| format!("Failed to read data: {}", e))?;

        transport.push_inbound(data_vec)?;
        
        let response: FfiResult<()> = FfiResult::success(());
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result: Result<Option<Vec<u8>>, String> = (|| {
        let transport = get_transport(handle)?;
        Ok(transport.next_outbound(max_len as usize))
    })();

    match result {
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
    let result = (|| {
        let transport = get_transport(handle)?;
        let frames = transport.tick(now_ms as u64);
        
        // Encode frames as JSON array of base64 strings
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        let encoded: Vec<String> = frames.iter().map(|f| BASE64.encode(f)).collect();
        
        let response: FfiResult<Vec<String>> = FfiResult::success(encoded);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get current metrics
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_metrics(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result = (|| {
        let transport = get_transport(handle)?;
        let metrics = transport.metrics();
        
        let response: FfiResult<MetricsSnapshot> = FfiResult::success(metrics);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
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
    let result = (|| {
        let transport = get_transport(handle)?;
        let tx_id_str: String = env
            .get_string(&tx_id)
            .map_err(|e| format!("Failed to read tx_id: {}", e))?
            .into();

        transport.clear_transaction(&tx_id_str);
        
        let response: FfiResult<()> = FfiResult::success(());
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

// =============================================================================
// Transaction builders (M4)
// =============================================================================

/// Create unsigned SOL transfer transaction
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createUnsignedTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JByteArray,
) -> jstring {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request: {}", e))?;

        let request: CreateUnsignedTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Build unsigned transaction
        let base64_tx = runtime::block_on(async {
            transport
                .transaction_service()
                .create_unsigned_transaction(
                    &request.sender,
                    &request.recipient,
                    &request.fee_payer,
                    request.amount,
                    &request.nonce_account,
                )
                .await
        })
        .map_err(|e| format!("Failed to create transaction: {}", e))?;
        
        tracing::info!("✅ Created unsigned transaction (base64 length: {})", base64_tx.len());
        
        let response: FfiResult<String> = FfiResult::success(base64_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Create unsigned SPL token transfer transaction
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createUnsignedSplTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JByteArray,
) -> jstring {
    let result = (|| {
        let transport = get_transport(handle)?;
        let request_data: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request: {}", e))?;

        let request: CreateUnsignedSplTransactionRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

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
                    &request.nonce_account,
                )
                .await
        })
        .map_err(|e| format!("Failed to create SPL transaction: {}", e))?;
        
        tracing::info!("✅ Created unsigned SPL transaction (base64 length: {})", base64_tx.len());
        
        let response: FfiResult<String> = FfiResult::success(base64_tx);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

// =============================================================================
// Signature helpers (M5)
// =============================================================================

/// Prepare sign payload - Extract message bytes that need to be signed
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_prepareSignPayload(
    mut env: JNIEnv,
    _class: JClass,
    _handle: jlong,
    base64_tx: JString,
) -> jbyteArray {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    
    let result: Result<Vec<u8>, String> = (|| {
        let tx_str: String = env
            .get_string(&base64_tx)
            .map_err(|e| format!("Failed to read base64 tx: {}", e))?
            .into();

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
        Ok(payload) => env
            .byte_array_from_slice(&payload)
            .expect("Failed to create byte array")
            .into_raw(),
        Err(e) => {
            tracing::error!("prepareSignPayload error: {}", e);
            std::ptr::null_mut()
        }
    }
}

/// Apply signature to unsigned transaction
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_applySignature(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    base64_tx: JString,
    signer_pubkey: JString,
    signature_bytes: JByteArray,
) -> jstring {
    let result = (|| {
        let transport = get_transport(handle)?;
        
        let tx_str: String = env
            .get_string(&base64_tx)
            .map_err(|e| format!("Failed to read base64 tx: {}", e))?
            .into();

        let pubkey_str: String = env
            .get_string(&signer_pubkey)
            .map_err(|e| format!("Failed to read signer pubkey: {}", e))?
            .into();

        let sig_bytes: Vec<u8> = env
            .convert_byte_array(&signature_bytes)
            .map_err(|e| format!("Failed to read signature bytes: {}", e))?;

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

    create_result_string(&mut env, result)
}

/// Verify and serialize transaction for submission
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_verifyAndSerialize(
    mut env: JNIEnv,
    _class: JClass,
    _handle: jlong,
    base64_tx: JString,
) -> jstring {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    
    let result = (|| {
        let tx_str: String = env
            .get_string(&base64_tx)
            .map_err(|e| format!("Failed to read base64 tx: {}", e))?
            .into();

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

    create_result_string(&mut env, result)
}

// =============================================================================
// Fragmentation API (M6)
// =============================================================================

/// Fragment a transaction for BLE transmission
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_fragment(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_bytes: JByteArray,
) -> jstring {
    let result = (|| {
        let transport = get_transport(handle)?;
        let tx_data: Vec<u8> = env
            .convert_byte_array(&tx_bytes)
            .map_err(|e| format!("Failed to read tx bytes: {}", e))?;

        let fragments = transport.queue_transaction(tx_data)?;
        
        let fragment_list = FragmentList { fragments };
        let response: FfiResult<FragmentList> = FfiResult::success(fragment_list);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

// =============================================================================
// Helper functions
// =============================================================================

#[cfg(feature = "android")]
fn get_transport(handle: jlong) -> Result<Arc<HostBleTransport>, String> {
    let transports = TRANSPORTS.lock();
    if handle < 0 || handle as usize >= transports.len() {
        return Err(format!("Invalid handle: {}", handle));
    }
    Ok(transports[handle as usize].clone())
}

#[cfg(feature = "android")]
fn create_result_string(env: &mut JNIEnv, result: Result<String, String>) -> jstring {
    match result {
        Ok(json) => env
            .new_string(json)
            .expect("Failed to create Java string")
            .into_raw(),
        Err(e) => {
            let error_response: FfiResult<()> =
                FfiResult::error("ERR_INTERNAL", e);
            let error_json = serde_json::to_string(&error_response)
                .unwrap_or_else(|_| r#"{"ok":false,"code":"ERR_FATAL","message":"Serialization failed"}"#.to_string());
            env.new_string(error_json)
                .expect("Failed to create error string")
                .into_raw()
        }
    }
}

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

