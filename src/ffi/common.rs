//! Platform-neutral FFI core shared by the Android (JNI) and iOS (C ABI) bindings.
//!
//! Every FFI operation lives here as a plain-Rust `op_*` function taking native
//! types (`i64` handle, `&str`/`&[u8]` arguments) and returning the JSON
//! `FfiResult` envelope as `Result<String, String>` (Ok = serialized success
//! envelope, Err = error message the binding wraps into an error envelope).
//! `android.rs` and `ios.rs` are thin marshalling shims over this module, so the
//! two platforms cannot drift apart in behavior or wire format.

#![allow(deprecated)]

use parking_lot::Mutex;
use std::str::FromStr;
use std::sync::Arc;

use super::host_transport::HostTransport;
use super::runtime;
use super::transport::HostBleTransport;
use super::types::*;
use super::wifi_direct_transport::HostWifiDirectTransport;

use log::{error, info};
use solana_sdk::pubkey::Pubkey;

/// One registered transport instance, tagged by which radio it drives.
///
/// `core` is the radio-agnostic [`HostTransport`] used by the byte-level FFI contract
/// (pushInbound/nextOutbound/metrics/…) so a single set of FFI functions serves BLE and
/// Wi-Fi Direct alike — no `if wifi { .. }` scattered through the core. `ble` is the
/// concrete BLE engine, present only for BLE handles, so the rich BLE-specific FFI
/// surface (queue manager, health, intent building) keeps working unchanged.
pub(crate) struct TransportEntry {
    pub(crate) kind: TransportKind,
    pub(crate) core: Arc<dyn HostTransport>,
    pub(crate) ble: Option<Arc<HostBleTransport>>,
}

// Global state for transport instances (single tagged registry; handle == index).
lazy_static::lazy_static! {
    pub(crate) static ref TRANSPORTS: Arc<Mutex<Vec<Option<TransportEntry>>>> =
        Arc::new(Mutex::new(Vec::new()));
}

/// Resolve a handle to the concrete BLE engine. Used by BLE-specific FFI functions
/// (queue manager, health, intent building). Returns an error for non-BLE handles.
pub(crate) fn get_transport(handle: i64) -> Result<Arc<HostBleTransport>, String> {
    let transports = TRANSPORTS.lock();
    if handle < 0 || handle as usize >= transports.len() {
        return Err(format!("Invalid handle: {}", handle));
    }
    let entry = transports[handle as usize]
        .as_ref()
        .ok_or_else(|| format!("Handle {} has been shut down", handle))?;
    entry.ble.clone().ok_or_else(|| {
        format!(
            "Handle {} is a {} transport (no BLE-specific surface)",
            handle,
            entry.kind.as_str()
        )
    })
}

/// Resolve a handle to the radio-agnostic transport contract. Works for BLE and Wi-Fi
/// Direct alike — used by the byte-level FFI functions (pushInbound/nextOutbound/…).
pub(crate) fn get_core(handle: i64) -> Result<Arc<dyn HostTransport>, String> {
    let transports = TRANSPORTS.lock();
    if handle < 0 || handle as usize >= transports.len() {
        return Err(format!("Invalid handle: {}", handle));
    }
    transports[handle as usize]
        .as_ref()
        .map(|e| e.core.clone())
        .ok_or_else(|| format!("Handle {} has been shut down", handle))
}

