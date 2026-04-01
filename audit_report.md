# 🔐 Security Review — Pollinet

---

## Scope

|                                  |                                                        |
| -------------------------------- | ------------------------------------------------------ |
| **Mode**                         | default (all `.rs` files)                              |
| **Framework**                    | Native Rust / Solana SDK client library (no on-chain program) |
| **Files reviewed**               | `src/storage.rs` · `src/nonce/mod.rs` · `src/queue/mod.rs`<br>`src/queue/storage.rs` · `src/ffi/android.rs` · `src/ffi/transport.rs`<br>`src/ffi/types.rs` · `src/ble/broadcaster.rs` · `src/ble/fragmenter.rs`<br>`src/ble/mesh.rs` · `src/ble/health_monitor.rs` · `src/lib.rs` |
| **Attack vectors checked**       | 120 (adapted from on-chain vectors to SDK/client threat model) |
| **Agents deployed**              | 1 (direct review)                                      |
| **Confidence threshold (1-100)** | 75                                                     |

---

## Findings

[90] **1. Hardcoded Fallback AES-256-GCM Encryption Key**

`src/storage.rs::SecureStorage::get_encryption_key` · Confidence: 90

**Description**
When `POLLINET_ENCRYPTION_KEY` is absent, the key silently falls back to the hardcoded string `"pollinet-default-encryption-key"`, making the SHA-256-derived key deterministic and publicly known; any attacker with read access to the storage directory can derive the same 32-byte key and decrypt the nonce bundle to extract private nonce accounts and blockhashes.

**Fix**

```diff
- let key_str = env::var("POLLINET_ENCRYPTION_KEY")
-     .unwrap_or_else(|_| "pollinet-default-encryption-key".to_string());
+ let key_str = env::var("POLLINET_ENCRYPTION_KEY")
+     .map_err(|_| StorageError::Encryption(
+         "POLLINET_ENCRYPTION_KEY must be set — no insecure fallback".to_string()
+     ))?;
```

---

[85] **2. Nonce Account DataSize Filter Returns Wrong Size — Discovery Always Fails**

`src/nonce/mod.rs::find_nonce_accounts_by_authority` · Confidence: 85

**Description**
`parse_nonce_account` rejects any input that is not exactly 80 bytes (line 149), but `find_nonce_accounts_by_authority` uses `RpcFilterType::DataSize(128)` (line 220); since real Solana nonce accounts are 80 bytes, the RPC filter never matches any nonce account, so automatic nonce discovery always returns an empty list and offline bundle refresh is silently broken.

**Fix**

```diff
- RpcFilterType::DataSize(128),
+ RpcFilterType::DataSize(80),
```

---

[80] **3. Unbounded Inbound Fragment Buffer Enables Memory Exhaustion**

`src/ffi/transport.rs::HostBleTransport::push_inbound` · Confidence: 80

**Description**
`inbound_buffers` (a `HashMap<String, Vec<TransactionFragment>>`) grows without any size cap; a malicious BLE peer can flood the transport with fragments referencing many distinct transaction IDs without ever completing reassembly, consuming unbounded heap memory and crashing the Android process.

**Fix**

```diff
+ const MAX_PENDING_TRANSACTIONS: usize = 64;
+ const MAX_FRAGMENTS_PER_TRANSACTION: usize = 256;

  let mut buffers = self.inbound_buffers.lock();
+ if buffers.len() >= MAX_PENDING_TRANSACTIONS && !buffers.contains_key(&tx_id) {
+     t_warn!("⚠️ Inbound buffer full ({} pending txs), dropping fragment for {}", buffers.len(), tx_id);
+     return Err("Inbound buffer full".to_string());
+ }
  let buffer = buffers.entry(tx_id.clone()).or_insert_with(Vec::new);
+ if buffer.len() >= MAX_FRAGMENTS_PER_TRANSACTION {
+     return Err(format!("Too many fragments for tx {}", tx_id));
+ }
```

---

[75] **4. Transaction Deduplication Disabled — Same Nonce Transaction Can Be Submitted Twice**

`src/ffi/transport.rs::HostBleTransport::push_received_transaction` (lines 791–818) · Confidence: 75

**Description**
The entire duplicate-transaction check (against both `submitted_tx_hashes` and the received queue) is commented out with "no duplicate check"; since nonce-based transactions are valid until the nonce advances, a race window exists between first submission and nonce advancement where the same transaction could be submitted a second time by a concurrent mesh delivery, potentially causing unexpected double-spend attempts or fee loss.

**Fix**

```diff
- // Duplicate check commented out - all reassembled transactions are queued
+ // Re-enable duplicate check
+ let submitted = self.submitted_tx_hashes.lock();
+ if submitted.contains_key(&tx_hash) {
+     return false;
+ }
+ drop(submitted);
+
+ let queue = self.received_tx_queue.lock();
+ for (_, queued_tx_bytes, _) in queue.iter() {
+     let mut h = Sha256::new();
+     h.update(queued_tx_bytes);
+     if h.finalize().to_vec() == tx_hash {
+         return false;
+     }
+ }
+ drop(queue);
```

