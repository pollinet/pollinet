//! Wi-Fi Direct host-driven transport.
//!
//! This is an **adapter, not a fork**. It wraps the proven, radio-agnostic transport
//! engine ([`HostBleTransport`]) and delegates every byte-level operation to it, so the
//! mesh fragmenter, reassembly, deduplication, store-and-forward queue, retry/backoff,
//! and health monitor are reused verbatim. The *only* behavioral difference is the
//! default fragment payload size: where BLE caps fragments near `MAX_FRAGMENT_DATA`
//! (~468 B for a ~517 B GATT MTU), Wi-Fi Direct runs over a TCP socket inside the P2P
//! group and can carry much larger frames, so it substitutes [`WIFI_DIRECT_MAX_PAYLOAD`]
//! when a caller does not specify one.
//!
//! Routing, voting, polling, and Solana semantics are *not* referenced here — they live
//! in the shared layers above the [`HostTransport`] seam.

use super::host_transport::HostTransport;
use super::transport::HostBleTransport;
use super::types::{Fragment, MetricsSnapshot, TransportKind};
use crate::ble::mesh::TransactionFragment;
use crate::ble::MeshHealthMonitor;
use std::sync::Arc;

/// Default per-fragment payload size for Wi-Fi Direct, in bytes.
///
/// Chosen to sit comfortably below a typical 1500-byte Ethernet/TCP MTU after the
/// 4-byte length prefix and bincode container overhead, so frames never IP-fragment.
/// ~3× BLE's payload ⇒ ~3× fewer fragments per transaction. The shared fragmenter
/// clamps to `MAX_FRAGMENT_PAYLOAD_CEILING`, which is comfortably above this value.
pub const WIFI_DIRECT_MAX_PAYLOAD: usize = 1400;

/// Largest socket frame the platform driver should accept before treating the peer as
/// hostile/desynchronized (DoS guard for the length-prefixed framing). Informational —
/// enforced by the driver, exposed here so Rust and the platform agree on one number.
pub const WIFI_DIRECT_MAX_FRAME: usize = 16 * 1024;

/// Host-driven Wi-Fi Direct transport: a thin policy layer over the shared engine.
///
/// The engine is held by `Arc` so a Wi-Fi handle can **share the very same engine** as a
/// co-located BLE handle (see `from_engine`). When shared, both radios use one
/// `received_tx_hash_set` / `submitted_tx_hashes` / outbound queue, so the same
/// transaction arriving over BLE *and* Wi-Fi is reassembled and submitted exactly once —
/// dual-transport deduplication for free.
pub struct HostWifiDirectTransport {
    /// The shared, radio-agnostic transport engine. All reassembly/dedup/queue/metrics
    /// state lives here and is reused unchanged — this is the "reuse, don't rebuild" core.
    engine: Arc<HostBleTransport>,
    /// Fragment payload substituted when `queue_transaction` is called with `None`.
    default_payload: usize,
}

impl HostWifiDirectTransport {
    /// Wrap an existing engine. Pass the *same* `Arc<HostBleTransport>` that backs a BLE
    /// handle to get cross-transport dedup; pass a fresh engine for a standalone Wi-Fi node.
    pub fn from_engine(engine: Arc<HostBleTransport>) -> Self {
        Self {
            engine,
            default_payload: WIFI_DIRECT_MAX_PAYLOAD,
        }
    }

    /// Create a standalone Wi-Fi Direct transport (own engine) without an RPC client.
    pub async fn new() -> Result<Self, String> {
        tracing::info!("🚀 HostWifiDirectTransport::new() — Wi-Fi Direct adapter over shared engine");
        Ok(Self::from_engine(Arc::new(HostBleTransport::new().await?)))
    }

    /// Create a standalone Wi-Fi Direct transport (own engine) with an RPC client.
    pub async fn new_with_rpc(rpc_url: &str) -> Result<Self, String> {
        tracing::info!(
            "🚀 HostWifiDirectTransport::new_with_rpc() — Wi-Fi Direct adapter (RPC: {})",
            rpc_url
        );
        Ok(Self::from_engine(Arc::new(
            HostBleTransport::new_with_rpc(rpc_url).await?,
        )))
    }

    /// Override the default fragment payload (e.g. to tune for a measured link MTU).
    pub fn set_default_payload(&mut self, payload: usize) {
        self.default_payload = payload.max(64);
    }

