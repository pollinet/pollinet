## PolliNet Android Integration Roadmap (Rust core + Kotlin Android)

### Purpose
Track the end-to-end work to ship an Android app and SDK that leverage the existing Rust PolliNet core for Solana transactions over BLE, integrating with Solana Mobile (MWA/Seed Vault) and Android BLE/GATT.

### Guiding Principles
- Keep Rust as the source of truth for protocol, crypto, transaction building, fragmentation, and offline flows.
- Keep Android/Kotlin for platform responsibilities: BLE stack, permissions, lifecycle, and Solana Mobile Wallet Adapter integration.
- Use narrow, byte-oriented FFI boundaries (JSON v1 for simplicity) and evolve later to a binary schema if needed.

---

## Milestones and Tasks

### M1 — Define Kotlin↔Rust FFI boundary and data formats (T1)
- Description
  - Specify the exact request/response payloads exchanged through FFI as JSON v1.
  - Choose encoding (JSON, UTF-8) and include a `version` field to allow non-breaking evolution.
  - Define error envelope for FFI calls (code, message).
- Deliverables
  - This TODO.md with schemas (see Schemas section).
  - Agreed function list and their inputs/outputs (see FFI Functions section).
- Acceptance criteria
  - All planned Android scenarios (unsigned tx build, signing, fragmentation, host-driven transport) are representable using the defined messages without ambiguity.

### M2 — Add host-driven BLE transport API in Rust (T2)
- Description
  - Provide a transport path that does not depend on Rust-own BLE adapters. Android BLE is driven by Kotlin; Rust only provides packetization/state.
  - New Rust APIs (internal) exposed via FFI:
    - `push_inbound(data: Vec<u8>)` — feed inbound bytes from GATT.
    - `next_outbound(max_len: usize) -> Option<Vec<u8>>` — retrieve next frame to send.
    - `tick(now_ms: u64) -> Vec<Vec<u8>>` — drive timers/acks/retries, return frames to send.
    - `metrics() -> MetricsSnapshot` — counters for diagnostics.
- Implementation notes
  - Reuse `transaction::fragment_transaction`/`reassemble_fragments` for app-level framing.
  - Keep existing `ble::bridge` for desktop examples; introduce a simple `HostBleBridge` for Android host mode (in-memory queues, no adapter).
- Acceptance criteria
  - Unit tests demonstrate deterministic push/drain behavior and reassembly.

### M3 — Create Rust FFI facade with global async runtime (T3)
- Description
  - Wrap async Rust APIs and expose a stable C/UniFFI interface for Kotlin.
  - Initialize a single-thread Tokio runtime once; run all async ops through it.
- Deliverables
  - `ffi/android.rs` (or UniFFI setup) exporting the functions listed below.
- Acceptance criteria
  - A sample JNI/UniFFI call can init, build an unsigned tx, and fragment it.

### M4 — Expose transaction builders via FFI (T4)
- Description
  - Expose: `create_unsigned_transaction`, `create_unsigned_spl_transaction`, `cast_unsigned_vote`, `prepare_offline_nonce_data`, `prepare_offline_bundle`.
  - Inputs are JSON requests; outputs are base64 strings or JSON blobs.
- Acceptance criteria
  - Roundtrip tests validate public key parsing, nonce fetch (when RPC configured), and output format.

### M5 — Expose signature helpers via FFI (T5)
- Description
  - `prepare_sign_payload(base64Tx)` → raw message bytes expected by signer (MWA or Keystore).
  - `apply_signature(base64Tx, signerPubkey, signatureBytes)` → updated base64 tx.
  - `verify_and_serialize(base64Tx)` → bincode-1 wire format bytes to submit/fragment.
- Acceptance criteria
  - Applying signatures for different roles (fee payer, sender, nonce authority) matches existing Rust logic (`add_signature`).

### M6 — Expose fragmentation APIs via FFI (T6)
- Description
  - `fragment(txBytes)` → JSON array of `Fragment`.
  - `reassemble(fragmentArray)` → `txBytes` with checksum verification.
  - `process_presigned_for_relay(base64SignedTx)` → `Fragment[]` (no sending).
- Acceptance criteria
  - Fragments match MTU sizing; checksum verified during reassembly; unit tests included.

### M7 — Android project + Rust build integration (T7)
- Description
  - Create Android library/app modules. Integrate `cargo-ndk` to build `.so` for `arm64-v8a`, `armeabi-v7a`, `x86_64`.
  - Gradle task to invoke cargo and package an AAR exposing the JNI/UniFFI Kotlin API.
- Acceptance criteria
  - `./gradlew :android:assemble` builds and includes Rust `.so` for all ABIs.