---

[75] **5. `std::env::set_var` Called in Multithreaded JNI Context**

`src/ffi/android.rs::Java_xyz_pollinet_sdk_PolliNetFFI_init` (line 148) · Confidence: 75

**Description**
`std::env::set_var("POLLINET_QUEUE_STORAGE", ...)` is invoked inside the JNI init function without synchronisation; Rust's standard library documents that `set_var` is not thread-safe in multithreaded processes and can cause undefined behaviour (data races on the environment block) if called concurrently with any `env::var` read — a realistic scenario on Android where the JVM may call JNI from worker threads.

**Fix**

```diff
- std::env::set_var("POLLINET_QUEUE_STORAGE", &queue_storage_dir);
+ // Store the path in the transport instead of the global environment
+ transport.set_queue_storage_dir(queue_storage_dir);
```

---

[75] **6. Plaintext Bundle Files Silently Accepted — Encrypted Storage Bypassable**

`src/storage.rs::SecureStorage::load_bundle` (lines 158–172) · Confidence: 75

**Description**
When loading a bundle, if the `PNET` magic header is absent the file is treated as plain JSON and loaded without any error; an attacker with write access to the storage directory can replace the encrypted bundle file with an attacker-crafted plaintext file containing fraudulent nonce account data, redirecting offline transactions to an attacker-controlled nonce.

**Fix**

```diff
  } else {
-     // File is plain JSON (backward compatibility with old unencrypted files)
-     tracing::warn!("⚠️  Loading unencrypted bundle file (backward compatibility mode)");
-     String::from_utf8(encrypted_data)
-         .map_err(|e| StorageError::Io(format!("Failed to read bundle as UTF-8: {}", e)))?
+     return Err(StorageError::Decryption(
+         "Bundle file is not encrypted. Refusing to load for security. \
+          Delete the file and call prepareOfflineBundle to recreate it.".to_string()
+     ));
  }
```

---

[70] **7. Private Keypairs Serialized in FFI Request Structs**

`src/ffi/types.rs::PrepareOfflineBundleRequest` / `CreateOfflineTransactionRequest` · Confidence: 70

**Description**
Both structs carry `sender_keypair_base64` and `nonce_authority_keypair_base64` as plain JSON string fields, meaning raw private keys cross the Kotlin–Rust boundary as JSON blobs. If any log sink captures request payloads at DEBUG level (the default log level in `android.rs` is `LevelFilter::Debug`), private key material will appear in logcat output or crash reports accessible to other apps with READ_LOGS permission.

**Recommendation**
Pass keypairs as raw `JByteArray` parameters directly to FFI functions rather than embedding them in serializable request structs. At minimum, zero the keypair bytes immediately after use and ensure no `t_info!` / `t_debug!` statements log request JSON that may contain them.

---

[60] **8. SDK Shutdown Does Not Invalidate Transport Handle**

`src/ffi/android.rs::Java_xyz_pollinet_sdk_PolliNetFFI_shutdown` · Confidence: 60

**Description**
The shutdown function logs a message but does not remove or null the transport from the `TRANSPORTS` vector; the handle index remains fully usable for all SDK operations after shutdown is called, meaning there is no safe way for the host application to revoke SDK access or force re-initialisation.

**Recommendation**
Replace the `Vec<Arc<HostBleTransport>>` with a `Vec<Option<Arc<HostBleTransport>>>` and set the slot to `None` on shutdown. Update `get_transport` to return an error if the slot is `None`.

---

## Findings Summary

| # | Confidence | Title |
|---|---|---|
| 1 | [90] | Hardcoded Fallback AES-256-GCM Encryption Key |
| 2 | [85] | Nonce Account DataSize Filter Returns Wrong Size — Discovery Always Fails |
| 3 | [80] | Unbounded Inbound Fragment Buffer Enables Memory Exhaustion |
| 4 | [75] | Transaction Deduplication Disabled — Same Nonce Transaction Can Be Submitted Twice |
| 5 | [75] | `std::env::set_var` Called in Multithreaded JNI Context |
| 6 | [75] | Plaintext Bundle Files Silently Accepted — Encrypted Storage Bypassable |
| | | **Below Confidence Threshold** |
| 7 | [70] | Private Keypairs Serialized in FFI Request Structs |
| 8 | [60] | SDK Shutdown Does Not Invalidate Transport Handle |

---

> ⚠️ This review was performed by an AI assistant. AI analysis can never verify the complete absence of vulnerabilities and no guarantee of security is given. Team security reviews, bug bounty programs, and on-chain monitoring are strongly recommended.