pub(crate) fn parse_log_level(level: Option<&str>) -> tracing::Level {
    match level {
        Some("trace") => tracing::Level::TRACE,
        Some("debug") => tracing::Level::DEBUG,
        Some("info") => tracing::Level::INFO,
        Some("warn") => tracing::Level::WARN,
        Some("error") => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

/// Serialize a success envelope. Every op funnels through this so the wire
/// format ({"ok":true,"data":…}) is identical on all platforms.
fn ok_json<T: serde::Serialize>(data: T) -> Result<String, String> {
    serde_json::to_string(&FfiResult::success(data))
        .map_err(|e| format!("Serialization error: {}", e))
}

/// Returns the bundled Pollicore Ed25519 public key (32 bytes), or None in dev mode.
/// The key is embedded at compile time via the POLLICORE_PUBKEY env var (64-char hex).
pub(crate) fn get_pollicore_pubkey() -> Option<[u8; 32]> {
    let hex_str = option_env!("POLLICORE_PUBKEY")?;
    if hex_str.is_empty() {
        return None;
    }
    let bytes = hex::decode(hex_str).ok()?;
    bytes.try_into().ok()
}

// =============================================================================
// Initialization and lifecycle
// =============================================================================

/// Apply the log level requested in the config — Off when enableLogging is false,
/// the desired level otherwise. `log::set_max_level` is the global filter gate;
/// setting it to Off prevents all log!/tracing! calls from reaching the platform
/// sink even if a subscriber is registered.
fn apply_logging(config: &SdkConfig) {
    if config.enable_logging {
        let tracing_level = parse_log_level(config.log_level.as_deref());
        let log_level = match tracing_level {
            tracing::Level::ERROR => log::LevelFilter::Error,
            tracing::Level::WARN => log::LevelFilter::Warn,
            tracing::Level::INFO => log::LevelFilter::Info,
            tracing::Level::DEBUG => log::LevelFilter::Debug,
            tracing::Level::TRACE => log::LevelFilter::Trace,
        };
        log::set_max_level(log_level);
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing_level)
            .try_init();
        info!(
            "🔧 PolliNet-Rust logging enabled (level: {:?})",
            tracing_level
        );
    } else {
        log::set_max_level(log::LevelFilter::Off);
    }
}

/// Create and configure the engine from an [`SdkConfig`] — shared by the BLE and
/// standalone Wi-Fi Direct init paths so both configure identically.
fn build_engine(config: &SdkConfig) -> Result<HostBleTransport, String> {
    match runtime::init_runtime() {
        Ok(_) => info!("✅ Runtime initialized"),
        Err(e) if e.contains("already initialized") => {}
        Err(e) => return Err(format!("Failed to initialize runtime: {}", e)),
    }

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

    if let Some(storage_dir) = &config.storage_directory {
        info!("Setting up secure storage at: {}", storage_dir);
        transport
            .set_secure_storage(storage_dir, config.encryption_key.clone())
            .map_err(|e| {
                error!("❌ Failed to set secure storage: {}", e);
                e
            })?;
        // Phase 5: Set queue storage directory (stored on transport, no env var mutation)
        let queue_storage_dir = format!("{}/queues", storage_dir);
        transport.set_queue_storage_dir(queue_storage_dir.clone());
        info!("✅ Queue persistence enabled at: {}", queue_storage_dir);
    } else {
        info!("ℹ️  No storage directory provided - bundle persistence disabled");
    }

    // Resolve pollicore URL: baked-in at compile time from .env / POLLICORE_URL env var
    if let Some(url) = option_env!("POLLICORE_URL") {
        transport.set_pollicore_url(Some(url.to_string()));
        info!("✅ Pollicore URL (compile-time): {}", url);
    } else {
        info!("⚠️  POLLICORE_URL not set at compile time — submitIntent will fail");
    }

    if let Some(ref addr) = config.wallet_address {
        transport.set_wallet_address(Some(addr.clone()));
        info!("✅ Wallet address set: {}", addr);
    } else {
        info!("ℹ️  No wallet address provided — rewards will not be attributed until one is set");
    }

    Ok(transport)
}

/// Initialize the PolliNet SDK with a BLE-backed engine.
/// Returns a handle (index) to the initialized transport instance.
pub(crate) fn init_common(config_json: &[u8]) -> Result<i64, String> {
    let config: SdkConfig = serde_json::from_slice(config_json)
        .map_err(|e| format!("Failed to parse config: {}", e))?;
    apply_logging(&config);
    info!("📱 FFI init — RPC: {:?}", config.rpc_url);

    let transport = build_engine(&config)?;

    let transport_arc = Arc::new(transport);
    let core: Arc<dyn HostTransport> = transport_arc.clone();
    let mut transports = TRANSPORTS.lock();
    transports.push(Some(TransportEntry {
        kind: TransportKind::Ble,
        core,
        ble: Some(transport_arc),
    }));
    let handle = (transports.len() - 1) as i64;
    info!(
        "✅ PolliNet SDK initialized successfully with handle {}",
        handle
    );
    Ok(handle)
}

/// Initialize a standalone Wi-Fi Direct transport handle.
///
/// Mirrors [`init_common`] but wraps the engine in [`HostWifiDirectTransport`]
/// (same engine, larger default MTU). BLE-specific FFI calls reject this handle.
pub(crate) fn init_wifi_direct_common(config_json: &[u8]) -> Result<i64, String> {
    let config: SdkConfig = serde_json::from_slice(config_json)
        .map_err(|e| format!("Failed to parse config: {}", e))?;
    apply_logging(&config);
    info!("📶 FFI initWifiDirect — RPC: {:?}", config.rpc_url);

    let engine = build_engine(&config)?;

    let transport = HostWifiDirectTransport::from_engine(Arc::new(engine));
    let transport_arc = Arc::new(transport);
    let core: Arc<dyn HostTransport> = transport_arc.clone();
    let mut transports = TRANSPORTS.lock();
    transports.push(Some(TransportEntry {
        kind: TransportKind::WifiDirect,
        core,
        ble: None,
    }));
    let handle = (transports.len() - 1) as i64;
    info!(
        "✅ Wi-Fi Direct transport initialized with handle {}",
        handle
    );
    Ok(handle)
}

/// Initialize a Wi-Fi Direct handle that **shares the engine** of an existing BLE handle.
///
/// Both handles then share one deduplication set, outbound queue, and received queue, so
/// a transaction arriving over both radios is reassembled and submitted exactly once.
pub(crate) fn init_wifi_direct_sharing_common(ble_handle: i64) -> Result<i64, String> {
    let engine = get_transport(ble_handle)?; // Arc<HostBleTransport>, shared
    let transport = Arc::new(HostWifiDirectTransport::from_engine(engine.clone()));
    let core: Arc<dyn HostTransport> = transport;
    let mut transports = TRANSPORTS.lock();
    transports.push(Some(TransportEntry {
        kind: TransportKind::WifiDirect,
        core,
        // Expose the SHARED engine via the BLE surface too, so BLE-gated FFI
        // (confirmations: popConfirmation / relayConfirmation / confirmDelivered)
        // work on this Wi-Fi handle — enabling the Wi-Fi confirmation reverse-channel.
        ble: Some(engine),
    }));
    let handle = (transports.len() - 1) as i64;
    info!(
        "✅ Wi-Fi Direct handle {} sharing engine of BLE handle {}",
        handle, ble_handle
    );
    Ok(handle)
}

/// Return the transport kind for a handle ("BLE" | "WIFI_DIRECT"), or "" if invalid.
pub(crate) fn transport_kind_common(handle: i64) -> &'static str {
    let transports = TRANSPORTS.lock();
    transports
        .get(handle as usize)
        .and_then(|t| t.as_ref())
        .map(|e| e.kind.as_str())
        .unwrap_or("")
}

/// Shutdown the SDK handle and release its resources.
pub(crate) fn shutdown_common(handle: i64) {
    let mut transports = TRANSPORTS.lock();
    if handle >= 0 && (handle as usize) < transports.len() {
        transports[handle as usize] = None;
        tracing::info!("🛑 SDK handle {} shut down and invalidated", handle);
    }
}

