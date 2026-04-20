//! Intent submission transport layer.
//!
//! This module owns the outbound path from a signed intent to on-chain execution.
//! The separation from the FFI glue in `ffi/android.rs` is deliberate: everything
//! below this boundary is pure Rust with no JNI dependencies, which makes it
//! straightforward to add censorship-hardening strategies later — e.g. BLE relay
//! submission, Tor/onion routing, multi-endpoint fan-out, or proof-of-relay.
//!
//! # Extension points
//!
//! Add new transports by implementing [`SubmissionTransport`] and selecting among
//! them in [`submit_intent`] based on network conditions or configuration.

use serde::{Deserialize, Serialize};

// ─── Public request / response types ────────────────────────────────────────

/// Canonical payload sent to pollicore `/sdk/intents/submit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitIntentRequest {
    /// Base64-encoded 169-byte serialised intent.
    pub intent_bytes: String,
    /// Base64-encoded 64-byte Ed25519 signature over `intent_bytes`.
    pub signature: String,
    /// SPL token account the tokens will be debited from (base58).
    pub from_token_account: String,
    /// `"spl-token"` or `"token-2022"`.
    pub token_program: String,
}

/// Successful response from pollicore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitIntentResponse {
    pub ok: bool,
    /// Solana transaction signature confirming on-chain execution.
    pub tx_signature: String,
}

// ─── Transport trait ─────────────────────────────────────────────────────────

/// A submission transport delivers a signed intent to the Solana network.
///
/// Implement this trait to add new relay strategies (BLE, Tor, multi-endpoint…).
pub trait SubmissionTransport: Send + Sync {
    fn submit(&self, req: &SubmitIntentRequest) -> Result<SubmitIntentResponse, SubmissionError>;
}

// ─── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SubmissionError {
    /// HTTP-level error with status code and response body.
    Http { status: u16, body: String },
    /// Transport or serialization failure.
    Transport(String),
}

impl std::fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmissionError::Http { status, body } => {
                write!(f, "pollicore HTTP {}: {}", status, body)
            }
            SubmissionError::Transport(msg) => write!(f, "transport error: {}", msg),
        }
    }
}

// ─── HTTP transport (primary) ────────────────────────────────────────────────

/// Direct HTTPS transport to a pollicore endpoint.
pub struct HttpTransport {
    /// Base URL, e.g. `"https://pollicore-production.up.railway.app"`.
    pollicore_url: String,
}

impl HttpTransport {
    pub fn new(pollicore_url: impl Into<String>) -> Self {
        Self { pollicore_url: pollicore_url.into() }
    }
}

#[cfg(feature = "reqwest")]
impl SubmissionTransport for HttpTransport {
    fn submit(&self, req: &SubmitIntentRequest) -> Result<SubmitIntentResponse, SubmissionError> {
        let url = format!("{}/sdk/intents/submit", self.pollicore_url.trim_end_matches('/'));

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| SubmissionError::Transport(format!("failed to build HTTP client: {}", e)))?;

        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(req)
            .send()
            .map_err(|e| SubmissionError::Transport(format!("HTTP request failed: {}", e)))?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .map_err(|e| SubmissionError::Transport(format!("failed to read response body: {}", e)))?;

        if !(200..300).contains(&status) {
            return Err(SubmissionError::Http { status, body });
        }

        serde_json::from_str::<SubmitIntentResponse>(&body)
            .map_err(|e| SubmissionError::Transport(
                format!("failed to parse pollicore response: {} — body: {}", e, body)
            ))
    }
}

#[cfg(not(feature = "reqwest"))]
impl SubmissionTransport for HttpTransport {
    fn submit(&self, _req: &SubmitIntentRequest) -> Result<SubmitIntentResponse, SubmissionError> {
        Err(SubmissionError::Transport(
            "reqwest feature not enabled — rebuild with the android feature flag".into(),
        ))
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Submit a signed intent to pollicore and return the Solana tx signature.
///
/// Currently uses [`HttpTransport`] directly. Future versions will select a
/// transport based on network conditions (BLE relay, multi-endpoint fan-out, etc.).
pub fn submit_intent(
    pollicore_url: &str,
    req: &SubmitIntentRequest,
) -> Result<SubmitIntentResponse, SubmissionError> {
    log::info!("📤 submission::submit_intent → {}/sdk/intents/submit", pollicore_url.trim_end_matches('/'));
    log::info!("   intent_bytes (base64 len={}): {}…",
        req.intent_bytes.len(),
        &req.intent_bytes[..20.min(req.intent_bytes.len())]);
    log::info!("   signature   (base64 len={}): {}…",
        req.signature.len(),
        &req.signature[..20.min(req.signature.len())]);
    log::info!("   from_token_account={}", req.from_token_account);
    log::info!("   token_program={}", req.token_program);

    let transport = HttpTransport::new(pollicore_url);
    match transport.submit(req) {
        Ok(resp) => {
            log::info!("✅ submission succeeded — tx_signature={}", resp.tx_signature);
            Ok(resp)
        }
        Err(SubmissionError::Http { ref status, ref body }) => {
            log::error!("❌ submission HTTP {}: {}", status, body);
            Err(SubmissionError::Http { status: *status, body: body.clone() })
        }
        Err(SubmissionError::Transport(ref msg)) => {
            log::error!("❌ submission transport error: {}", msg);
            Err(SubmissionError::Transport(msg.clone()))
        }
    }
}