### M8 — Android BLE Service and GATT plumbing (T8)
- Description
  - Implement a Foreground Service that owns scanning/advertising (if needed), connect/reconnect, GATT services/characteristics, MTU to 512, and PHY 2M when available.
  - Handle Android 12+ permissions, background limits, and Doze.
- Acceptance criteria
  - Manual test: connect two devices, exchange payloads; service survives background.

### M9 — Bridge GATT callbacks to Rust FFI (T9)
- Description
  - On characteristic changed → `push_inbound(bytes)`.
  - Sending loop → `next_outbound(maxLen)` until empty; write to GATT.
  - Scheduler → periodic `tick(nowMs)`; write returned frames.
- Acceptance criteria
  - End-to-end: fragments produced in Rust are sent; receiver reassembles and reports completion.

### M10 — Solana Mobile Wallet Adapter + Seed Vault (T10)
- Description
  - Integrate MWA. If Seed Vault present: request signatures for payloads produced by `prepare_sign_payload`.
  - Return signature to Rust via `apply_signature` and proceed with fragment/relay.
- Acceptance criteria
  - Demo flow signs successfully on a supported Solana Mobile device.

### M11 — Android Keystore fallback signer (T11)
- Description
  - If MWA/Seed Vault unavailable, create Ed25519 keys in Android Keystore (StrongBox where available), user-auth guarded.
  - Sign payloads and return signatures to Rust via FFI.
- Acceptance criteria
  - Unit/instrumented tests for sign/verify; keys are non-exportable.

### M12 — Diagnostics UI (T12)
- Description
  - Screens: Scan/connect, Build Tx (SOL/SPL/Vote), Sign (MWA/Keystore), Relay/Receive, Metrics/Logs.
  - Show RSSI, MTU, throughput, retries, error codes; export logs.
- Acceptance criteria
  - Happy-path demo and fault injection (disconnect/low MTU) present meaningful diagnostics.

### M13 — Testing (T13)
- Description
  - Rust unit tests for transport and FFI marshalling.
  - Android instrumented tests for permissions, lifecycle, and basic BLE IO.
- Acceptance criteria
  - CI runs tests; flakiness kept under control; clear retry/backoff policies.

### M14 — CI/CD and release (T14)
- Description
  - GitHub Actions to build Rust for ABIs, produce AAR/APK, attach artifacts, sign if needed.
- Acceptance criteria
  - One-click release produces versioned artifacts.

### M15 — Documentation (T15)
- Description
  - Developer guide for building, integrating AAR, and using the Kotlin API.
  - Security notes for keys, BLE privacy, and nonce handling.
  - Example flows mapped to app screens and sample code.
- Acceptance criteria
  - New developers can complete the demo in under 30 minutes.

---

## FFI Functions (initial set)

Initialization
- `init(configBytes: ByteArray) -> Handle`
- `shutdown(handle: Handle)`
- `version() -> String`

Transaction Builders
- `create_unsigned_transaction(reqJson: ByteArray) -> Base64TxString`
- `create_unsigned_spl_transaction(reqJson: ByteArray) -> Base64TxString`
- `cast_unsigned_vote(reqJson: ByteArray) -> Base64TxString`
- `prepare_offline_nonce_data(nonceAccount: String) -> CachedNonceDataJson`
- `prepare_offline_bundle(reqJson: ByteArray) -> OfflineBundleJson`

Signing Helpers
- `prepare_sign_payload(base64Tx: String) -> ByteArray`
- `apply_signature(base64Tx: String, signerPubkey: String, signature: ByteArray) -> Base64TxString`
- `verify_and_serialize(base64Tx: String) -> ByteArray`

Fragmentation / Relay Prep
- `fragment(txBytes: ByteArray) -> FragmentsJson`
- `reassemble(fragmentsJson: ByteArray) -> TxBytes`
- `process_presigned_for_relay(base64SignedTx: String) -> FragmentsJson`

Host-driven Transport
- `push_inbound(data: ByteArray)`
- `next_outbound(maxLen: Int) -> ByteArray?`
- `tick(nowMs: Long) -> FrameListJson`
- `metrics() -> MetricsJson`
- `clear_transaction(txId: String)`

Error Envelope
- All FFI results returned as `{ "ok": true, "data": ... }` or `{ "ok": false, "code": "ERR_CODE", "message": "..." }`.

---

## JSON Schemas (v1)

Note: Pseudocode-style JSON; enforce with serde structs in Rust and Kotlin data classes.