/// Derive the Associated Token Account (ATA) address for owner wallet + token mint.
/// Stateless — no SDK handle required.
pub(crate) fn derive_associated_token_account_common(
    owner_str: &str,
    mint_str: &str,
) -> Result<String, String> {
    let owner = Pubkey::from_str(owner_str).map_err(|e| format!("Invalid owner: {}", e))?;
    let mint = Pubkey::from_str(mint_str).map_err(|e| format!("Invalid mint: {}", e))?;
    let ata = spl_associated_token_account::get_associated_token_address(&owner, &mint);
    Ok(ata.to_string())
}

// =============================================================================
// Host-driven transport API
// =============================================================================

pub(crate) fn op_push_inbound(handle: i64, data: Vec<u8>) -> Result<String, String> {
    let transport = get_core(handle)?;
    log::debug!("📡 pushInbound handle={} bytes={}", handle, data.len());
    transport.push_inbound(data)?;
    log::debug!("✅ pushInbound queued successfully");
    ok_json(())
}

pub(crate) fn next_outbound_common(handle: i64, max_len: usize) -> Result<Option<Vec<u8>>, String> {
    let transport = get_core(handle)?;
    Ok(transport.next_outbound(max_len))
}

pub(crate) fn op_tick(handle: i64, now_ms: u64) -> Result<String, String> {
    let transport = get_core(handle)?;
    let frames = transport.tick(now_ms);

    // Encode frames as JSON array of base64 strings
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    let encoded: Vec<String> = frames.iter().map(|f| BASE64.encode(f)).collect();
    ok_json(encoded)
}

pub(crate) fn op_metrics(handle: i64) -> Result<String, String> {
    let transport = get_core(handle)?;
    ok_json(transport.metrics())
}

pub(crate) fn op_clear_transaction(handle: i64, tx_id: &str) -> Result<String, String> {
    let transport = get_core(handle)?;
    transport.clear_transaction(tx_id);
    ok_json(())
}

/// Remove all outbound queue fragments that belong to `tx_id`.
/// Must be called when a BLE confirmation arrives (success or failure) so the
/// originating device stops re-broadcasting a transaction already handled by a
/// relay peer.
pub(crate) fn op_clear_outbound_transaction(handle: i64, tx_id: &str) -> Result<String, String> {
    let transport = get_core(handle)?;
    let removed = transport.clear_outbound_for_tx(tx_id);

    #[derive(serde::Serialize)]
    struct Out {
        removed: usize,
    }
    ok_json(Out { removed })
}

// =============================================================================
// Fragmentation API (M6)
// =============================================================================

pub(crate) fn op_fragment(
    handle: i64,
    tx_data: Vec<u8>,
    max_payload: Option<usize>,
) -> Result<String, String> {
    let transport = get_transport(handle)?;
    log::info!(
        "✂️  fragment handle={} input_bytes={} max_payload={:?}",
        handle,
        tx_data.len(),
        max_payload
    );

    let fragments = transport.queue_transaction(tx_data, max_payload)?;

    let total_fragment_bytes: usize = fragments.iter().map(|f| f.data.len()).sum();
    log::info!(
        "✅ fragment → {} fragments, {} total payload bytes",
        fragments.len(),
        total_fragment_bytes
    );

    ok_json(FragmentList { fragments })
}

