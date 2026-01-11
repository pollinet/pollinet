//! PolliNet message schema for BLE transmission
//!
//! Implements the standardized message format for transaction propagation

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// PolliNet message for transaction propagation over BLE mesh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolliNetMessage {
    /// Unique message identifier
    pub id: String,

    /// Origin device DID (did:key or device DID)
    pub origin: String,

    /// Base64 encoded encrypted presigned transaction bytes
    pub tx_enc: String,

    /// Optional human-readable transaction metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_meta: Option<serde_json::Value>,

    /// Base64 Ed25519 signature over canonical message (without origin_sig)
    pub origin_sig: String,

    /// List of allowed submitters (relay IDs or app IDs)
    pub allowed_submitters: Vec<String>,

    /// Time-to-live (number of hops)
    pub ttl: u32,

    /// Message creation timestamp
    pub created_at: DateTime<Utc>,

    /// Message expiry timestamp
    pub expiry: DateTime<Utc>,

    /// Hop records (relay chain)
    #[serde(default)]
    pub hops: Vec<HopRecord>,

    /// Freeform metadata (size, fee, priority, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// Record of a relay hop in the message chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopRecord {
    /// Relay device DID
    pub relay: String,

    /// Timestamp when relayed
    pub ts: DateTime<Utc>,

    /// Base64 signature from relay
    pub relay_sig: String,
}

impl PolliNetMessage {
    /// Create a new PolliNet message
    pub fn new(
        origin: String,
        dest: String,
        tx_enc: String,
        origin_sig: String,
        allowed_submitters: Vec<String>,
        ttl: u32,
        expiry_duration: chrono::Duration,
    ) -> Self {
        let now = Utc::now();

        Self {
            id: Uuid::new_v4().to_string(),
            origin,
            tx_enc,
            tx_meta: None,
            origin_sig,
            allowed_submitters,
            ttl,
            created_at: now,
            expiry: now + expiry_duration,
            hops: Vec::new(),
            meta: None,
        }
    }

    /// Add metadata to the message
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Add transaction metadata
    pub fn with_tx_meta(mut self, tx_meta: serde_json::Value) -> Self {
        self.tx_meta = Some(tx_meta);
        self
    }

    /// Check if message has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expiry
    }

    /// Check if TTL is exhausted
    pub fn is_ttl_exhausted(&self) -> bool {
        self.ttl == 0
    }

    /// Add a hop record when relaying
    pub fn add_hop(&mut self, relay_did: String, relay_sig: String) {
        self.hops.push(HopRecord {
            relay: relay_did,
            ts: Utc::now(),
            relay_sig,
        });

        // Decrement TTL
        if self.ttl > 0 {
            self.ttl -= 1;
        }
    }

    /// Get canonical representation for signing (without origin_sig)
    pub fn canonical_for_signing(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut msg_copy = self.clone();
        msg_copy.origin_sig = String::new(); // Clear signature for canonical form

        // Serialize in deterministic order
        serde_json::to_vec(&msg_copy)
    }

    /// Serialize to JSON bytes for transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from JSON bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// Get estimated size in bytes
    pub fn estimated_size(&self) -> usize {
        serde_json::to_vec(self).map(|v| v.len()).unwrap_or(0)
    }

    /// Check if this device can submit the transaction
    pub fn can_submit(&self, submitter_id: &str) -> bool {
        self.allowed_submitters.contains(&submitter_id.to_string())
            || self.allowed_submitters.contains(&"any".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_pollinet_message_creation() {
        let msg = PolliNetMessage::new(
            "did:pollinet:alice".to_string(),
            "did:pollinet:bob".to_string(),
            "BASE64_ENCODED_TX".to_string(),
            "BASE64_SIGNATURE".to_string(),
            vec!["relay:001".to_string(), "pollinet-app".to_string()],
            6,
            chrono::Duration::minutes(5),
        );

        assert!(!msg.id.is_empty());
        assert_eq!(msg.origin, "did:pollinet:alice");
        assert_eq!(msg.ttl, 6);
        assert!(!msg.is_expired());
        assert!(!msg.is_ttl_exhausted());
    }

    #[test]
    fn test_message_with_metadata() {
        let msg = PolliNetMessage::new(
            "did:pollinet:alice".to_string(),
            "any-relay".to_string(),
            "BASE64_TX".to_string(),
            "BASE64_SIG".to_string(),
            vec!["any".to_string()],
            3,
            chrono::Duration::minutes(2),
        )
        .with_meta(json!({"size": 1184, "fee": 100, "priority": "high"}))
        .with_tx_meta(json!({"type": "transfer", "amount": "1.5 SOL"}));

        assert!(msg.meta.is_some());
        assert!(msg.tx_meta.is_some());
    }

    #[test]
    fn test_hop_addition() {
        let mut msg = PolliNetMessage::new(
            "did:pollinet:alice".to_string(),
            "any-relay".to_string(),
            "BASE64_TX".to_string(),
            "BASE64_SIG".to_string(),
            vec!["any".to_string()],
            3,
            chrono::Duration::minutes(2),
        );

        assert_eq!(msg.ttl, 3);
        assert_eq!(msg.hops.len(), 0);

        msg.add_hop("did:pollinet:relay1".to_string(), "RELAY_SIG_1".to_string());

        assert_eq!(msg.ttl, 2);
        assert_eq!(msg.hops.len(), 1);
        assert_eq!(msg.hops[0].relay, "did:pollinet:relay1");
    }

    #[test]
    fn test_serialization() {
        let msg = PolliNetMessage::new(
            "did:pollinet:alice".to_string(),
            "did:pollinet:bob".to_string(),
            "BASE64_TX".to_string(),
            "BASE64_SIG".to_string(),
            vec!["relay:001".to_string()],
            6,
            chrono::Duration::minutes(5),
        );

        let bytes = msg.to_bytes().expect("Should serialize");
        let deserialized = PolliNetMessage::from_bytes(&bytes).expect("Should deserialize");

        assert_eq!(msg.id, deserialized.id);
        assert_eq!(msg.origin, deserialized.origin);
    }
}
