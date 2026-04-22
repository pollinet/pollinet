//! Android JNI interface
//!
//! This module provides the JNI bindings that Kotlin can call from Android.
//! All functions follow the JNI naming convention and handle marshalling
//! between Java types and Rust types.

#![allow(deprecated)]

#[cfg(feature = "android")]
use jni::objects::{JByteArray, JClass, JString};
#[cfg(feature = "android")]
use jni::sys::{jbyteArray, jint, jlong, jstring};
#[cfg(feature = "android")]
use jni::JNIEnv;
#[cfg(feature = "android")]
use parking_lot::Mutex;
#[cfg(feature = "android")]
use std::str::FromStr;
#[cfg(feature = "android")]
use std::sync::Arc;

use super::runtime;
use super::transport::HostBleTransport;
use super::types::*;

#[cfg(feature = "android")]
use solana_sdk::pubkey::Pubkey;

#[cfg(feature = "android")]
use log::{error, info};

// Initialize Android logger once
#[cfg(feature = "android")]
use std::sync::Once;

#[cfg(feature = "android")]
static ANDROID_LOGGER_INIT: Once = Once::new();

// Global state for transport instances
#[cfg(feature = "android")]
lazy_static::lazy_static! {
    static ref TRANSPORTS: Arc<Mutex<Vec<Option<Arc<HostBleTransport>>>>> = Arc::new(Mutex::new(Vec::new()));
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
    // Initialize Android logger once — starts silent (Off); level is set after config is parsed.
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

    let result: Result<jlong, String> = (|| {
        // Parse config before touching any logging so the enable_logging flag controls everything.
        let config_data: Vec<u8> = env.convert_byte_array(&config_bytes)
            .map_err(|e| format!("Failed to read config bytes: {}", e))?;

        let config: SdkConfig = serde_json::from_slice(&config_data)
            .map_err(|e| format!("Failed to parse config: {}", e))?;

        // Apply log level — Off when enableLogging is false, desired level otherwise.
        // log::set_max_level is the global filter gate; setting it to Off prevents all
        // log!/tracing! calls from reaching logcat even if a subscriber is registered.
        if config.enable_logging {
            let tracing_level = parse_log_level(config.log_level.as_deref());
            let log_level = match tracing_level {
                tracing::Level::ERROR => log::LevelFilter::Error,
                tracing::Level::WARN  => log::LevelFilter::Warn,
                tracing::Level::INFO  => log::LevelFilter::Info,
                tracing::Level::DEBUG => log::LevelFilter::Debug,
                tracing::Level::TRACE => log::LevelFilter::Trace,
            };
            log::set_max_level(log_level);
            let _ = tracing_subscriber::fmt().with_max_level(tracing_level).try_init();
            info!("🔧 PolliNet-Rust logging enabled (level: {:?})", tracing_level);
        } else {
            log::set_max_level(log::LevelFilter::Off);
        }

        info!("📱 FFI init — RPC: {:?}", config.rpc_url);

        // Initialize runtime if needed
        match runtime::init_runtime() {
            Ok(_) => info!("✅ Runtime initialized"),
            Err(e) if e.contains("already initialized") => {}
            Err(e) => return Err(format!("Failed to initialize runtime: {}", e)),
        }

        info!("Step 5: Creating transport...");

        // Create transport instance
        let mut transport = runtime::block_on(async {
            if let Some(rpc_url) = &config.rpc_url {
                info!("Creating transport with RPC: {}", rpc_url);
                HostBleTransport::new_with_rpc(rpc_url).await
            } else {
                info!("Creating transport without RPC");
                HostBleTransport::new().await
            }
        })
        .map_err(|e| {
            error!("❌ Transport creation failed: {}", e);
            e
        })?;

        // Set secure storage if directory provided
        if let Some(storage_dir) = &config.storage_directory {
            info!("Step 5b: Setting up secure storage at: {}", storage_dir);
            transport.set_secure_storage(storage_dir, config.encryption_key.clone()).map_err(|e| {
                error!("❌ Failed to set secure storage: {}", e);
                e
            })?;
            info!("✅ Secure storage configured");

            // Phase 5: Set queue storage directory (stored on transport, no env var mutation)
            let queue_storage_dir = format!("{}/queues", storage_dir);
            transport.set_queue_storage_dir(queue_storage_dir.clone());
            info!("✅ Queue persistence enabled at: {}", queue_storage_dir);
        } else {
            info!("ℹ️  No storage directory provided - bundle persistence disabled");
        }

        // Resolve pollicore URL: baked-in at compile time from .env / POLLICORE_URL env var
        let pollicore_url: Option<&str> = option_env!("POLLICORE_URL");
        if let Some(url) = pollicore_url {
            transport.set_pollicore_url(Some(url.to_string()));
            info!("✅ Pollicore URL (compile-time): {}", url);
        } else {
            info!("⚠️  POLLICORE_URL not set at compile time — submitIntent will fail");
        }

        // Store wallet address if provided in config
        if let Some(ref addr) = config.wallet_address {
            transport.set_wallet_address(Some(addr.clone()));
            info!("✅ Wallet address set: {}", addr);
        } else {
            info!("ℹ️  No wallet address provided — rewards will not be attributed until one is set");
        }

        info!("Step 6: Storing transport...");

        let transport_arc = Arc::new(transport);
        let mut transports = TRANSPORTS.lock();
        transports.push(Some(transport_arc));
        let handle = (transports.len() - 1) as jlong;

        info!(
            "✅ PolliNet SDK initialized successfully with handle {}",
            handle
        );
        Ok(handle)
    })();

    match result {
        Ok(handle) => {
            info!("🎉 Returning handle {} to Kotlin", handle);
            handle
        }
        Err(e) => {
            error!("💥 SDK initialization failed: {}", e);
            error!("Returning error handle -1");
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

/// Return the pollicore base URL baked in at compile time from POLLICORE_URL env var.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getPolliCoreUrl(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let url = option_env!("POLLICORE_URL").unwrap_or("");
    env.new_string(url)
        .expect("Failed to create Java string")
        .into_raw()
}

/// Derive the Associated Token Account (ATA) address for a given owner wallet and token mint.
/// Stateless — no SDK handle required.
/// Returns the base58 ATA address, or an empty string on invalid input.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_deriveAssociatedTokenAccount(
    mut env: JNIEnv,
    _class: JClass,
    owner_j: JString,
    mint_j: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let owner_str: String = env.get_string(&owner_j).map_err(|e| e.to_string())?.into();
        let mint_str: String  = env.get_string(&mint_j).map_err(|e| e.to_string())?.into();
        let owner = Pubkey::from_str(&owner_str).map_err(|e| format!("Invalid owner: {}", e))?;
        let mint  = Pubkey::from_str(&mint_str).map_err(|e| format!("Invalid mint: {}", e))?;
        let ata = spl_associated_token_account::get_associated_token_address(&owner, &mint);
        Ok(ata.to_string())
    })();
    let s = match result {
        Ok(addr) => addr,
        Err(e) => {
            error!("❌ deriveAssociatedTokenAccount error: {}", e);
            String::new()
        }
    };
    env.new_string(s).expect("Failed to create Java string").into_raw()
}