/// Reconstruct a transaction from a JSON array of fragment objects with base64 data.
pub(crate) fn op_reconstruct_transaction(fragments_json: &[u8]) -> Result<String, String> {
    tracing::info!("🔗 FFI reconstructTransaction called");

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

    let fragment_data: Vec<FragmentData> = serde_json::from_slice(fragments_json)
        .map_err(|e| format!("Failed to parse fragments JSON: {}", e))?;

    tracing::info!("Reconstructing from {} fragments", fragment_data.len());

    let fragments: Vec<crate::ble::mesh::TransactionFragment> = fragment_data
        .iter()
        .map(|f| {
            let tx_id_bytes = hex::decode(&f.transaction_id)
                .map_err(|e| format!("Invalid transaction ID: {}", e))?;
            let tx_id: [u8; 32] = tx_id_bytes.try_into().map_err(|_| {
                "Transaction ID must be 32 bytes (64 hex chars)".to_string()
            })?;

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

    let reconstructed = crate::ble::reconstruct_transaction(&fragments)
        .map_err(|e| format!("Reconstruction failed: {}", e))?;

    tracing::info!("✅ Reconstructed transaction: {} bytes", reconstructed.len());

    ok_json(base64::encode(&reconstructed))
}

pub(crate) fn op_get_fragmentation_stats(tx_bytes: &[u8]) -> Result<String, String> {
    tracing::info!("📊 FFI getFragmentationStats called");

    let stats = crate::ble::FragmentationStats::calculate(tx_bytes);

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

    ok_json(StatsResponse {
        original_size: stats.original_size,
        fragment_count: stats.fragment_count,
        max_fragment_size: stats.max_fragment_size,
        avg_fragment_size: stats.avg_fragment_size,
        total_overhead: stats.total_overhead,
        efficiency: stats.efficiency,
    })
}

/// Prepare a transaction broadcast (fragments it and returns fragments with packets).
pub(crate) fn op_prepare_broadcast(tx_bytes: &[u8]) -> Result<String, String> {
    tracing::info!("📡 FFI prepareBroadcast called");
    tracing::info!("Preparing broadcast for {} byte transaction", tx_bytes.len());

    let fragments = crate::ble::fragment_transaction(tx_bytes);
    let transaction_id = fragments[0].transaction_id;

    let broadcaster = crate::ble::TransactionBroadcaster::new(uuid::Uuid::new_v4());

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

    ok_json(BroadcastPreparation {
        transaction_id: hex::encode(transaction_id),
        fragment_packets,
    })
}

// =============================================================================
// Mesh health
// =============================================================================

pub(crate) fn op_get_health_snapshot(handle: i64) -> Result<String, String> {
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

    ok_json(HealthSnapshotResponse { snapshot })
}

pub(crate) fn op_record_peer_heartbeat(handle: i64, peer_id: &str) -> Result<String, String> {
    tracing::info!("💓 FFI recordPeerHeartbeat called");
    let transport = get_transport(handle)?;
    transport.health_monitor().record_heartbeat(peer_id);
    tracing::info!("✅ Recorded heartbeat for peer: {}", peer_id);
    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_record_peer_latency(
    handle: i64,
    peer_id: &str,
    latency_ms: u32,
) -> Result<String, String> {
    tracing::info!("⏱️ FFI recordPeerLatency called");
    let transport = get_transport(handle)?;
    transport.health_monitor().record_latency(peer_id, latency_ms);
    tracing::info!("✅ Recorded {}ms latency for peer: {}", latency_ms, peer_id);
    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_record_peer_rssi(handle: i64, peer_id: &str, rssi: i8) -> Result<String, String> {
    tracing::info!("📶 FFI recordPeerRssi called");
    let transport = get_transport(handle)?;
    transport.health_monitor().record_rssi(peer_id, rssi);
    tracing::info!("✅ Recorded {}dBm RSSI for peer: {}", rssi, peer_id);
    ok_json(SuccessResponse { success: true })
}

// =============================================================================
// Received (auto-submission) queue
// =============================================================================

pub(crate) fn op_push_received_transaction(
    handle: i64,
    tx_bytes: Vec<u8>,
) -> Result<String, String> {
    let transport = get_core(handle)?;
    log::info!(
        "📥 pushReceivedTransaction handle={} bytes={}",
        handle,
        tx_bytes.len()
    );

    let added = transport.push_received_transaction(tx_bytes);

    #[derive(serde::Serialize)]
    struct PushResponse {
        added: bool,
        queue_size: usize,
    }

    let queue_size = transport.received_queue_size();
    if added {
        log::info!(
            "✅ pushReceivedTransaction accepted — queue_size={}",
            queue_size
        );
    } else {
        log::info!(
            "⚠️  pushReceivedTransaction duplicate/full — queue_size={}",
            queue_size
        );
    }

    ok_json(PushResponse { added, queue_size })
}

pub(crate) fn op_next_received_transaction(handle: i64) -> Result<String, String> {
    log::debug!(
        "🔍 FFI nextReceivedTransaction called with handle: {}",
        handle
    );
    let transport = get_core(handle)?;
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

            ok_json(ReceivedTransaction {
                tx_id,
                transaction_base64: BASE64.encode(&tx_bytes),
                received_at,
            })
        }
        None => {
            log::debug!("📭 No transaction in queue, returning None");
            ok_json(None::<String>)
        }
    }
}

pub(crate) fn op_get_received_queue_size(handle: i64) -> Result<String, String> {
    log::debug!("🔍 FFI getReceivedQueueSize called with handle: {}", handle);
    let transport = get_core(handle)?;
    ok_json(QueueSizeResponse {
        queue_size: transport.received_queue_size(),
    })
}

pub(crate) fn op_get_fragment_reassembly_info(handle: i64) -> Result<String, String> {
    log::debug!(
        "🔍 FFI getFragmentReassemblyInfo called with handle: {}",
        handle
    );
    let transport = get_transport(handle)?;
    let info_list = transport.get_fragment_reassembly_info();
    ok_json(FragmentReassemblyInfoList {
        transactions: info_list,
    })
}

pub(crate) fn op_mark_transaction_submitted(handle: i64, tx_bytes: &[u8]) -> Result<String, String> {
    let transport = get_transport(handle)?;
    // Log SHA-256 prefix for dedup tracing without logging the full tx
    let hash_prefix = {
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(tx_bytes);
        hex::encode(&h[..4])
    };
    log::info!(
        "🔖 markTransactionSubmitted handle={} sha256_prefix={} bytes={}",
        handle,
        hash_prefix,
        tx_bytes.len()
    );
    transport.mark_transaction_submitted(tx_bytes);
    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_cleanup_old_submissions(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    transport.cleanup_old_submissions();
    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_get_outbound_queue_size(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    ok_json(QueueSizeResponse {
        queue_size: transport.outbound_queue_size(),
    })
}

pub(crate) fn op_debug_outbound_queue(handle: i64) -> Result<String, String> {
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

    ok_json(QueueDebugResponse {
        total_fragments: fragments.len(),
        fragments,
    })
}

// =============================================================================
// Queue Persistence (Phase 5)
// =============================================================================

pub(crate) fn op_save_queues(handle: i64) -> Result<String, String> {
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

    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_auto_save_queues(handle: i64) -> Result<String, String> {
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
        if let Some(queue_storage_dir) = transport.get_queue_storage_dir() {
            if let Err(e) = transport.save_received_queue(&queue_storage_dir) {
                log::warn!("⚠️ Failed to auto-save received queue: {}", e);
            }
        }

        Ok::<(), String>(())
    })?;

    ok_json(SuccessResponse { success: true })
}

// =============================================================================
// Queue Management (Phase 2)
// =============================================================================

pub(crate) fn op_push_outbound_transaction(
    handle: i64,
    request_json: &str,
) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let request: PushOutboundRequest = serde_json::from_str(request_json)
        .map_err(|e| format!("Failed to parse request: {}", e))?;

    log::info!(
        "📤 pushOutboundTransaction handle={} tx_id={} fragments={} priority={:?}",
        handle,
        &request.tx_id[..8.min(request.tx_id.len())],
        request.fragments.len(),
        request.priority
    );

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

    let priority = match request.priority {
        PriorityFFI::High => crate::queue::Priority::High,
        PriorityFFI::Normal => crate::queue::Priority::Normal,
        PriorityFFI::Low => crate::queue::Priority::Low,
    };

    let outbound_tx =
        crate::queue::OutboundTransaction::new(request.tx_id, tx_bytes, fragments, priority);

    runtime::block_on(async {
        let mut queue = transport.sdk.queue_manager().outbound.write().await;
        queue
            .push(outbound_tx)
            .map_err(|e| format!("Failed to push to queue: {}", e))?;
        Ok::<(), String>(())
    })?;

    log::info!("✅ pushOutboundTransaction enqueued");
    ok_json(SuccessResponse { success: true })
}

/// Accept and queue a pre-signed transaction from external partners.
/// Verifies the transaction, compresses it if needed, fragments it, and adds to queue.
pub(crate) fn op_accept_and_queue_external_transaction(
    handle: i64,
    request_json: &str,
) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let request: AcceptExternalTransactionRequest = serde_json::from_str(request_json)
        .map_err(|e| format!("Failed to parse request: {}", e))?;

    let tx_id = runtime::block_on(async {
        // First, verify and queue in priority queue (for tracking/management)
        transport
            .sdk
            .accept_and_queue_external_transaction(&request.base64_signed_tx, request.max_payload)
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

        while let Some(tx) = queue.pop() {
            if tx.tx_id == tx_id {
                found_tx = Some(tx);
                break;
            } else {
                popped_txs.push(tx);
            }
        }

        // Put back all the transactions we popped (maintain original order)
        for tx in popped_txs {
            if let Err(e) = queue.push(tx) {
                tracing::warn!("⚠️ Failed to re-queue transaction: {}", e);
            }
        }

        if let Some(tx) = found_tx {
            let fragment_count = tx.fragments.len();

            transport
                .queue_fragments(&tx.fragments)
                .map_err(|e| format!("Failed to queue fragments: {}", e))?;

            queue
                .push(tx)
                .map_err(|e| format!("Failed to re-queue transaction: {}", e))?;

            tracing::info!(
                "✅ External transaction {} fragments added to transport outbound queue ({} fragments)",
                tx_id,
                fragment_count
            );
        } else {
            tracing::warn!(
                "⚠️ Could not find queued transaction {} to populate fragment queue",
                tx_id
            );
        }

        Ok::<(), String>(())
    })
    .map_err(|e| format!("Failed to populate fragment queue: {}", e))?;

    ok_json(tx_id)
}

pub(crate) fn op_pop_outbound_transaction(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let tx_opt = runtime::block_on(async {
        let mut queue = transport.sdk.queue_manager().outbound.write().await;
        queue.pop()
    });

    if let Some(tx) = tx_opt {
        log::info!(
            "📦 popOutboundTransaction → tx_id={} fragments={} priority={:?}",
            &tx.tx_id[..8.min(tx.tx_id.len())],
            tx.fragments.len(),
            tx.priority
        );
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
        ok_json(Some(tx_ffi))
    } else {
        log::debug!("📭 popOutboundTransaction — queue empty");
        ok_json(None::<OutboundTransactionFFI>)
    }
}

pub(crate) fn op_add_to_retry_queue(handle: i64, request_json: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let request: AddToRetryRequest = serde_json::from_str(request_json)
        .map_err(|e| format!("Failed to parse request: {}", e))?;

    let tx_bytes = base64::decode(&request.tx_bytes)
        .map_err(|e| format!("Invalid transaction bytes: {}", e))?;

    log::info!(
        "🔁 addToRetryQueue handle={} tx_id={} error={:?}",
        handle,
        &request.tx_id[..8.min(request.tx_id.len())],
        request.error
    );

    let retry_item = crate::queue::RetryItem::new(tx_bytes, request.tx_id, request.error);

    runtime::block_on(async {
        let mut queue = transport.sdk.queue_manager().retries.write().await;
        queue
            .push(retry_item)
            .map_err(|e| format!("Failed to push to retry queue: {}", e))?;
        Ok::<(), String>(())
    })?;

    log::info!("✅ addToRetryQueue enqueued");
    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_pop_ready_retry(handle: i64) -> Result<String, String> {
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
        ok_json(Some(retry_ffi))
    } else {
        ok_json(None::<RetryItemFFI>)
    }
}

pub(crate) fn op_get_retry_queue_size(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let size = runtime::block_on(async {
        let queue = transport.sdk.queue_manager().retries.read().await;
        queue.len()
    });

    ok_json(QueueSizeResponse { queue_size: size })
}

pub(crate) fn op_cleanup_expired(handle: i64) -> Result<String, String> {
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

    ok_json(CleanupExpiredResponse {
        confirmations_cleaned,
        retries_cleaned,
    })
}

/// Confirm that all fragments for `tx_id` were delivered to the current peer.
/// Decrements the transaction's relevance counter by 1. Evicts the transaction and
/// returns { removed: true } when relevance hits 0 (fan-out exhausted).
pub(crate) fn op_confirm_delivered(handle: i64, tx_id: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let removed = runtime::block_on(async {
        let mut queue = transport.sdk.queue_manager().outbound.write().await;
        queue.confirm_delivered(tx_id)
    });

    #[derive(serde::Serialize)]
    struct ConfirmDeliveredResponse {
        removed: bool,
    }
    ok_json(ConfirmDeliveredResponse { removed })
}

/// Peek at the highest-relevance transaction in the outbound queue and load its
/// fragments into the transport's BLE frame buffer so the sending loop can deliver
/// them to the current peer.
pub(crate) fn op_load_for_sending(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    // Peek under a read lock — clone the data we need so we don't hold the lock
    // while calling queue_fragments (which takes an unrelated mutex).
    let tx_info = runtime::block_on(async {
        let queue = transport.sdk.queue_manager().outbound.read().await;
        queue
            .peek_highest_relevance()
            .map(|tx| (tx.tx_id.clone(), tx.fragments.clone(), tx.relevance))
    });

    #[derive(serde::Serialize)]
    struct LoadResponse {
        tx_id: String,
        relevance: u8,
        fragment_count: usize,
    }

    if let Some((tx_id, fragments, relevance)) = tx_info {
        transport
            .queue_fragments(&fragments)
            .map_err(|e| format!("Failed to load fragments into transport: {}", e))?;

        ok_json(Some(LoadResponse {
            tx_id,
            relevance,
            fragment_count: fragments.len(),
        }))
    } else {
        ok_json(None::<LoadResponse>)
    }
}

/// Purge outbound transactions older than max_age_secs from all priority queues.
pub(crate) fn op_purge_stale_outbound(handle: i64, max_age_secs: u64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let removed = runtime::block_on(async {
        let mut outbound = transport.sdk.queue_manager().outbound.write().await;
        outbound.cleanup_stale(max_age_secs)
    });

    #[derive(serde::Serialize)]
    struct PurgeResponse {
        removed: usize,
    }
    ok_json(PurgeResponse { removed })
}

pub(crate) fn op_queue_confirmation(handle: i64, request_json: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let request: QueueConfirmationRequest = serde_json::from_str(request_json)
        .map_err(|e| format!("Failed to parse request: {}", e))?;

    tracing::info!(
        "📨 Queueing confirmation for tx {} with signature {}...",
        request.tx_id,
        &request.signature[..std::cmp::min(16, request.signature.len())]
    );

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

        let confirmation =
            crate::queue::confirmation::Confirmation::success(tx_id_array, request.signature.clone());

        conf_queue
            .push(confirmation)
            .map_err(|e| format!("Failed to queue confirmation: {:?}", e))?;

        Ok::<(), String>(())
    })?;

    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_pop_confirmation(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let confirmation = runtime::block_on(async {
        let mut conf_queue = transport.sdk.queue_manager().confirmations.write().await;
        conf_queue.pop()
    });

    if let Some(conf) = confirmation {
        let tx_id_hex = hex::encode(conf.original_tx_id);
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

        ok_json(Some(conf_ffi))
    } else {
        ok_json(None::<ConfirmationFFI>)
    }
}