CreateUnsignedTransactionRequest v1
```
{
  "version": 1,
  "sender": "<base58>",
  "recipient": "<base58>",
  "feePayer": "<base58>",
  "amount": 123456,
  "nonceAccount": "<base58>"
}
```

CreateUnsignedSplTransactionRequest v1
```
{
  "version": 1,
  "senderWallet": "<base58>",
  "recipientWallet": "<base58>",
  "feePayer": "<base58>",
  "mintAddress": "<base58>",
  "amount": 123,
  "nonceAccount": "<base58>"
}
```

CastUnsignedVoteRequest v1
```
{
  "version": 1,
  "voter": "<base58>",
  "proposalId": "<base58>",
  "voteAccount": "<base58>",
  "choice": 1,
  "feePayer": "<base58>",
  "nonceAccount": "<base58>"
}
```

PrepareOfflineBundleRequest v1
```
{
  "version": 1,
  "count": 10,
  "senderKeypair": "<ed25519 keypair bytes base64>",
  "bundleFile": "optional/path.json"
}
```

CachedNonceData v1 (response)
```
{
  "version": 1,
  "nonce_account": "<base58>",
  "authority": "<base58>",
  "blockhash": "<base58>",
  "lamports_per_signature": 5000,
  "cached_at": 1700000000,
  "used": false
}
```

OfflineTransactionBundle v1 (response)
```
{
  "version": 1,
  "nonce_caches": [CachedNonceData v1, ...],
  "max_transactions": 10,
  "created_at": 1700000000
}
```

Fragment v1
```
{
  "id": "tx_...",
  "index": 0,
  "total": 3,
  "data": "<bytes base64>",
  "fragment_type": "FragmentStart|FragmentContinue|FragmentEnd",
  "checksum": "<32 bytes base64>"
}
```

ProtocolEvent v1
```
{
  "type": "TransactionComplete|TextMessage|Error|Ack",
  "txId": "optional",
  "size": 123,
  "message": "optional"
}
```

MetricsSnapshot v1
```
{
  "fragmentsBuffered": 0,
  "transactionsComplete": 0,
  "reassemblyFailures": 0,
  "lastError": "",
  "updatedAt": 1700000000
}
```

---

## Android Work Breakdown

Project Setup
- Create modules: `app/`, `rust-ffi/` (AAR), `native/` (Rust crate reference).
- Configure `cargo-ndk` for ABIs: `arm64-v8a`, `armeabi-v7a`, `x86_64`.
- Gradle tasks to build Rust first, then package AAR that includes JNI/UniFFI Kotlin stubs.

BLE Service
- Foreground Service with Notification for long-running BLE work.
- Permissions for Android 12+: `BLUETOOTH_SCAN`, `BLUETOOTH_CONNECT`, `BLUETOOTH_ADVERTISE`; Android 10–11 location gating.
- Scan/connect, discover services/characteristics, enable notifications, negotiate MTU to 512, try PHY 2M.
- Reconnect/backoff, Doze handling, startForeground when active.

FFI Wiring
- Load `.so` on app start, call `init(config)`. Store Handle in a singleton.
- onCharacteristicChanged → `push_inbound(bytes)`.
- Sender loop calls `next_outbound(maxLen)` until null; write to GATT with writeType=NoResponse when possible; respect backpressure.
- Periodic `tick(nowMs)` in a Handler/Coroutine every 50–100ms; write any returned frames.

Signing Flows
- Seed Vault path (MWA):
  - Call `prepare_sign_payload(base64Tx)`, present MWA sheet, get signature, call `apply_signature(...)`.
- Keystore path:
  - Create non-exportable Ed25519 key, user auth if available; sign payload; `apply_signature(...)`.

Diagnostics/UI
- Screens for scan/connect, tx compose (SOL/SPL/Vote), sign, relay, receive.
- Metrics view from `metrics()`; export logs.

---

## Dependencies and Risks
- Dependencies
  - Solana Mobile Wallet Adapter availability on target devices.
  - Android MTU/PHY support varies by device; plan for 23–512 MTU.
- Risks & Mitigations
  - BLE flakiness → robust retries, foreground service, telemetry.
  - Nonce freshness → prefer durable nonce flow; verify or refresh when online.
  - Keystore differences across OEMs → feature-detect StrongBox, fall back to TEE.

---

## Out of Scope (for v1)
- iOS client.
- Mesh multi-hop routing at the Android level (Rust may simulate logical mesh; Android handles a single physical link).
- Binary FFI schema (consider after v1 for performance).

---

## Completion Checklist
- All M1–M15 acceptance criteria met.
- Demo video: build, sign (MWA + Keystore), relay, receive & submit.
- CI green for Rust + Android; artifacts published.