    /// Borrow the underlying engine for shared configuration and BLE-parity helpers.
    pub fn engine(&self) -> &HostBleTransport {
        &self.engine
    }

    /// Clone the shared engine `Arc` (e.g. to register a paired BLE handle).
    pub fn engine_arc(&self) -> Arc<HostBleTransport> {
        self.engine.clone()
    }

    /// Health monitor (reused from the engine — Wi-Fi peer events feed the same metrics).
    pub fn health_monitor(&self) -> Arc<MeshHealthMonitor> {
        self.engine.health_monitor()
    }
}

/// Delegates the entire byte-level contract to the shared engine. The single override is
/// `queue_transaction`, which fills in the larger Wi-Fi default payload when the caller
/// passes `None` — every other method is identical to BLE by construction.
impl HostTransport for HostWifiDirectTransport {
    fn push_inbound(&self, data: Vec<u8>) -> Result<(), String> {
        self.engine.push_inbound(data)
    }

    fn next_outbound(&self, max_len: usize) -> Option<Vec<u8>> {
        self.engine.next_outbound(max_len)
    }

    fn queue_transaction(
        &self,
        tx_bytes: Vec<u8>,
        max_payload: Option<usize>,
    ) -> Result<Vec<Fragment>, String> {
        // The only Wi-Fi-specific behavior: default to the larger payload when unspecified.
        let effective = max_payload.or(Some(self.default_payload));
        self.engine.queue_transaction(tx_bytes, effective)
    }

    fn queue_fragments(&self, fragments: &[TransactionFragment]) -> Result<(), String> {
        self.engine.queue_fragments(fragments)
    }

    fn pop_completed(&self) -> Option<(String, Vec<u8>)> {
        self.engine.pop_completed()
    }

    fn push_received_transaction(&self, tx_bytes: Vec<u8>) -> bool {
        self.engine.push_received_transaction(tx_bytes)
    }

    fn next_received_transaction(&self) -> Option<(String, Vec<u8>, u64)> {
        self.engine.next_received_transaction()
    }

    fn received_queue_size(&self) -> usize {
        self.engine.received_queue_size()
    }

    fn tick(&self, now_ms: u64) -> Vec<Vec<u8>> {
        self.engine.tick(now_ms)
    }

    fn metrics(&self) -> MetricsSnapshot {
        self.engine.metrics()
    }

    fn clear_transaction(&self, tx_id: &str) {
        self.engine.clear_transaction(tx_id)
    }

    fn clear_outbound_for_tx(&self, tx_id: &str) -> usize {
        self.engine.clear_outbound_for_tx(tx_id)
    }

    fn kind(&self) -> TransportKind {
        TransportKind::WifiDirect
    }