pub(crate) fn op_cleanup_stale_fragments(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    // Cleanup stale fragments (older than 5 minutes = 300 seconds)
    let cleaned = runtime::block_on(async {
        let mut cache = transport.sdk.local_cache.write().await;
        cache.cleanup_stale_fragments(300)
    });

    #[derive(serde::Serialize)]
    struct CleanupResponse {
        fragments_cleaned: usize,
    }

    ok_json(CleanupResponse {
        fragments_cleaned: cleaned,
    })
}

/// Relay a received confirmation (increment hop count and re-queue for relay).
pub(crate) fn op_relay_confirmation(handle: i64, confirmation_json: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let conf_ffi: ConfirmationFFI = serde_json::from_str(confirmation_json)
        .map_err(|e| format!("Failed to parse confirmation: {}", e))?;

    tracing::info!(
        "🔄 Relaying confirmation for tx {} (current hops: {})",
        &conf_ffi.tx_id[..std::cmp::min(16, conf_ffi.tx_id.len())],
        conf_ffi.relay_count
    );

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
        return ok_json(SuccessResponse { success: true });
    }

    let relay_count_after = confirmation.relay_count;

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

    ok_json(SuccessResponse { success: true })
}

/// Clear all queues (outbound, retry, confirmation, received) and reassembly buffers.
/// Note: This does NOT clear nonce data.
pub(crate) fn op_clear_all_queues(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;

    runtime::block_on(async {
        transport
            .sdk
            .clear_all_queues()
            .await
            .map_err(|e| format!("Failed to clear queues: {}", e))?;

        transport.clear_all_reassembly_buffers();
        transport.clear_received_queue();

        tracing::info!(
            "✅ Cleared all queues (outbound, retry, confirmation, received) and reassembly buffers"
        );

        Ok::<(), String>(())
    })?;

    ok_json(SuccessResponse { success: true })
}