/// Shutdown the SDK and release resources
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_shutdown(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let mut transports = TRANSPORTS.lock();
    if handle >= 0 && (handle as usize) < transports.len() {
        transports[handle as usize] = None;
        tracing::info!("🛑 SDK handle {} shut down and invalidated", handle);
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

        log::debug!("📡 pushInbound handle={} bytes={}", handle, data_vec.len());
        transport.push_inbound(data_vec)?;
        log::debug!("✅ pushInbound queued successfully");

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

/// Remove all outbound queue fragments that belong to `tx_id`.
/// Must be called when a BLE confirmation arrives (success or failure) so the
/// originating device stops re-broadcasting a transaction already handled by a
/// relay peer.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_clearOutboundTransaction(
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

        let removed = transport.clear_outbound_for_tx(&tx_id_str);

        #[derive(serde::Serialize)]
        struct Out { removed: usize }
        let response: FfiResult<Out> = FfiResult::success(Out { removed });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

// =============================================================================
// Fragmentation API (M6)
// =============================================================================

/// Fragment a transaction for BLE transmission
///
/// Optionally accepts max_payload (MTU - 10) for MTU-aware fragmentation
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_fragment(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_bytes: JByteArray,
    max_payload: jlong,
) -> jstring {
    let result = (|| {
        let transport = get_transport(handle)?;
        let tx_data: Vec<u8> = env
            .convert_byte_array(&tx_bytes)
            .map_err(|e| format!("Failed to read tx bytes: {}", e))?;

        let max_payload_opt = if max_payload > 0 { Some(max_payload as usize) } else { None };
        log::info!("✂️  fragment handle={} input_bytes={} max_payload={:?}",
            handle, tx_data.len(), max_payload_opt);

        let fragments = transport.queue_transaction(tx_data, max_payload_opt)?;

        let total_fragment_bytes: usize = fragments.iter().map(|f| f.data.len()).sum();
        log::info!("✅ fragment → {} fragments, {} total payload bytes",
            fragments.len(), total_fragment_bytes);

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
    transports[handle as usize]
        .clone()
        .ok_or_else(|| format!("Handle {} has been shut down", handle))
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


/// Reconstruct a transaction from fragments
/// Takes JSON array of fragment objects with base64 data
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_reconstructTransaction(
    mut env: JNIEnv,
    _class: JClass,
    fragments_json: JByteArray,
) -> jstring {
    let result = (|| -> Result<String, String> {
        tracing::info!("🔗 FFI reconstructTransaction called");

        let json_data: Vec<u8> = env
            .convert_byte_array(&fragments_json)
            .map_err(|e| format!("Failed to read fragments JSON: {}", e))?;

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
        let fragments: Vec<crate::ble::mesh::TransactionFragment> = fragment_data
            .iter()
            .map(|f| {
                let mut tx_id = [0u8; 32];
                let tx_id_bytes = hex::decode(&f.transaction_id)
                    .map_err(|e| format!("Invalid transaction ID: {}", e))?;
                tx_id.copy_from_slice(&tx_id_bytes);

                let data = base64::decode(&f.data_base64)
                    .map_err(|e| format!("Invalid fragment data: {}", e))?;

                Ok(crate::ble::mesh::TransactionFragment {
                    transaction_id: tx_id,
                    fragment_index: f.fragment_index,
                    total_fragments: f.total_fragments,
                    data,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        // Reconstruct the transaction
        let reconstructed = crate::ble::reconstruct_transaction(&fragments)
            .map_err(|e| format!("Reconstruction failed: {}", e))?;

        tracing::info!(
            "✅ Reconstructed transaction: {} bytes",
            reconstructed.len()
        );

        // Return base64-encoded transaction
        let tx_base64 = base64::encode(&reconstructed);

        let response: FfiResult<String> = FfiResult::success(tx_base64);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result = (|| -> Result<String, String> {
        tracing::info!("📊 FFI getFragmentationStats called");

        let tx_bytes: Vec<u8> = env
            .convert_byte_array(&transaction_bytes)
            .map_err(|e| format!("Failed to read transaction: {}", e))?;

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
            #[serde(rename = "efficiency")]
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

    create_result_string(&mut env, result)
}

// =============================================================================
// Transaction Broadcasting
// =============================================================================

/// Prepare a transaction broadcast (fragments it and returns fragments with packets)
/// Takes transaction bytes and returns fragments ready for BLE transmission
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_prepareBroadcast(
    mut env: JNIEnv,
    _class: JClass,
    _handle: jlong,
    transaction_bytes: JByteArray,
) -> jstring {
    let result = (|| -> Result<String, String> {
        tracing::info!("📡 FFI prepareBroadcast called");

        let tx_bytes: Vec<u8> = env
            .convert_byte_array(&transaction_bytes)
            .map_err(|e| format!("Failed to read transaction: {}", e))?;

        tracing::info!(
            "Preparing broadcast for {} byte transaction",
            tx_bytes.len()
        );

        // Fragment the transaction
        let fragments = crate::ble::fragment_transaction(&tx_bytes);
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
            let packet_bytes = broadcaster.prepare_fragment_packet(fragment)?;
            fragment_packets.push(FragmentPacket {
                transaction_id: hex::encode(fragment.transaction_id),
                fragment_index: fragment.fragment_index,
                total_fragments: fragment.total_fragments,
                packet_bytes: base64::encode(&packet_bytes),
            });
        }

        tracing::info!(
            "✅ Prepared {} fragment packets for broadcast",
            fragment_packets.len()
        );

        #[derive(serde::Serialize)]
        struct BroadcastPreparation {
            #[serde(rename = "transactionId")]
            transaction_id: String,
            #[serde(rename = "fragmentPackets")]
            fragment_packets: Vec<FragmentPacket>,
        }

        let preparation = BroadcastPreparation {
            transaction_id: hex::encode(transaction_id),
            fragment_packets,
        };

        let response: FfiResult<BroadcastPreparation> = FfiResult::success(preparation);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get mesh health snapshot
/// Returns current health metrics, peer status, and network topology
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getHealthSnapshot(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result = (|| -> Result<String, String> {
        tracing::info!("💚 FFI getHealthSnapshot called");

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        let snapshot = monitor.get_snapshot();

        tracing::info!(
            "✅ Health snapshot: {} peers, health score: {}",
            snapshot.metrics.total_peers,
            snapshot.metrics.health_score
        );

        #[derive(serde::Serialize)]
        struct HealthSnapshotResponse {
            #[serde(rename = "snapshot")]
            snapshot: crate::ble::HealthSnapshot,
        }

        let response: FfiResult<HealthSnapshotResponse> =
            FfiResult::success(HealthSnapshotResponse { snapshot });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
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
    let result = (|| -> Result<String, String> {
        tracing::info!("💓 FFI recordPeerHeartbeat called");

        let peer_id: String = env
            .get_string(&peer_id)
            .map_err(|e| format!("Failed to read peer_id: {}", e))?
            .into();

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        monitor.record_heartbeat(&peer_id);

        tracing::info!("✅ Recorded heartbeat for peer: {}", peer_id);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result = (|| -> Result<String, String> {
        tracing::info!("⏱️ FFI recordPeerLatency called");

        let peer_id: String = env
            .get_string(&peer_id)
            .map_err(|e| format!("Failed to read peer_id: {}", e))?
            .into();

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        monitor.record_latency(&peer_id, latency_ms as u32);

        tracing::info!("✅ Recorded {}ms latency for peer: {}", latency_ms, peer_id);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result = (|| -> Result<String, String> {
        tracing::info!("📶 FFI recordPeerRssi called");

        let peer_id: String = env
            .get_string(&peer_id)
            .map_err(|e| format!("Failed to read peer_id: {}", e))?
            .into();

        let transport = get_transport(handle)?;
        let monitor = transport.health_monitor();
        monitor.record_rssi(&peer_id, rssi as i8);

        tracing::info!("✅ Recorded {}dBm RSSI for peer: {}", rssi, peer_id);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Push a received transaction into the auto-submission queue
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_pushReceivedTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    transaction_bytes: JByteArray,
) -> jstring {
    let result: Result<String, String> = (|| {
        let tx_bytes: Vec<u8> = env
            .convert_byte_array(&transaction_bytes)
            .map_err(|e| format!("Failed to read transaction bytes: {}", e))?;

        let transport = get_transport(handle)?;
        log::info!("📥 pushReceivedTransaction handle={} bytes={}", handle, tx_bytes.len());

        let added = transport.push_received_transaction(tx_bytes);

        #[derive(serde::Serialize)]
        struct PushResponse {
            added: bool,
            queue_size: usize,
        }

        let queue_size = transport.received_queue_size();
        if added {
            log::info!("✅ pushReceivedTransaction accepted — queue_size={}", queue_size);
        } else {
            log::info!("⚠️  pushReceivedTransaction duplicate/full — queue_size={}", queue_size);
        }

        let response: FfiResult<PushResponse> =
            FfiResult::success(PushResponse { added, queue_size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get next received transaction for auto-submission
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_nextReceivedTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        log::debug!(
            "🔍 FFI nextReceivedTransaction called with handle: {}",
            handle
        );
        let transport = get_transport(handle)?;
        match transport.next_received_transaction() {
            Some((tx_id, tx_bytes, received_at)) => {
                log::debug!(
                    "✅ Popped transaction {} ({} bytes) from queue",
                    tx_id,
                    tx_bytes.len()
                );
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

                let response: FfiResult<ReceivedTransaction> =
                    FfiResult::success(ReceivedTransaction {
                        tx_id,
                        transaction_base64: BASE64.encode(&tx_bytes),
                        received_at,
                    });

                serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
            }
            None => {
                log::debug!("📭 No transaction in queue, returning None");
                let response: FfiResult<Option<String>> = FfiResult::success(None);
                let json_response = serde_json::to_string(&response)
                    .map_err(|e| format!("Serialization error: {}", e))?;
                log::debug!(
                    "📤 FFI nextReceivedTransaction returning None (JSON: {})",
                    json_response
                );
                Ok(json_response)
            }
        }
    })();

    create_result_string(&mut env, result)
}

/// Get count of transactions waiting for auto-submission
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getReceivedQueueSize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        log::debug!("🔍 FFI getReceivedQueueSize called with handle: {}", handle);
        let transport = get_transport(handle)?;
        log::debug!("✅ Got transport instance for handle {}", handle);

        let queue_size = transport.received_queue_size();
        #[derive(serde::Serialize)]
        struct QueueSizeResponse {
            #[serde(rename = "queueSize")]
            queue_size: usize,
        }

        let response: FfiResult<QueueSizeResponse> =
            FfiResult::success(QueueSizeResponse { queue_size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get fragment reassembly info for all incomplete transactions
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getFragmentReassemblyInfo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        log::debug!(
            "🔍 FFI getFragmentReassemblyInfo called with handle: {}",
            handle
        );
        let transport = get_transport(handle)?;
        log::debug!("✅ Got transport instance for handle {}", handle);

        let info_list = transport.get_fragment_reassembly_info();

        use crate::ffi::types::FragmentReassemblyInfoList;

        let response_data = FragmentReassemblyInfoList {
            transactions: info_list,
        };

        let response: FfiResult<FragmentReassemblyInfoList> = FfiResult::success(response_data);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Mark a transaction as successfully submitted
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_markTransactionSubmitted(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    transaction_bytes: JByteArray,
) -> jstring {
    let result: Result<String, String> = (|| {
        let tx_bytes: Vec<u8> = env
            .convert_byte_array(&transaction_bytes)
            .map_err(|e| format!("Failed to read transaction bytes: {}", e))?;

        let transport = get_transport(handle)?;
        // Log SHA-256 prefix for dedup tracing without logging the full tx
        let hash_prefix = {
            use sha2::{Digest, Sha256};
            let h = Sha256::digest(&tx_bytes);
            hex::encode(&h[..4])
        };
        log::info!("🔖 markTransactionSubmitted handle={} sha256_prefix={} bytes={}",
            handle, hash_prefix, tx_bytes.len());
        transport.mark_transaction_submitted(&tx_bytes);

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Clean up old submitted transaction hashes
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_cleanupOldSubmissions(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        transport.cleanup_old_submissions();

        #[derive(serde::Serialize)]
        struct SuccessResponse {
            success: bool,
        }

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get outbound queue size (non-destructive peek for debugging)
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getOutboundQueueSize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let queue_size = transport.outbound_queue_size();

        #[derive(serde::Serialize)]
        struct QueueSizeResponse {
            #[serde(rename = "queueSize")]
            queue_size: usize,
        }

        let response: FfiResult<QueueSizeResponse> =
            FfiResult::success(QueueSizeResponse { queue_size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get outbound queue debug info (non-destructive peek)
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_debugOutboundQueue(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
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
            total_fragments: usize,
            fragments: Vec<FragmentInfo>,
        }

        let fragments: Vec<FragmentInfo> = queue_info
            .iter()
            .map(|(idx, size)| FragmentInfo {
                index: *idx,
                size: *size,
            })
            .collect();

        let total_bytes: usize = fragments.iter().map(|f| f.size).sum();

        tracing::info!(
            "🔍 Queue debug: {} fragments, {} total bytes",
            fragments.len(),
            total_bytes
        );

        let response = QueueDebugResponse {
            total_fragments: fragments.len(),
            fragments,
        };

        let ffi_response: FfiResult<QueueDebugResponse> = FfiResult::success(response);
        serde_json::to_string(&ffi_response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        runtime::block_on(async {
            // Save queue manager queues (outbound, retry, confirmation)
            transport
                .sdk
                .queue_manager()
                .force_save()
                .await
                .map_err(|e| format!("Failed to save queues: {}", e))?;

            // Save received queue if storage directory is available
            if let Some(queue_storage_dir) = transport.get_queue_storage_dir() {
                if let Err(e) = transport.save_received_queue(&queue_storage_dir) {
                    log::warn!("⚠️ Failed to save received queue: {}", e);
                    // Don't fail the entire operation if received queue save fails
                }
            }

            Ok::<(), String>(())
        })?;

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Trigger auto-save if needed (debounced)
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_autoSaveQueues(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        runtime::block_on(async {
            // Auto-save queue manager queues (outbound, retry, confirmation)
            transport
                .sdk
                .queue_manager()
                .save_if_needed()
                .await
                .map_err(|e| format!("Failed to auto-save queues: {}", e))?;

            // Auto-save received queue if storage directory is available
            // Note: Received queue uses the same debouncing as queue manager
            if let Some(queue_storage_dir) = transport.get_queue_storage_dir() {
                if let Err(e) = transport.save_received_queue(&queue_storage_dir) {
                    log::warn!("⚠️ Failed to auto-save received queue: {}", e);
                    // Don't fail the entire operation if received queue save fails
                }
            }

            Ok::<(), String>(())
        })?;

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let request_str: String = env
            .get_string(&request_json)
            .map_err(|e| format!("Failed to get request string: {}", e))?
            .into();

        let request: PushOutboundRequest = serde_json::from_str(&request_str)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        log::info!("📤 pushOutboundTransaction handle={} tx_id={} fragments={} priority={:?}",
            handle, &request.tx_id[..8.min(request.tx_id.len())],
            request.fragments.len(), request.priority);

        // Convert FFI fragments to mesh fragments
        let fragments: Result<Vec<crate::ble::mesh::TransactionFragment>, String> = request
            .fragments
            .iter()
            .map(|f| {
                let tx_id = hex::decode(&f.transaction_id)
                    .map_err(|e| format!("Invalid transaction ID: {}", e))?;
                if tx_id.len() != 32 {
                    return Err("Transaction ID must be 32 bytes".to_string());
                }
                let mut tx_id_array = [0u8; 32];
                tx_id_array.copy_from_slice(&tx_id);

                let data = base64::decode(&f.data_base64)
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
        let tx_bytes = base64::decode(&request.tx_bytes)
            .map_err(|e| format!("Invalid transaction bytes: {}", e))?;

        // Convert priority
        let priority = match request.priority {
            PriorityFFI::High => crate::queue::Priority::High,
            PriorityFFI::Normal => crate::queue::Priority::Normal,
            PriorityFFI::Low => crate::queue::Priority::Low,
        };

        // Create outbound transaction
        let outbound_tx =
            crate::queue::OutboundTransaction::new(request.tx_id, tx_bytes, fragments, priority);

        // Push to queue
        runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue
                .push(outbound_tx)
                .map_err(|e| format!("Failed to push to queue: {}", e))?;
            Ok::<(), String>(())
        })?;

        log::info!("✅ pushOutboundTransaction enqueued");
        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Accept and queue a pre-signed transaction from external partners
/// Verifies the transaction, compresses it if needed, fragments it, and adds to queue
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_acceptAndQueueExternalTransaction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let request_str: String = env
            .get_string(&request_json)
            .map_err(|e| format!("Failed to get request string: {}", e))?
            .into();

        let request: AcceptExternalTransactionRequest = serde_json::from_str(&request_str)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        let tx_id = runtime::block_on(async {
            // First, verify and queue in priority queue (for tracking/management)
            transport
                .sdk
                .accept_and_queue_external_transaction(
                    &request.base64_signed_tx,
                    request.max_payload,
                )
                .await
        })
        .map_err(|e| format!("Failed to accept and queue external transaction: {}", e))?;

        // CRITICAL FIX: Also populate transport.outbound_queue so next_outbound() can read fragments
        // The transaction was already verified and fragmented by accept_and_queue_external_transaction
        // Now we need to get those fragments and add them to the fragment queue
        runtime::block_on(async {
            // Get mutable access to the queue to pop transactions
            let mut queue = transport.sdk.queue_manager().outbound.write().await;

            // Pop transactions until we find the one we just added
            let mut found_tx = None;
            let mut popped_txs = Vec::new();

            // Search through all priorities by popping
            while let Some(tx) = queue.pop() {
                if tx.tx_id == tx_id {
                    found_tx = Some(tx);
                    break;
                } else {
                    popped_txs.push(tx);
                }
            }

            // Put back all the transactions we popped (maintain original order)
            // Note: push() will add to the correct priority queue based on tx.priority
            for tx in popped_txs {
                // Re-add to queue (this will maintain priority)
                if let Err(e) = queue.push(tx) {
                    tracing::warn!("⚠️ Failed to re-queue transaction: {}", e);
                }
            }

            if let Some(tx) = found_tx {
                // Store fragment count before moving tx
                let fragment_count = tx.fragments.len();

                // Queue fragments directly using the public method
                transport.queue_fragments(&tx.fragments)
                    .map_err(|e| format!("Failed to queue fragments: {}", e))?;

                // Put the transaction back in the priority queue (for management/tracking)
                queue.push(tx).map_err(|e| format!("Failed to re-queue transaction: {}", e))?;

                tracing::info!("✅ External transaction {} fragments added to transport outbound queue ({} fragments)", tx_id, fragment_count);
            } else {
                tracing::warn!("⚠️ Could not find queued transaction {} to populate fragment queue", tx_id);
            }

            Ok::<(), String>(())
        }).map_err(|e| format!("Failed to populate fragment queue: {}", e))?;

        let response: FfiResult<String> = FfiResult::success(tx_id);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let tx_opt = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.pop()
        });

        if let Some(tx) = tx_opt {
            log::info!("📦 popOutboundTransaction → tx_id={} fragments={} priority={:?}",
                &tx.tx_id[..8.min(tx.tx_id.len())], tx.fragments.len(), tx.priority);
            let tx_ffi = OutboundTransactionFFI {
                tx_id: tx.tx_id,
                original_bytes: base64::encode(&tx.original_bytes),
                fragment_count: tx.fragments.len(),
                priority: match tx.priority {
                    crate::queue::Priority::High => PriorityFFI::High,
                    crate::queue::Priority::Normal => PriorityFFI::Normal,
                    crate::queue::Priority::Low => PriorityFFI::Low,
                },
                created_at: tx.created_at,
                retry_count: tx.retry_count,
            };

            let response: FfiResult<Option<OutboundTransactionFFI>> =
                FfiResult::success(Some(tx_ffi));
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        } else {
            log::debug!("📭 popOutboundTransaction — queue empty");
            let response: FfiResult<Option<OutboundTransactionFFI>> = FfiResult::success(None);
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        }
    })();

    create_result_string(&mut env, result)
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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let request_str: String = env
            .get_string(&request_json)
            .map_err(|e| format!("Failed to get request string: {}", e))?
            .into();

        let request: AddToRetryRequest = serde_json::from_str(&request_str)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        let tx_bytes = base64::decode(&request.tx_bytes)
            .map_err(|e| format!("Invalid transaction bytes: {}", e))?;

        log::info!("🔁 addToRetryQueue handle={} tx_id={} error={:?}",
            handle, &request.tx_id[..8.min(request.tx_id.len())], request.error);

        let retry_item = crate::queue::RetryItem::new(tx_bytes, request.tx_id, request.error);

        runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().retries.write().await;
            queue
                .push(retry_item)
                .map_err(|e| format!("Failed to push to retry queue: {}", e))?;
            Ok::<(), String>(())
        })?;

        log::info!("✅ addToRetryQueue enqueued");
        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let retry_opt = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().retries.write().await;
            queue.pop_ready()
        });

        if let Some(retry) = retry_opt {
            let retry_ffi = RetryItemFFI {
                tx_bytes: base64::encode(&retry.tx_bytes),
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

    create_result_string(&mut env, result)
}

/// Get retry queue size
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getRetryQueueSize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
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

        let response: FfiResult<QueueSizeResponse> =
            FfiResult::success(QueueSizeResponse { queue_size: size });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Cleanup expired confirmations and retry items
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_cleanupExpired(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
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
            confirmations_cleaned: usize,
            retries_cleaned: usize,
        }

        let response: FfiResult<CleanupExpiredResponse> =
            FfiResult::success(CleanupExpiredResponse {
                confirmations_cleaned,
                retries_cleaned,
            });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Confirm that all fragments for `tx_id` were delivered to the current peer.
/// Decrements the transaction's relevance counter by 1. Evicts the transaction and
/// returns { removed: true } when relevance hits 0 (fan-out exhausted).
/// Returns { removed: false } when the transaction is retained for future peers.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_confirmDelivered(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id_j: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let tx_id: String = env.get_string(&tx_id_j).map_err(|e| e.to_string())?.into();

        let removed = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.confirm_delivered(&tx_id)
        });

        #[derive(serde::Serialize)]
        struct ConfirmDeliveredResponse { removed: bool }
        let response: FfiResult<ConfirmDeliveredResponse> =
            FfiResult::success(ConfirmDeliveredResponse { removed });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Peek at the highest-relevance transaction in the outbound queue and load its
/// fragments into the transport's BLE frame buffer so the sending loop can deliver
/// them to the current peer. Returns the tx_id, current relevance, and fragment count,
/// or null data if the queue is empty.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_loadForSending(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        // Peek under a read lock — clone the data we need so we don't hold the lock
        // while calling queue_fragments (which takes an unrelated mutex).
        let tx_info = runtime::block_on(async {
            let queue = transport.sdk.queue_manager().outbound.read().await;
            queue.peek_highest_relevance().map(|tx| {
                (tx.tx_id.clone(), tx.fragments.clone(), tx.relevance)
            })
        });

        #[derive(serde::Serialize)]
        struct LoadResponse { tx_id: String, relevance: u8, fragment_count: usize }

        if let Some((tx_id, fragments, relevance)) = tx_info {
            transport.queue_fragments(&fragments)
                .map_err(|e| format!("Failed to load fragments into transport: {}", e))?;

            let response: FfiResult<Option<LoadResponse>> =
                FfiResult::success(Some(LoadResponse {
                    tx_id,
                    relevance,
                    fragment_count: fragments.len(),
                }));
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        } else {
            let response: FfiResult<Option<LoadResponse>> = FfiResult::success(None);
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        }
    })();
    create_result_string(&mut env, result)
}

/// Purge outbound transactions older than max_age_secs from all priority queues.
/// Call this at connection-start so stale relayed data is not forwarded.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_purgeStaleOutbound(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    max_age_secs: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let max_age = max_age_secs.max(0) as u64;

        let removed = runtime::block_on(async {
            let mut outbound = transport.sdk.queue_manager().outbound.write().await;
            outbound.cleanup_stale(max_age)
        });

        #[derive(serde::Serialize)]
        struct PurgeResponse { removed: usize }
        let response: FfiResult<PurgeResponse> = FfiResult::success(PurgeResponse { removed });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        // Parse request JSON from Kotlin
        let request_str: String = env
            .get_string(&request_json)
            .map_err(|e| format!("Failed to read request: {}", e))?
            .into();

        let request: QueueConfirmationRequest = serde_json::from_str(&request_str)
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
            let tx_id_bytes =
                hex::decode(&request.tx_id).map_err(|e| format!("Invalid txId hex: {}", e))?;
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

        let response: FfiResult<crate::ffi::types::SuccessResponse> =
            FfiResult::success(crate::ffi::types::SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let confirmation = runtime::block_on(async {
            let mut conf_queue = transport.sdk.queue_manager().confirmations.write().await;
            conf_queue.pop()
        });

        if let Some(conf) = confirmation {
            // Convert Rust Confirmation to FFI format
            let tx_id_hex = hex::encode(conf.original_tx_id);
            let status_ffi = match &conf.status {
                crate::queue::confirmation::ConfirmationStatus::Success { signature } => {
                    crate::ffi::types::ConfirmationStatusFFI::Success {
                        signature: signature.clone(),
                    }
                }
                crate::queue::confirmation::ConfirmationStatus::Failed { error } => {
                    crate::ffi::types::ConfirmationStatusFFI::Failed {
                        error: error.clone(),
                    }
                }
            };

            let conf_ffi = crate::ffi::types::ConfirmationFFI {
                tx_id: tx_id_hex,
                status: status_ffi,
                timestamp: conf.timestamp,
                relay_count: conf.relay_count,
            };

            let response: FfiResult<Option<crate::ffi::types::ConfirmationFFI>> =
                FfiResult::success(Some(conf_ffi));
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        } else {
            let response: FfiResult<Option<crate::ffi::types::ConfirmationFFI>> =
                FfiResult::success(None);
            serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
        }
    })();

    create_result_string(&mut env, result)
}

/// Cleanup stale fragments from the transaction cache
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_cleanupStaleFragments(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
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
            fragments_cleaned: usize,
        }

        let response: FfiResult<CleanupResponse> = FfiResult::success(CleanupResponse {
            fragments_cleaned: cleaned,
        });

        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Relay a received confirmation (increment hop count and re-queue for relay)
/// This is called when a confirmation is received that needs to be relayed further
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_relayConfirmation(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    confirmation_json: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        // Parse confirmation JSON from Kotlin
        let conf_str: String = env
            .get_string(&confirmation_json)
            .map_err(|e| format!("Failed to read confirmation JSON: {}", e))?
            .into();

        let conf_ffi: ConfirmationFFI = serde_json::from_str(&conf_str)
            .map_err(|e| format!("Failed to parse confirmation: {}", e))?;

        tracing::info!(
            "🔄 Relaying confirmation for tx {} (current hops: {})",
            &conf_ffi.tx_id[..std::cmp::min(16, conf_ffi.tx_id.len())],
            conf_ffi.relay_count
        );

        // Convert FFI confirmation to Rust confirmation
        let tx_id_bytes =
            hex::decode(&conf_ffi.tx_id).map_err(|e| format!("Invalid txId hex: {}", e))?;
        if tx_id_bytes.len() != 32 {
            return Err(format!(
                "Invalid txId length: expected 32 bytes, got {}",
                tx_id_bytes.len()
            ));
        }
        let mut tx_id_array = [0u8; 32];
        tx_id_array.copy_from_slice(&tx_id_bytes);

        let status = match &conf_ffi.status {
            ConfirmationStatusFFI::Success { signature } => {
                crate::queue::confirmation::ConfirmationStatus::Success {
                    signature: signature.clone(),
                }
            }
            ConfirmationStatusFFI::Failed { error } => {
                crate::queue::confirmation::ConfirmationStatus::Failed {
                    error: error.clone(),
                }
            }
        };

        // Create confirmation with incremented relay count
        let mut confirmation = crate::queue::confirmation::Confirmation {
            original_tx_id: tx_id_array,
            status,
            timestamp: conf_ffi.timestamp,
            relay_count: conf_ffi.relay_count,
            max_hops: 5, // Default max hops
        };

        // Increment relay count
        let relay_count_before = confirmation.relay_count;
        let max_hops = confirmation.max_hops;
        if !confirmation.increment_relay() {
            tracing::warn!(
                "⚠️ Confirmation for tx {} exceeded max hops ({}/{}) - dropping",
                &conf_ffi.tx_id[..std::cmp::min(16, conf_ffi.tx_id.len())],
                relay_count_before,
                max_hops
            );
            // Return success but don't queue (TTL exceeded)
            let response: FfiResult<SuccessResponse> =
                FfiResult::success(SuccessResponse { success: true });
            return serde_json::to_string(&response)
                .map_err(|e| format!("Serialization error: {}", e));
        }

        // Store relay count after increment for logging
        let relay_count_after = confirmation.relay_count;

        // Re-queue for relay
        runtime::block_on(async {
            let mut conf_queue = transport.sdk.queue_manager().confirmations.write().await;
            conf_queue
                .push(confirmation)
                .map_err(|e| format!("Failed to re-queue confirmation: {:?}", e))?;

            tracing::info!(
                "✅ Re-queued confirmation for tx {} (hops: {}/{})",
                &conf_ffi.tx_id[..std::cmp::min(16, conf_ffi.tx_id.len())],
                relay_count_after,
                max_hops
            );

            Ok::<(), String>(())
        })?;

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
/// Note: This does NOT clear nonce data
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_clearAllQueues(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        runtime::block_on(async {
            // Clear queue manager queues (outbound, retry, confirmation)
            transport
                .sdk
                .clear_all_queues()
                .await
                .map_err(|e| format!("Failed to clear queues: {}", e))?;

            // Clear reassembly buffers and completed transactions in transport
            transport.clear_all_reassembly_buffers();

            // Clear received queue
            transport.clear_received_queue();

            tracing::info!("✅ Cleared all queues (outbound, retry, confirmation, received) and reassembly buffers");

            Ok::<(), String>(())
        })?;

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });

        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let addr: String = env
            .get_string(&address)
            .map_err(|e| format!("Failed to read address string: {}", e))?
            .into();

        let addr_opt = if addr.is_empty() { None } else { Some(addr.clone()) };
        transport.set_wallet_address(addr_opt);

        info!("✅ Wallet address updated: {}", if addr.is_empty() { "<cleared>" } else { &addr });

        let response: FfiResult<SuccessResponse> =
            FfiResult::success(SuccessResponse { success: true });

        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
}

/// Get the wallet address currently set for this node session.
/// Returns an empty address field if none has been set.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getWalletAddress(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;

        let addr = transport.get_wallet_address().unwrap_or_default();

        #[derive(serde::Serialize)]
        struct WalletAddressResponse {
            address: String,
        }

        let response: FfiResult<WalletAddressResponse> =
            FfiResult::success(WalletAddressResponse { address: addr });

        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();

    create_result_string(&mut env, result)
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
    let result: Result<String, String> = (|| {
        let (pda, bump) = crate::intent::executor_pda();
        log::info!("🏦 getExecutorPda → pda={} bump={}", pda, bump);
        let response: FfiResult<ExecutorPdaResponse> = FfiResult::success(ExecutorPdaResponse {
            pda: pda.to_string(),
            bump,
        });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Builds a single unsigned transaction containing one `approve_checked` instruction
/// per entry in the request. The `owner_wallet` must sign the returned transaction
/// before it can be submitted to Solana.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createApproveTransaction(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JByteArray,
) -> jstring {
    let result: Result<String, String> = (|| {
        let bytes: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request bytes: {}", e))?;

        let req: CreateApproveTransactionRequest = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        log::info!("🔐 createApproveTransaction owner={} fee_payer={} blockhash={} tokens={}",
            req.owner_wallet, req.fee_payer, &req.recent_blockhash[..8], req.tokens.len());
        for t in &req.tokens {
            log::info!("   token: mint={} account={} amount={} decimals={}",
                t.mint_address, t.token_account, t.amount, t.decimals);
        }

        let owner: solana_sdk::pubkey::Pubkey = std::str::FromStr::from_str(&req.owner_wallet)
            .map_err(|e| format!("Invalid owner_wallet: {}", e))?;
        let fee_payer: solana_sdk::pubkey::Pubkey = std::str::FromStr::from_str(&req.fee_payer)
            .map_err(|e| format!("Invalid fee_payer: {}", e))?;

        let blockhash_bytes = bs58::decode(&req.recent_blockhash)
            .into_vec()
            .map_err(|e| format!("Invalid recent_blockhash: {}", e))?;
        let blockhash_arr: [u8; 32] = blockhash_bytes
            .try_into()
            .map_err(|_| "recent_blockhash must decode to 32 bytes".to_string())?;
        let recent_blockhash = solana_sdk::hash::Hash::new_from_array(blockhash_arr);

        let approvals: Vec<crate::intent::TokenApprovalInput> = req
            .tokens
            .into_iter()
            .map(|t| crate::intent::TokenApprovalInput {
                mint_address: t.mint_address,
                amount: t.amount,
                decimals: t.decimals,
                token_account: t.token_account,
                token_program: t.token_program,
            })
            .collect();

        let (executor_pda_key, _) = crate::intent::executor_pda();

        let tx_base64 = crate::intent::build_approve_transaction(
            &owner,
            &fee_payer,
            recent_blockhash,
            &approvals,
        )?;

        log::info!("✅ createApproveTransaction → executor_pda={} tx_base64_len={}",
            executor_pda_key, tx_base64.len());
        let response: FfiResult<ApproveTransactionResponse> =
            FfiResult::success(ApproveTransactionResponse {
                transaction: tx_base64,
                executor_pda: executor_pda_key.to_string(),
            });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Builds a single unsigned transaction with one `revoke` instruction per token account,
/// clearing the executor PDA's delegate authority.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createRevokeTransaction(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JByteArray,
) -> jstring {
    let result: Result<String, String> = (|| {
        let bytes: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request bytes: {}", e))?;

        let req: CreateRevokeTransactionRequest = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        log::info!("🔓 createRevokeTransaction owner={} fee_payer={} accounts={} program={}",
            req.owner_wallet, req.fee_payer, req.token_accounts.len(), req.token_program);

        let owner: solana_sdk::pubkey::Pubkey = std::str::FromStr::from_str(&req.owner_wallet)
            .map_err(|e| format!("Invalid owner_wallet: {}", e))?;
        let fee_payer: solana_sdk::pubkey::Pubkey = std::str::FromStr::from_str(&req.fee_payer)
            .map_err(|e| format!("Invalid fee_payer: {}", e))?;

        let blockhash_bytes = bs58::decode(&req.recent_blockhash)
            .into_vec()
            .map_err(|e| format!("Invalid recent_blockhash: {}", e))?;
        let blockhash_arr: [u8; 32] = blockhash_bytes
            .try_into()
            .map_err(|_| "recent_blockhash must decode to 32 bytes".to_string())?;
        let recent_blockhash = solana_sdk::hash::Hash::new_from_array(blockhash_arr);

        let tx_base64 = crate::intent::build_revoke_transaction(
            &owner,
            &fee_payer,
            recent_blockhash,
            &req.token_accounts,
            &req.token_program,
        )?;

        log::info!("✅ createRevokeTransaction → tx_base64_len={}", tx_base64.len());
        let response: FfiResult<RevokeTransactionResponse> =
            FfiResult::success(RevokeTransactionResponse { transaction: tx_base64 });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Serializes an Intent into the canonical 169-byte borsh layout and returns it as
/// base64. Generates a random 16-byte nonce unless `nonce_hex` is supplied.
/// Sign the returned `intent_bytes` with Ed25519 before submitting via pollicore.
#[no_mangle]
#[cfg(feature = "android")]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createIntentBytes(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JByteArray,
) -> jstring {
    let result: Result<String, String> = (|| {
        let bytes: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request bytes: {}", e))?;

        let req: CreateIntentBytesRequest = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse request: {}", e))?;

        log::info!("🎯 createIntentBytes");
        log::info!("   from={}", req.from);
        log::info!("   to={}", req.to);
        log::info!("   token_mint={}", req.token_mint);
        log::info!("   amount={}", req.amount);
        log::info!("   expires_at={}", req.expires_at);
        log::info!("   gas_fee_amount={}", req.gas_fee_amount);
        log::info!("   gas_fee_payee={}", req.gas_fee_payee);

        let pubkey_bytes = |s: &str, field: &str| -> Result<[u8; 32], String> {
            let pk: solana_sdk::pubkey::Pubkey = std::str::FromStr::from_str(s)
                .map_err(|e| format!("Invalid {}: {}", field, e))?;
            Ok(pk.to_bytes())
        };

        let from          = pubkey_bytes(&req.from, "from")?;
        let to            = pubkey_bytes(&req.to, "to")?;
        let token_mint    = pubkey_bytes(&req.token_mint, "token_mint")?;
        let gas_fee_payee = pubkey_bytes(&req.gas_fee_payee, "gas_fee_payee")?;

        let nonce: [u8; 16] = if let Some(hex_str) = &req.nonce_hex {
            let decoded = hex::decode(hex_str)
                .map_err(|e| format!("Invalid nonce_hex: {}", e))?;
            decoded
                .try_into()
                .map_err(|_| "nonce_hex must decode to exactly 16 bytes (32 hex chars)".to_string())?
        } else {
            crate::intent::random_nonce()
        };

        let intent_bytes = crate::intent::serialize_intent(
            1,
            &from,
            &to,
            &token_mint,
            req.amount,
            &nonce,
            req.expires_at,
            req.gas_fee_amount,
            &gas_fee_payee,
        );

        use base64::{engine::general_purpose::STANDARD, Engine};
        let encoded = STANDARD.encode(intent_bytes);
        log::info!("✅ createIntentBytes → {} bytes (base64_len={}) nonce={}",
            169, encoded.len(), hex::encode(nonce));
        let response: FfiResult<IntentBytesResponse> = FfiResult::success(IntentBytesResponse {
            intent_bytes: encoded,
            nonce_hex: hex::encode(nonce),
        });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

// =============================================================================
// Intent submission — delegates to crate::submission
// =============================================================================

/// Submit a signed intent to pollicore.
///
/// The pollicore URL is baked in at compile time from `POLLICORE_URL` in `.env`.
/// All submission logic (HTTP client, logging, error handling) lives in
/// `crate::submission` so that censorship-hardening transports can be added there
/// without touching the FFI glue.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_submitIntent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    request_json: JByteArray,
) -> jstring {
    use crate::submission::{SubmitIntentRequest, SubmitIntentResponse};

    let result: Result<String, String> = (|| {
        let transports = TRANSPORTS.lock();
        let transport = transports
            .get(handle as usize)
            .and_then(|t| t.as_ref())
            .ok_or_else(|| format!("Invalid handle: {}", handle))?;

        let pollicore_url = transport
            .get_pollicore_url()
            .ok_or_else(|| "POLLICORE_URL not configured — set it in .env before building".to_string())?;

        let req_bytes: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request bytes: {}", e))?;
        let req: SubmitIntentRequest = serde_json::from_slice(&req_bytes)
            .map_err(|e| format!("Failed to parse SubmitIntentRequest: {}", e))?;

        let resp: SubmitIntentResponse = crate::submission::submit_intent(&pollicore_url, &req)
            .map_err(|e| e.to_string())?;

        let response: FfiResult<SubmitIntentResponse> = FfiResult::success(resp);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

// =============================================================================
// Subsystem 1 — Density-adaptive rotation
// =============================================================================

/// Record a scan observation for density estimation.
/// Call on every `onScanResult` with the remote device address.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_recordScanResult(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let peer_str: String = env
            .get_string(&peer_id)
            .map_err(|e| format!("peer_id: {}", e))?
            .into();
        transport.density_estimator.lock().record(&peer_str);
        let response: FfiResult<bool> = FfiResult::success(true);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Recompute and return adaptive BLE session/cooldown parameters.
/// Call every 10 seconds from Kotlin.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getAdaptiveParams(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let params = transport.density_estimator.lock().compute_params();
        let response: FfiResult<crate::ble::AdaptiveParams> = FfiResult::success(params);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Add `peer_id` to the cooldown list for `cooldown_ms` milliseconds.
/// Call after every session ends (both mutual-drain and force-close paths).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_addPeerToCooldown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
    cooldown_ms: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let peer_str: String = env
            .get_string(&peer_id)
            .map_err(|e| format!("peer_id: {}", e))?
            .into();
        transport
            .cooldown_list
            .lock()
            .add(&peer_str, cooldown_ms as u64);
        let response: FfiResult<bool> = FfiResult::success(true);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Returns true if `peer_id` is currently in cooldown (should not be connected to).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_isPeerInCooldown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let peer_str: String = env
            .get_string(&peer_id)
            .map_err(|e| format!("peer_id: {}", e))?
            .into();
        let cooling = transport.cooldown_list.lock().is_cooling(&peer_str);
        let response: FfiResult<bool> = FfiResult::success(cooling);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Sparse-network safety net: expire the oldest cooldown entry early.
/// Call when idle_ms > 2 * session_target_ms AND all eligible peers are in cooldown.
/// Returns the peer_id that was released, or null if the cooldown list was empty.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_expireOldestCooldown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let expired = transport.cooldown_list.lock().expire_oldest();
        let response: FfiResult<Option<String>> = FfiResult::success(expired);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Log a session telemetry record. Stored locally; future versions will relay to Pollicore.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_logSessionTelemetry(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    telemetry_json: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let _transport = get_transport(handle)?;
        let json_str: String = env
            .get_string(&telemetry_json)
            .map_err(|e| format!("telemetry_json: {}", e))?
            .into();
        // Parse to validate the structure before logging.
        let _record: crate::ble::SessionTelemetry = serde_json::from_str(&json_str)
            .map_err(|e| format!("Invalid telemetry JSON: {}", e))?;
        log::info!("[SESSION_TELEMETRY] {}", json_str);
        let response: FfiResult<bool> = FfiResult::success(true);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

// =============================================================================
// Subsystem 2 — Per-peer materialized queue
// =============================================================================

/// Returns the list of tx_ids that should be sent to `peer_id` (4-byte hex compact ID).
/// Filters by deliveredTo exclusion, TTL, and relevance > 0.
/// Sorted by: is_confirmation desc, priority desc, relevance desc, age asc.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_outboundForPeer(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    peer_id_hex: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let peer_hex: String = env
            .get_string(&peer_id_hex)
            .map_err(|e| format!("peer_id_hex: {}", e))?
            .into();
        let peer_bytes = hex::decode(&peer_hex)
            .map_err(|e| format!("Invalid peer_id_hex: {}", e))?;
        let peer_id: [u8; 4] = peer_bytes
            .try_into()
            .map_err(|_| "peer_id must be 4 bytes (8 hex chars)".to_string())?;

        let tx_ids = runtime::block_on(async {
            let queue = transport.sdk.queue_manager().outbound.read().await;
            queue.outbound_for_peer(&peer_id)
                .iter()
                .map(|tx| tx.tx_id.clone())
                .collect::<Vec<_>>()
        });

        let response: FfiResult<Vec<String>> = FfiResult::success(tx_ids);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Drain-conditional delivery confirmation (Subsystem 2).
/// Call ONLY on mutual drain. Adds `peer_id` to deliveredTo, decrements relevance.
/// Returns `{ removed: bool }` — true if the entry was evicted (relevance reached 0).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_confirmDeliveredByPeer(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id: JString,
    peer_id_hex: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let tx_id_str: String = env
            .get_string(&tx_id)
            .map_err(|e| format!("tx_id: {}", e))?
            .into();
        let peer_hex: String = env
            .get_string(&peer_id_hex)
            .map_err(|e| format!("peer_id_hex: {}", e))?
            .into();
        let peer_bytes = hex::decode(&peer_hex)
            .map_err(|e| format!("Invalid peer_id_hex: {}", e))?;
        let peer_id: [u8; 4] = peer_bytes
            .try_into()
            .map_err(|_| "peer_id must be 4 bytes".to_string())?;

        let removed = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.confirm_delivered_by_peer(&tx_id_str, &peer_id)
        });

        #[derive(serde::Serialize)]
        struct RemovedResponse { removed: bool }
        let response: FfiResult<RemovedResponse> = FfiResult::success(RemovedResponse { removed });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

// =============================================================================
// Subsystem 3 — Confirmation-driven purge
// =============================================================================

/// Ingest a received (or locally generated) confirmation.
///
/// Verifies the Ed25519 signature against the bundled Pollicore public key.
/// On success:
///   - purges any matching entry from the outbound carrier set
///   - discards the inbound reassembly buffer for this txId (if present)
///   - creates a tombstone
///   - queues the confirmation for re-propagation at HIGH priority
///
/// Returns `{ purged: bool, added_to_carrier: bool }`.
/// Silently drops tampered/unverifiable confirmations (returns success with both false).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_ingestConfirmation(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    confirmation_bytes: JByteArray,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let raw: Vec<u8> = env
            .convert_byte_array(&confirmation_bytes)
            .map_err(|e| format!("confirmation_bytes: {}", e))?;

        let conf = crate::ble::MeshConfirmation::from_frame_bytes(&raw)
            .map_err(|e| format!("Deserialize confirmation: {}", e))?;

        // Verify signature — POLLICORE_PUBKEY_BYTES is the 32-byte Ed25519 verifying key
        // bundled at compile time. If not set, skip verification (dev mode only).
        let valid = if let Some(pk) = get_pollicore_pubkey() {
            conf.verify(&pk)
        } else {
            log::warn!("POLLICORE_PUBKEY not configured — skipping signature verification (dev mode)");
            true
        };

        #[derive(serde::Serialize)]
        struct IngestResult { purged: bool, added_to_carrier: bool }

        if !valid {
            log::warn!("Dropped tampered confirmation for tx_id_hash={}", hex::encode(conf.tx_id_hash));
            let response: FfiResult<IngestResult> = FfiResult::success(IngestResult { purged: false, added_to_carrier: false });
            return serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e));
        }

        let tx_id_hash_hex = hex::encode(conf.tx_id_hash);

        // Purge matching entry from outbound carrier set
        let purged = runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.purge_by_tx_id(&tx_id_hash_hex)
        });

        // Discard inbound reassembly buffer for this txId
        {
            let mut bufs = transport.inbound_buffers.lock();
            bufs.remove(&tx_id_hash_hex);
        }

        // Create tombstone (valid for 2 × confirmation TTL)
        {
            let tomb = crate::ble::Tombstone::new(conf.tx_id_hash, crate::ble::CONFIRMATION_TTL_SECS / 2);
            transport.tombstones.lock().insert(tx_id_hash_hex.clone(), tomb);
        }

        // Expire cooldown overrides so peers learn about this purge quickly
        {
            transport.cooldown_list.lock().expire_not_delivered(&conf.delivered_to);
        }

        // Wrap confirmation as an outbound entry and push to HIGH priority
        let added_to_carrier = if conf.is_alive() {
            let conf_bytes = conf.to_frame_bytes()?;
            let fragments = crate::ble::fragment_transaction(&conf_bytes);
            let tx = crate::queue::OutboundTransaction {
                tx_id: tx_id_hash_hex.clone(),
                original_bytes: conf_bytes,
                fragments,
                priority: crate::queue::Priority::High,
                created_at: conf.added_at,
                retry_count: 0,
                max_retries: 3,
                relevance: conf.relevance,
                delivered_to: conf.delivered_to,
                ttl_secs: crate::ble::CONFIRMATION_TTL_SECS,
                hop_count: conf.hop_count,
                is_confirmation: true,
            };
            let pushed = runtime::block_on(async {
                let mut queue = transport.sdk.queue_manager().outbound.write().await;
                queue.push(tx).is_ok()
            });
            pushed
        } else {
            false
        };

        log::info!("ingestConfirmation txId={} purged={} added_to_carrier={}", tx_id_hash_hex, purged, added_to_carrier);

        let response: FfiResult<IngestResult> = FfiResult::success(IngestResult { purged, added_to_carrier });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Check if a tx_id_hash (hex) has an active tombstone.
/// Returns `{ tombstoned: bool }`. Call before buffering inbound fragments.
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_isTombstoned(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tx_id_hash_hex: JString,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let hash_hex: String = env
            .get_string(&tx_id_hash_hex)
            .map_err(|e| format!("tx_id_hash_hex: {}", e))?
            .into();
        let tombstoned = transport
            .tombstones
            .lock()
            .get(&hash_hex)
            .map(|t| t.is_valid())
            .unwrap_or(false);
        #[derive(serde::Serialize)]
        struct TombResponse { tombstoned: bool }
        let response: FfiResult<TombResponse> = FfiResult::success(TombResponse { tombstoned });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
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
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        // Evict expired tombstones
        transport.tombstones.lock().retain(|_, t| t.is_valid());
        // Evict expired cooldowns
        transport.cooldown_list.lock().evict_expired();
        let response: FfiResult<bool> = FfiResult::success(true);
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

/// Get the number of active tombstones (diagnostic only).
#[cfg(feature = "android")]
#[no_mangle]
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_getTombstoneCount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result: Result<String, String> = (|| {
        let transport = get_transport(handle)?;
        let count = transport.tombstones.lock().len();
        #[derive(serde::Serialize)]
        struct CountResponse { count: usize }
        let response: FfiResult<CountResponse> = FfiResult::success(CountResponse { count });
        serde_json::to_string(&response).map_err(|e| format!("Serialization error: {}", e))
    })();
    create_result_string(&mut env, result)
}

// =============================================================================
// Pollicore public key resolution
// =============================================================================

/// Returns the bundled Pollicore Ed25519 public key (32 bytes), or None in dev mode.
/// The key is embedded at compile time via the POLLICORE_PUBKEY env var (64-char hex).
fn get_pollicore_pubkey() -> Option<[u8; 32]> {
    let hex_str = option_env!("POLLICORE_PUBKEY")?;
    if hex_str.is_empty() {
        return None;
    }
    let bytes = hex::decode(hex_str).ok()?;
    bytes.try_into().ok()
}
