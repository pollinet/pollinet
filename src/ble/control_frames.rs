//! Control frame types for Subsystem 3 — Confirmation-driven purge.
//!
//! Extends the base PacketType with four new types starting at 0x08.
//! All new frame types are single-BLE-fragment (no sub-fragmentation).

use serde::{Deserialize, Serialize};

/// Extended packet type byte values.
/// 0x01–0x07 are defined in `mesh.rs`. These extend that space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ControlFrameType {
    /// Signed Pollicore confirmation (SUCCESS or FAILURE_TERMINAL).
    Confirmation = 0x08,
    /// Abort signal: "stop reassembling txId; a confirmation follows."
    TxAbort = 0x09,
    /// Mutual-drain signal: "I have sent everything I have for you."
    DrainReady = 0x0A,
    /// Handshake close acknowledgment.
    CloseAck = 0x0B,
}

impl ControlFrameType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x08 => Some(Self::Confirmation),
            0x09 => Some(Self::TxAbort),
            0x0A => Some(Self::DrainReady),
            0x0B => Some(Self::CloseAck),
            _ => None,
        }
    }
}

/// Confirmation status as carried over the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ConfirmationStatus {
    /// Solana accepted the transaction.
    Success = 1,
    /// Terminal failure (bad signature, insufficient funds, expired intent, …).
    FailureTerminal = 2,
}

impl ConfirmationStatus {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Success),
            2 => Some(Self::FailureTerminal),
            _ => None,
        }
    }
}

/// TTL for confirmation carrier entries (10 minutes > max tx TTL of 5 minutes).
pub const CONFIRMATION_TTL_SECS: u64 = 600;

/// Signed Pollicore confirmation. Propagates through the mesh like a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfirmation {
    /// First 16 bytes of SHA-256(original_tx_id_hex).
    pub tx_id_hash: [u8; 16],
    /// SUCCESS or FAILURE_TERMINAL.
    pub status: ConfirmationStatus,
    /// Relay hop count, capped at MAX_TX_RELAY_HOPS.
    pub hop_count: u8,
    /// Ed25519 signature over borsh(tx_id_hash ++ status ++ slot_or_error).
    /// Stored as Vec<u8> (64 bytes) because serde only auto-impls arrays up to [u8; 32].
    pub signature: Vec<u8>,
    /// On SUCCESS: Solana slot or tx signature bytes. On failure: short reason code.
    pub slot_or_error: Vec<u8>,
    // Carrier-set fields (mirroring OutboundTransaction)
    pub relevance: u8,
    /// Compact peer IDs already delivered to (4 bytes each, flat).
    pub delivered_to: Vec<u8>,
    pub added_at: u64,
}

impl MeshConfirmation {
    pub fn new(
        tx_id_hash: [u8; 16],
        status: ConfirmationStatus,
        signature: [u8; 64],
        slot_or_error: Vec<u8>,
    ) -> Self {
        Self {
            tx_id_hash,
            status,
            hop_count: 0,
            signature: signature.to_vec(),
            slot_or_error,
            relevance: 10,
            delivered_to: Vec::new(),
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// True if this confirmation has not expired.
    pub fn is_alive(&self) -> bool {
        let age = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(self.added_at);
        age < CONFIRMATION_TTL_SECS
    }

    /// Serialize the signable payload: tx_id_hash || status_byte || slot_or_error
    pub fn signable_payload(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(16 + 1 + self.slot_or_error.len());
        buf.extend_from_slice(&self.tx_id_hash);
        buf.push(self.status as u8);
        buf.extend_from_slice(&self.slot_or_error);
        buf
    }

    /// Verify the Ed25519 signature against `pollicore_pubkey` (32-byte verifying key).
    /// Returns true if valid. Silently returns false on any error.
    pub fn verify(&self, pollicore_pubkey: &[u8; 32]) -> bool {
        use ed25519_dalek::{Signature, VerifyingKey, Verifier};
        let Ok(vk) = VerifyingKey::from_bytes(pollicore_pubkey) else {
            return false;
        };
        let sig_bytes: [u8; 64] = match self.signature.as_slice().try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(&self.signable_payload(), &sig).is_ok()
    }

    /// Serialize to bytes for BLE frame payload (bincode v1 API).
    pub fn to_frame_bytes(&self) -> Result<Vec<u8>, String> {
        bincode1::serialize(self).map_err(|e| format!("Confirmation serialize: {}", e))
    }

    /// Deserialize from BLE frame payload bytes (bincode v1 API).
    pub fn from_frame_bytes(data: &[u8]) -> Result<Self, String> {
        bincode1::deserialize(data).map_err(|e| format!("Confirmation deserialize: {}", e))
    }
}

/// Tombstone — local-only, never transmitted.
#[derive(Debug, Clone)]
pub struct Tombstone {
    pub tx_id_hash: [u8; 16],
    /// Stop accepting fragments/entries for this tx until this timestamp (unix secs).
    pub until: u64,
}

impl Tombstone {
    /// Create a tombstone valid for 2 × original_tx_ttl_secs.
    pub fn new(tx_id_hash: [u8; 16], original_tx_ttl_secs: u64) -> Self {
        let until = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + 2 * original_tx_ttl_secs;
        Self { tx_id_hash, until }
    }

    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now < self.until
    }
}

/// TX_ABORT frame payload — just the tx_id_hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxAbortFrame {
    pub tx_id_hash: [u8; 16],
}

/// DRAIN_READY / CLOSE_ACK frames carry no payload — the type byte is sufficient.
/// This zero-sized struct is kept for symmetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyControlFrame;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_frame_type_roundtrip() {
        assert_eq!(ControlFrameType::from_u8(0x08), Some(ControlFrameType::Confirmation));
        assert_eq!(ControlFrameType::from_u8(0x09), Some(ControlFrameType::TxAbort));
        assert_eq!(ControlFrameType::from_u8(0x0A), Some(ControlFrameType::DrainReady));
        assert_eq!(ControlFrameType::from_u8(0x0B), Some(ControlFrameType::CloseAck));
        assert_eq!(ControlFrameType::from_u8(0x01), None);
    }

    #[test]
    fn test_tombstone_validity() {
        let hash = [0u8; 16];
        let tomb = Tombstone::new(hash, 300); // valid for 600 s
        assert!(tomb.is_valid());

        let expired = Tombstone { tx_id_hash: hash, until: 0 };
        assert!(!expired.is_valid());
    }

    #[test]
    fn test_confirmation_signable_payload() {
        let conf = MeshConfirmation::new(
            [1u8; 16],
            ConfirmationStatus::Success,
            [0u8; 64], // zero signature, not verified in this test
            vec![2, 3, 4],
        );
        let payload = conf.signable_payload();
        assert_eq!(&payload[..16], &[1u8; 16]);
        assert_eq!(payload[16], 1); // Success
        assert_eq!(&payload[17..], &[2, 3, 4]);
    }
}