// =============================================================================
// Wallet address — reward attribution
// =============================================================================

/// Set the wallet address for this node session. Empty string clears it.
pub(crate) fn op_set_wallet_address(handle: i64, addr: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let addr_opt = if addr.is_empty() {
        None
    } else {
        Some(addr.to_string())
    };
    transport.set_wallet_address(addr_opt);

    info!(
        "✅ Wallet address updated: {}",
        if addr.is_empty() { "<cleared>" } else { addr }
    );

    ok_json(SuccessResponse { success: true })
}

pub(crate) fn op_get_wallet_address(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let addr = transport.get_wallet_address().unwrap_or_default();

    #[derive(serde::Serialize)]
    struct WalletAddressResponse {
        address: String,
    }

    ok_json(WalletAddressResponse { address: addr })
}

// =============================================================================
// Intent protocol — stateless helpers (no SDK transport handle needed)
// =============================================================================

pub(crate) fn op_get_executor_pda() -> Result<String, String> {
    let (pda, bump) = crate::intent::executor_pda();
    log::info!("🏦 getExecutorPda → pda={} bump={}", pda, bump);
    ok_json(ExecutorPdaResponse {
        pda: pda.to_string(),
        bump,
    })
}

/// Builds a single unsigned transaction containing one `approve_checked` instruction
/// per entry in the request.
pub(crate) fn op_create_approve_transaction(request_json: &[u8]) -> Result<String, String> {
    let req: CreateApproveTransactionRequest = serde_json::from_slice(request_json)
        .map_err(|e| format!("Failed to parse request: {}", e))?;

    log::info!(
        "🔐 createApproveTransaction owner={} fee_payer={} blockhash={} tokens={}",
        req.owner_wallet,
        req.fee_payer,
        &req.recent_blockhash[..8],
        req.tokens.len()
    );
    for t in &req.tokens {
        log::info!(
            "   token: mint={} account={} amount={} decimals={}",
            t.mint_address,
            t.token_account,
            t.amount,
            t.decimals
        );
    }

    let owner: Pubkey = FromStr::from_str(&req.owner_wallet)
        .map_err(|e| format!("Invalid owner_wallet: {}", e))?;
    let fee_payer: Pubkey =
        FromStr::from_str(&req.fee_payer).map_err(|e| format!("Invalid fee_payer: {}", e))?;

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

    let tx_base64 =
        crate::intent::build_approve_transaction(&owner, &fee_payer, recent_blockhash, &approvals)?;

    log::info!(
        "✅ createApproveTransaction → executor_pda={} tx_base64_len={}",
        executor_pda_key,
        tx_base64.len()
    );
    ok_json(ApproveTransactionResponse {
        transaction: tx_base64,
        executor_pda: executor_pda_key.to_string(),
    })
}

