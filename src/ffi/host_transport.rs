//! Transport abstraction shared by all host-driven transports.
//!
//! Pollinet's radios (BLE today; Wi-Fi Direct, LoRa, satellite, internet later) are
//! all *host-driven*: the platform owns the hardware and pumps bytes through a tiny
//! contract, while the Rust core owns protocol state (fragmentation, reassembly,
//! deduplication, the store-and-forward queue, retries, health metrics).
//!
//! `HostTransport` is the seam. It captures exactly the byte-level contract that the
//! platform driver and FFI layer depend on, and nothing about routing, voting, polling,
//! or Solana semantics — those live in the shared layers above. Every concrete transport
//! (`HostBleTransport`, `HostWifiDirectTransport`, …) implements this same trait and, where
//! sensible, *delegates to the same engine* so behavior cannot drift between radios.
//!
//! The trait is intentionally **object-safe** so the FFI registry can store
//! `Arc<dyn HostTransport>` and select a transport by [`TransportKind`] at runtime.

use super::types::{Fragment, MetricsSnapshot, TransportKind};
use crate::ble::mesh::TransactionFragment;

/// The byte-level, radio-agnostic transport contract.
///
/// Implementors must be `Send + Sync` (handles are shared across the JNI/FFI threads
/// and the platform reader/writer loops).
pub trait HostTransport: Send + Sync {
    /// Push one inbound frame (a bincode1-serialized [`TransactionFragment`]) received
    /// from the radio. Reassembles, deduplicates, and enqueues completed transactions.
    fn push_inbound(&self, data: Vec<u8>) -> Result<(), String>;

    /// Pop the next outbound frame that fits within `max_len` bytes, if any.
    fn next_outbound(&self, max_len: usize) -> Option<Vec<u8>>;

    /// Fragment a full transaction and enqueue it for sending.
    ///
    /// When `max_payload` is `None`, each transport substitutes its own default
    /// ([`default_max_payload`](HostTransport::default_max_payload)) — this is the *only*
    /// place BLE and Wi-Fi Direct legitimately differ.
    fn queue_transaction(
        &self,
        tx_bytes: Vec<u8>,
        max_payload: Option<usize>,
    ) -> Result<Vec<Fragment>, String>;

    /// Enqueue already-built fragments directly (used by the external-tx accept path).
    fn queue_fragments(&self, fragments: &[TransactionFragment]) -> Result<(), String>;

    /// Pop the next fully reassembled transaction `(tx_id, bytes)`, if any.
    fn pop_completed(&self) -> Option<(String, Vec<u8>)>;

    /// Push a received transaction into the auto-submission queue. Returns `false` on
    /// duplicate (content-hash dedup — shared across transports).
    fn push_received_transaction(&self, tx_bytes: Vec<u8>) -> bool;

    /// Pop the next received transaction `(tx_id, bytes, received_at)` for submission.
    fn next_received_transaction(&self) -> Option<(String, Vec<u8>, u64)>;

    /// Count of transactions waiting for auto-submission.
    fn received_queue_size(&self) -> usize;

    /// Periodic tick for retry/timeout handling; returns frames to send, if any.
    fn tick(&self, now_ms: u64) -> Vec<Vec<u8>>;

    /// Current metrics snapshot.
    fn metrics(&self) -> MetricsSnapshot;

    /// Drop a specific transaction's reassembly buffer.
    fn clear_transaction(&self, tx_id: &str);

    /// Remove all outbound fragments for `tx_id` (on confirmation). Returns count removed.
    fn clear_outbound_for_tx(&self, tx_id: &str) -> usize;

    /// Which radio this transport drives.
    fn kind(&self) -> TransportKind;

    /// The fragment payload size used when `queue_transaction` is called with `None`.
    /// Informational for observability; BLE ≈ `MAX_FRAGMENT_DATA`, Wi-Fi Direct is larger.
    fn default_max_payload(&self) -> usize;
}