    fn default_max_payload(&self) -> usize {
        self.default_payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wifi_transport_creation() {
        let t = HostWifiDirectTransport::new().await.unwrap();
        assert_eq!(t.kind(), TransportKind::WifiDirect);
        assert_eq!(t.default_max_payload(), WIFI_DIRECT_MAX_PAYLOAD);
        assert!(t.next_outbound(1400).is_none());
    }

    /// A multi-fragment transaction round-trips through the Wi-Fi transport using the
    /// larger MTU: queue → drain outbound frames → push back into inbound → reassemble.
    #[tokio::test]
    async fn test_wifi_loopback_multifragment() {
        let tx = HostWifiDirectTransport::new().await.unwrap();
        let rx = HostWifiDirectTransport::new().await.unwrap();

        // ~4 KB transaction: at 1400 B payload this is a handful of large fragments,
        // far fewer than BLE's 468 B would produce.
        let payload: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();

        let frags = tx.queue_transaction(payload.clone(), None).unwrap();
        assert!(
            frags.len() >= 3,
            "expected multiple fragments, got {}",
            frags.len()
        );

        // Move every outbound frame to the receiver, exactly as the socket writer/reader
        // loops would. Use a generous max_len reflecting the Wi-Fi frame budget.
        let mut moved = 0;
        while let Some(frame) = tx.next_outbound(WIFI_DIRECT_MAX_FRAME) {
            rx.push_inbound(frame).unwrap();
            moved += 1;
        }
        assert_eq!(moved, frags.len(), "all fragments should be drained & delivered");

        // The receiver reassembled exactly one transaction, byte-identical.
        let (_id, bytes) = rx.pop_completed().expect("reassembled transaction");
        assert_eq!(bytes, payload);
    }

    /// Fragments larger than BLE's cap are actually produced (proves the MTU override
    /// and the raised fragmenter ceiling are in effect).
    #[tokio::test]
    async fn test_wifi_uses_larger_fragments() {
        let tx = HostWifiDirectTransport::new().await.unwrap();
        let payload = vec![7u8; 4096];
        let frags = tx.queue_transaction(payload, None).unwrap();
        let max_data = frags.iter().map(|f| {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            STANDARD.decode(&f.data).map(|d| d.len()).unwrap_or(0)
        }).max().unwrap();
        assert!(
            max_data > crate::ble::mesh::MAX_FRAGMENT_DATA,
            "Wi-Fi fragment data ({max_data}B) should exceed BLE's {}B cap",
            crate::ble::mesh::MAX_FRAGMENT_DATA
        );
    }

    /// Duplicate suppression is inherited from the shared engine: the same transaction
    /// delivered twice is only queued for submission once.
    #[tokio::test]
    async fn test_wifi_duplicate_suppression() {
        let tx = HostWifiDirectTransport::new().await.unwrap();
        let rx = HostWifiDirectTransport::new().await.unwrap();
        let payload = vec![9u8; 2048];

        // First full delivery.
        tx.queue_transaction(payload.clone(), None).unwrap();
        let mut frames = Vec::new();
        while let Some(f) = tx.next_outbound(WIFI_DIRECT_MAX_FRAME) {
            frames.push(f);
        }
        for f in &frames {
            rx.push_inbound(f.clone()).unwrap();
        }
        assert_eq!(rx.received_queue_size(), 1);

        // Replay the identical frames — content-hash dedup must keep the queue at 1.
        for f in &frames {
            // Duplicate fragments are ignored (Ok), reassembly won't re-fire.
            let _ = rx.push_inbound(f.clone());
        }
        assert_eq!(
            rx.received_queue_size(),
            1,
            "replayed transaction must not be queued twice"
        );
    }

    /// Dual-transport dedup (C3.4): a BLE handle and a Wi-Fi handle that **share one
    /// engine** must process the same transaction exactly once, even when it arrives over
    /// both radios.
    #[tokio::test]
    async fn test_shared_engine_cross_transport_dedup() {
        let engine = Arc::new(HostBleTransport::new().await.unwrap());
        let wifi = HostWifiDirectTransport::from_engine(engine.clone());

        // Sender fragments a transaction at the Wi-Fi MTU.
        let sender = HostWifiDirectTransport::new().await.unwrap();
        let payload = vec![5u8; 3000];
        sender.queue_transaction(payload.clone(), None).unwrap();
        let mut frames = Vec::new();
        while let Some(f) = sender.next_outbound(WIFI_DIRECT_MAX_FRAME) {
            frames.push(f);
        }

        // Arrives first over BLE (push straight into the shared engine).
        for f in &frames {
            let _ = engine.push_inbound(f.clone());
        }
        assert_eq!(engine.received_queue_size(), 1);

        // The very same transaction then arrives over Wi-Fi (shared engine) — no double.
        for f in &frames {
            let _ = wifi.push_inbound(f.clone());
        }
        assert_eq!(
            engine.received_queue_size(),
            1,
            "tx seen over BLE+Wi-Fi on a shared engine must be queued once"
        );
        // Both handles observe the same shared queue.
        assert_eq!(wifi.received_queue_size(), 1);
    }

    /// Metrics flow through the shared `MetricsSnapshot` for Wi-Fi handles (C5.3):
    /// a completed reassembly increments `transactions_complete` and clears the buffer.
    #[tokio::test]
    async fn test_wifi_metrics_snapshot() {
        let tx = HostWifiDirectTransport::new().await.unwrap();
        let rx = HostWifiDirectTransport::new().await.unwrap();

        assert_eq!(rx.metrics().transactions_complete, 0);

        tx.queue_transaction(vec![4u8; 2500], None).unwrap();
        while let Some(f) = tx.next_outbound(WIFI_DIRECT_MAX_FRAME) {
            rx.push_inbound(f).unwrap();
        }

        let m = rx.metrics();
        assert_eq!(m.transactions_complete, 1, "one tx should be reassembled");
        assert_eq!(m.fragments_buffered, 0, "buffers cleared after reassembly");
        assert_eq!(m.reassembly_failures, 0);
    }
}