/// Builds a single unsigned transaction with one `revoke` instruction per token account.
pub(crate) fn op_create_revoke_transaction(request_json: &[u8]) -> Result<String, String> {
    let req: CreateRevokeTransactionRequest = serde_json::from_slice(request_json)
        .map_err(|e| format!("Failed to parse request: {}", e))?;

    log::info!(
        "🔓 createRevokeTransaction owner={} fee_payer={} accounts={} program={}",
        req.owner_wallet,
        req.fee_payer,
        req.token_accounts.len(),
        req.token_program
    );

    let owner: Pubkey = FromStr::from_str(&req.owner_wallet)
        .map_err(|e| format!("Invalid owner_wallet: {}", e))?;
    let fee_payer: Pubkey =
        FromStr::from_str(&req.fee_payer).map_err(|e| format!("Invalid fee_payer: {}", e))?;

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

    log::info!(
        "✅ createRevokeTransaction → tx_base64_len={}",
        tx_base64.len()
    );
    ok_json(RevokeTransactionResponse {
        transaction: tx_base64,
    })
}

/// Serializes an Intent into the canonical 169-byte borsh layout and returns it as
/// base64. Generates a random 16-byte nonce unless `nonce_hex` is supplied.
pub(crate) fn op_create_intent_bytes(request_json: &[u8]) -> Result<String, String> {
    let req: CreateIntentBytesRequest = serde_json::from_slice(request_json)
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
        let pk: Pubkey = FromStr::from_str(s).map_err(|e| format!("Invalid {}: {}", field, e))?;
        Ok(pk.to_bytes())
    };

    let from = pubkey_bytes(&req.from, "from")?;
    let to = pubkey_bytes(&req.to, "to")?;
    let token_mint = pubkey_bytes(&req.token_mint, "token_mint")?;
    let gas_fee_payee = pubkey_bytes(&req.gas_fee_payee, "gas_fee_payee")?;

    let nonce: [u8; 16] = if let Some(hex_str) = &req.nonce_hex {
        let decoded = hex::decode(hex_str).map_err(|e| format!("Invalid nonce_hex: {}", e))?;
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
    log::info!(
        "✅ createIntentBytes → {} bytes (base64_len={}) nonce={}",
        169,
        encoded.len(),
        hex::encode(nonce)
    );
    ok_json(IntentBytesResponse {
        intent_bytes: encoded,
        nonce_hex: hex::encode(nonce),
    })
}

// =============================================================================
// Intent submission — delegates to crate::submission
// =============================================================================

/// Submit a signed intent to pollicore. The pollicore URL is baked in at compile
/// time from `POLLICORE_URL` in `.env`.
pub(crate) fn op_submit_intent(handle: i64, request_json: &[u8]) -> Result<String, String> {
    use crate::submission::{SubmitIntentRequest, SubmitIntentResponse};

    let transport = get_transport(handle)?;

    let pollicore_url = transport.get_pollicore_url().ok_or_else(|| {
        "POLLICORE_URL not configured — set it in .env before building".to_string()
    })?;

    let req: SubmitIntentRequest = serde_json::from_slice(request_json)
        .map_err(|e| format!("Failed to parse SubmitIntentRequest: {}", e))?;

    let resp: SubmitIntentResponse =
        crate::submission::submit_intent(&pollicore_url, &req).map_err(|e| e.to_string())?;

    ok_json(resp)
}

// =============================================================================
// Subsystem 1 — Density-adaptive rotation
// =============================================================================

pub(crate) fn op_record_scan_result(handle: i64, peer_id: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;
    transport.density_estimator.lock().record(peer_id);
    ok_json(true)
}

pub(crate) fn op_get_adaptive_params(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let params = transport.density_estimator.lock().compute_params();
    ok_json(params)
}

pub(crate) fn op_add_peer_to_cooldown(
    handle: i64,
    peer_id: &str,
    cooldown_ms: u64,
) -> Result<String, String> {
    let transport = get_transport(handle)?;
    transport.cooldown_list.lock().add(peer_id, cooldown_ms);
    ok_json(true)
}

pub(crate) fn op_is_peer_in_cooldown(handle: i64, peer_id: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let cooling = transport.cooldown_list.lock().is_cooling(peer_id);
    ok_json(cooling)
}

pub(crate) fn op_expire_oldest_cooldown(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let expired = transport.cooldown_list.lock().expire_oldest();
    ok_json(expired)
}

pub(crate) fn op_log_session_telemetry(handle: i64, telemetry_json: &str) -> Result<String, String> {
    let _transport = get_transport(handle)?;
    // Parse to validate the structure before logging.
    let _record: crate::ble::SessionTelemetry = serde_json::from_str(telemetry_json)
        .map_err(|e| format!("Invalid telemetry JSON: {}", e))?;
    log::info!("[SESSION_TELEMETRY] {}", telemetry_json);
    ok_json(true)
}

// =============================================================================
// Subsystem 2 — Per-peer materialized queue
// =============================================================================

/// Returns the list of tx_ids that should be sent to `peer_id` (4-byte hex compact ID).
pub(crate) fn op_outbound_for_peer(handle: i64, peer_hex: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let peer_bytes = hex::decode(peer_hex).map_err(|e| format!("Invalid peer_id_hex: {}", e))?;
    let peer_id: [u8; 4] = peer_bytes
        .try_into()
        .map_err(|_| "peer_id must be 4 bytes (8 hex chars)".to_string())?;

    let tx_ids = runtime::block_on(async {
        let queue = transport.sdk.queue_manager().outbound.read().await;
        queue
            .outbound_for_peer(&peer_id)
            .iter()
            .map(|tx| tx.tx_id.clone())
            .collect::<Vec<_>>()
    });

    ok_json(tx_ids)
}

/// Drain-conditional delivery confirmation (Subsystem 2). Call ONLY on mutual drain.
pub(crate) fn op_confirm_delivered_by_peer(
    handle: i64,
    tx_id: &str,
    peer_hex: &str,
) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let peer_bytes = hex::decode(peer_hex).map_err(|e| format!("Invalid peer_id_hex: {}", e))?;
    let peer_id: [u8; 4] = peer_bytes
        .try_into()
        .map_err(|_| "peer_id must be 4 bytes".to_string())?;

    let removed = runtime::block_on(async {
        let mut queue = transport.sdk.queue_manager().outbound.write().await;
        queue.confirm_delivered_by_peer(tx_id, &peer_id)
    });

    #[derive(serde::Serialize)]
    struct RemovedResponse {
        removed: bool,
    }
    ok_json(RemovedResponse { removed })
}

// =============================================================================
// Subsystem 3 — Confirmation-driven purge
// =============================================================================

/// Ingest a received (or locally generated) confirmation.
///
/// Verifies the Ed25519 signature against the bundled Pollicore public key.
/// Returns `{ purged: bool, added_to_carrier: bool }`; silently drops
/// tampered/unverifiable confirmations (success with both false).
pub(crate) fn op_ingest_confirmation(handle: i64, raw: &[u8]) -> Result<String, String> {
    let transport = get_transport(handle)?;

    let conf = crate::ble::MeshConfirmation::from_frame_bytes(raw)
        .map_err(|e| format!("Deserialize confirmation: {}", e))?;

    // Verify signature — POLLICORE_PUBKEY is the 32-byte Ed25519 verifying key
    // bundled at compile time. If not set, skip verification (dev mode only).
    let valid = if let Some(pk) = get_pollicore_pubkey() {
        conf.verify(&pk)
    } else {
        log::warn!("POLLICORE_PUBKEY not configured — skipping signature verification (dev mode)");
        true
    };

    #[derive(serde::Serialize)]
    struct IngestResult {
        purged: bool,
        added_to_carrier: bool,
    }

    if !valid {
        log::warn!(
            "Dropped tampered confirmation for tx_id_hash={}",
            hex::encode(conf.tx_id_hash)
        );
        return ok_json(IngestResult {
            purged: false,
            added_to_carrier: false,
        });
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
        transport
            .tombstones
            .lock()
            .insert(tx_id_hash_hex.clone(), tomb);
    }

    // Expire cooldown overrides so peers learn about this purge quickly
    {
        transport
            .cooldown_list
            .lock()
            .expire_not_delivered(&conf.delivered_to);
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
        runtime::block_on(async {
            let mut queue = transport.sdk.queue_manager().outbound.write().await;
            queue.push(tx).is_ok()
        })
    } else {
        false
    };

    log::info!(
        "ingestConfirmation txId={} purged={} added_to_carrier={}",
        tx_id_hash_hex,
        purged,
        added_to_carrier
    );

    ok_json(IngestResult {
        purged,
        added_to_carrier,
    })
}

/// Check if a tx_id_hash (hex) has an active tombstone.
pub(crate) fn op_is_tombstoned(handle: i64, hash_hex: &str) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let tombstoned = transport
        .tombstones
        .lock()
        .get(hash_hex)
        .map(|t| t.is_valid())
        .unwrap_or(false);
    #[derive(serde::Serialize)]
    struct TombResponse {
        tombstoned: bool,
    }
    ok_json(TombResponse { tombstoned })
}

/// Evict expired tombstones and expired cooldowns. Call in the periodic 10s tick.
pub(crate) fn op_periodic_maintenance(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    transport.tombstones.lock().retain(|_, t| t.is_valid());
    transport.cooldown_list.lock().evict_expired();
    ok_json(true)
}

/// Get the number of active tombstones (diagnostic only).
pub(crate) fn op_get_tombstone_count(handle: i64) -> Result<String, String> {
    let transport = get_transport(handle)?;
    let count = transport.tombstones.lock().len();
    #[derive(serde::Serialize)]
    struct CountResponse {
        count: usize,
    }
    ok_json(CountResponse { count })
}
