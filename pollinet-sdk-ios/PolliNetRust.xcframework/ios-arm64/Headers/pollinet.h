/*
 * PolliNet iOS FFI — C ABI over the Rust core.
 * Contract: JSON FfiResult envelope ({"ok":true,"data":...} | {"ok":false,"code":...,"message":...}).
 * Every returned char* must be freed with pollinet_string_free();
 * the buffer from pollinet_next_outbound() with pollinet_bytes_free().
 */

#ifndef POLLINET_H
#define POLLINET_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#define POLLINET_IOS 1

/**
 * BLE MTU size for packet fragmentation
 */
#define BLE_MTU_SIZE 360

/**
 * Compression threshold in bytes
 */
#define COMPRESSION_THRESHOLD 100

/**
 * TTL for confirmation carrier entries (10 minutes > max tx TTL of 5 minutes).
 */
#define CONFIRMATION_TTL_SECS 600

/**
 * Recomputation interval (not enforced here; Kotlin calls every 10 s).
 */
#define RECOMPUTE_INTERVAL_MS 10000

/**
 * Upper bound on per-fragment data size when an MTU-aware payload is supplied.
 *
 * BLE negotiates MTUs up to ~517, so its effective `max_data` is always well under
 * 512 and this ceiling never binds for BLE (its output is byte-identical regardless
 * of this value). Larger-MTU transports such as Wi-Fi Direct (TCP inside the P2P
 * group) legitimately produce bigger fragments; this ceiling lets them do so while
 * still capping any single fragment to a sane size.
 */
#define MAX_FRAGMENT_PAYLOAD_CEILING 8192

/**
 * Maximum number of hops a message can traverse
 */
#define MAX_HOPS 10

/**
 * Default TTL for new messages
 */
#define DEFAULT_TTL 10

/**
 * Maximum fragments per transaction
 */
#define MAX_FRAGMENTS 100

/**
 * Maximum payload size per packet (bytes)
 * Target: BLE MTU ~517 bytes; with 48 bytes of header overhead this gives 469 bytes of data.
 * Using 516 to yield exactly 468 bytes of usable fragment data.
 */
#define MAX_PAYLOAD_SIZE 516

/**
 * Mesh packet header size (bytes)
 */
#define HEADER_SIZE 42

/**
 * Maximum usable fragment data size (bytes)
 * This is the actual transaction data that fits in a fragment
 * 516 - 42 - 6 = 468 bytes of transaction data per fragment
 */
#define MAX_FRAGMENT_DATA ((MAX_PAYLOAD_SIZE - HEADER_SIZE) - 6)

/**
 * Maximum incomplete transactions in buffer
 */
#define MAX_INCOMPLETE_TRANSACTIONS 50

/**
 * Timeout for incomplete transactions (seconds)
 */
#define REASSEMBLY_TIMEOUT 60

/**
 * Seen message cache size
 */
#define SEEN_CACHE_SIZE 1000

/**
 * Seen message TTL (seconds)
 */
#define SEEN_CACHE_TTL 600

#if (defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS))
/**
 * Version 1 of the FFI protocol
 */
#define FFI_VERSION 1
#endif

#if (defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS))
/**
 * Default per-fragment payload size for Wi-Fi Direct, in bytes.
 *
 * Chosen to sit comfortably below a typical 1500-byte Ethernet/TCP MTU after the
 * 4-byte length prefix and bincode container overhead, so frames never IP-fragment.
 * ~3× BLE's payload ⇒ ~3× fewer fragments per transaction. The shared fragmenter
 * clamps to `MAX_FRAGMENT_PAYLOAD_CEILING`, which is comfortably above this value.
 */
#define WIFI_DIRECT_MAX_PAYLOAD 1400
#endif

#if (defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS))
/**
 * Largest socket frame the platform driver should accept before treating the peer as
 * hostile/desynchronized (DoS guard for the length-prefixed framing). Informational —
 * enforced by the driver, exposed here so Rust and the platform agree on one number.
 */
#define WIFI_DIRECT_MAX_FRAME (16 * 1024)
#endif

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Initialize the PolliNet SDK.
 * Returns a handle (index) to the initialized transport instance, or -1 on error.
 */
jlong Java_xyz_pollinet_sdk_PolliNetFFI_init(JNIEnv env,
                                             JClass _class,
                                             JByteArray config_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Initialize a Wi-Fi Direct transport handle.
 *
 * Mirrors [`Java_xyz_pollinet_sdk_PolliNetFFI_init`] but creates a
 * `HostWifiDirectTransport` (same engine, larger default MTU). BLE-specific FFI
 * calls reject this handle by design.
 */
jlong Java_xyz_pollinet_sdk_PolliNetFFI_initWifiDirect(JNIEnv env,
                                                       JClass _class,
                                                       JByteArray config_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Initialize a Wi-Fi Direct handle that **shares the engine** of an existing BLE
 * handle, giving both radios one dedup set and queue. Returns -1 on invalid handle.
 */
jlong Java_xyz_pollinet_sdk_PolliNetFFI_initWifiDirectSharing(JNIEnv _env,
                                                              JClass _class,
                                                              jlong ble_handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Return the transport kind for a handle ("BLE" | "WIFI_DIRECT"), or "" if invalid.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_transportKind(JNIEnv env,
                                                        JClass _class,
                                                        jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get SDK version
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_version(JNIEnv env,
                                                  JClass _class);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Return the pollicore base URL baked in at compile time from POLLICORE_URL env var.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getPolliCoreUrl(JNIEnv env,
                                                          JClass _class);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Derive the Associated Token Account (ATA) address for a given owner wallet and
 * token mint. Stateless — returns the base58 ATA, or an empty string on bad input.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_deriveAssociatedTokenAccount(JNIEnv env,
                                                                       JClass _class,
                                                                       JString owner_j,
                                                                       JString mint_j);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Shutdown the SDK and release resources
 */
void Java_xyz_pollinet_sdk_PolliNetFFI_shutdown(JNIEnv _env,
                                                JClass _class,
                                                jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Push inbound data from GATT characteristic
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_pushInbound(JNIEnv env,
                                                      JClass _class,
                                                      jlong handle,
                                                      JByteArray data);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get next outbound frame to send
 */
jbyteArray Java_xyz_pollinet_sdk_PolliNetFFI_nextOutbound(JNIEnv env,
                                                          JClass _class,
                                                          jlong handle,
                                                          jlong max_len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Periodic tick for retry/timeout handling
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_tick(JNIEnv env,
                                               JClass _class,
                                               jlong handle,
                                               jlong now_ms);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get current metrics
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_metrics(JNIEnv env,
                                                  JClass _class,
                                                  jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Clear transaction from buffers
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_clearTransaction(JNIEnv env,
                                                           JClass _class,
                                                           jlong handle,
                                                           JString tx_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Remove all outbound queue fragments that belong to `tx_id`.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_clearOutboundTransaction(JNIEnv env,
                                                                   JClass _class,
                                                                   jlong handle,
                                                                   JString tx_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Fragment a transaction for BLE transmission.
 * Optionally accepts max_payload (MTU - 10) for MTU-aware fragmentation.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_fragment(JNIEnv env,
                                                   JClass _class,
                                                   jlong handle,
                                                   JByteArray tx_bytes,
                                                   jlong max_payload);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Reconstruct a transaction from fragments.
 * Takes JSON array of fragment objects with base64 data.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_reconstructTransaction(JNIEnv env,
                                                                 JClass _class,
                                                                 JByteArray fragments_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get fragmentation statistics for a transaction
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getFragmentationStats(JNIEnv env,
                                                                JClass _class,
                                                                JByteArray transaction_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Prepare a transaction broadcast (fragments it and returns fragments with packets)
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_prepareBroadcast(JNIEnv env,
                                                           JClass _class,
                                                           jlong _handle,
                                                           JByteArray transaction_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get mesh health snapshot
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getHealthSnapshot(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Record peer heartbeat
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_recordPeerHeartbeat(JNIEnv env,
                                                              JClass _class,
                                                              jlong handle,
                                                              JString peer_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Record peer latency measurement
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_recordPeerLatency(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle,
                                                            JString peer_id,
                                                            jint latency_ms);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Record peer RSSI (signal strength)
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_recordPeerRssi(JNIEnv env,
                                                         JClass _class,
                                                         jlong handle,
                                                         JString peer_id,
                                                         jint rssi);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Push a received transaction into the auto-submission queue
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_pushReceivedTransaction(JNIEnv env,
                                                                  JClass _class,
                                                                  jlong handle,
                                                                  JByteArray transaction_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get next received transaction for auto-submission
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_nextReceivedTransaction(JNIEnv env,
                                                                  JClass _class,
                                                                  jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get count of transactions waiting for auto-submission
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getReceivedQueueSize(JNIEnv env,
                                                               JClass _class,
                                                               jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get fragment reassembly info for all incomplete transactions
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getFragmentReassemblyInfo(JNIEnv env,
                                                                    JClass _class,
                                                                    jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Mark a transaction as successfully submitted
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_markTransactionSubmitted(JNIEnv env,
                                                                   JClass _class,
                                                                   jlong handle,
                                                                   JByteArray transaction_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Clean up old submitted transaction hashes
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_cleanupOldSubmissions(JNIEnv env,
                                                                JClass _class,
                                                                jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get outbound queue size (non-destructive peek for debugging)
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getOutboundQueueSize(JNIEnv env,
                                                               JClass _class,
                                                               jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get outbound queue debug info (non-destructive peek)
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_debugOutboundQueue(JNIEnv env,
                                                             JClass _class,
                                                             jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Save all queues to disk
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_saveQueues(JNIEnv env,
                                                     JClass _class,
                                                     jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Trigger auto-save if needed (debounced)
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_autoSaveQueues(JNIEnv env,
                                                         JClass _class,
                                                         jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Push transaction to outbound queue
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_pushOutboundTransaction(JNIEnv env,
                                                                  JClass _class,
                                                                  jlong handle,
                                                                  JString request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Accept and queue a pre-signed transaction from external partners
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_acceptAndQueueExternalTransaction(JNIEnv env,
                                                                            JClass _class,
                                                                            jlong handle,
                                                                            JString request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Pop next transaction from outbound queue
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_popOutboundTransaction(JNIEnv env,
                                                                 JClass _class,
                                                                 jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Add transaction to retry queue
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_addToRetryQueue(JNIEnv env,
                                                          JClass _class,
                                                          jlong handle,
                                                          JString request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Pop next ready retry item
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_popReadyRetry(JNIEnv env,
                                                        JClass _class,
                                                        jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get retry queue size
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getRetryQueueSize(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Cleanup expired confirmations and retry items
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_cleanupExpired(JNIEnv env,
                                                         JClass _class,
                                                         jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Confirm that all fragments for `tx_id` were delivered to the current peer.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_confirmDelivered(JNIEnv env,
                                                           JClass _class,
                                                           jlong handle,
                                                           JString tx_id_j);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Peek at the highest-relevance transaction and load its fragments for sending.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_loadForSending(JNIEnv env,
                                                         JClass _class,
                                                         jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Purge outbound transactions older than max_age_secs from all priority queues.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_purgeStaleOutbound(JNIEnv env,
                                                             JClass _class,
                                                             jlong handle,
                                                             jlong max_age_secs);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Queue a confirmation for relay back to origin device
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_queueConfirmation(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle,
                                                            JString request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Pop next confirmation from queue
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_popConfirmation(JNIEnv env,
                                                          JClass _class,
                                                          jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Cleanup stale fragments from the transaction cache
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_cleanupStaleFragments(JNIEnv env,
                                                                JClass _class,
                                                                jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Relay a received confirmation (increment hop count and re-queue for relay)
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_relayConfirmation(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle,
                                                            JString confirmation_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_clearAllQueues(JNIEnv env,
                                                         JClass _class,
                                                         jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Set the wallet address for this node session.
 * Pass an empty string to clear a previously-set address.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_setWalletAddress(JNIEnv env,
                                                           JClass _class,
                                                           jlong handle,
                                                           JString address);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get the wallet address currently set for this node session.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getWalletAddress(JNIEnv env,
                                                           JClass _class,
                                                           jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Returns the executor PDA address for the pollinet-executor Anchor program.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getExecutorPda(JNIEnv env,
                                                         JClass _class);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Builds a single unsigned transaction containing one `approve_checked` instruction
 * per entry in the request.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_createApproveTransaction(JNIEnv env,
                                                                   JClass _class,
                                                                   JByteArray request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Builds a single unsigned transaction with one `revoke` instruction per token account.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_createRevokeTransaction(JNIEnv env,
                                                                  JClass _class,
                                                                  JByteArray request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Serializes an Intent into the canonical 169-byte borsh layout (base64).
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_createIntentBytes(JNIEnv env,
                                                            JClass _class,
                                                            JByteArray request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Submit a signed intent to pollicore.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_submitIntent(JNIEnv env,
                                                       JClass _class,
                                                       jlong handle,
                                                       JByteArray request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Record a scan observation for density estimation.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_recordScanResult(JNIEnv env,
                                                           JClass _class,
                                                           jlong handle,
                                                           JString peer_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Recompute and return adaptive BLE session/cooldown parameters.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getAdaptiveParams(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Add `peer_id` to the cooldown list for `cooldown_ms` milliseconds.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_addPeerToCooldown(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle,
                                                            JString peer_id,
                                                            jlong cooldown_ms);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Returns true if `peer_id` is currently in cooldown.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_isPeerInCooldown(JNIEnv env,
                                                           JClass _class,
                                                           jlong handle,
                                                           JString peer_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Sparse-network safety net: expire the oldest cooldown entry early.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_expireOldestCooldown(JNIEnv env,
                                                               JClass _class,
                                                               jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Log a session telemetry record.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_logSessionTelemetry(JNIEnv env,
                                                              JClass _class,
                                                              jlong handle,
                                                              JString telemetry_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Returns the list of tx_ids that should be sent to `peer_id` (4-byte hex compact ID).
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_outboundForPeer(JNIEnv env,
                                                          JClass _class,
                                                          jlong handle,
                                                          JString peer_id_hex);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Drain-conditional delivery confirmation (Subsystem 2). Call ONLY on mutual drain.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_confirmDeliveredByPeer(JNIEnv env,
                                                                 JClass _class,
                                                                 jlong handle,
                                                                 JString tx_id,
                                                                 JString peer_id_hex);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Ingest a received (or locally generated) confirmation.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_ingestConfirmation(JNIEnv env,
                                                             JClass _class,
                                                             jlong handle,
                                                             JByteArray confirmation_bytes);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Check if a tx_id_hash (hex) has an active tombstone.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_isTombstoned(JNIEnv env,
                                                       JClass _class,
                                                       jlong handle,
                                                       JString tx_id_hash_hex);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Evict expired tombstones and expired cooldowns. Call in the periodic 10s tick.
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_periodicMaintenance(JNIEnv env,
                                                              JClass _class,
                                                              jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_CBINDGEN_ANDROID_JNI))
/**
 * Get the number of active tombstones (diagnostic only).
 */
jstring Java_xyz_pollinet_sdk_PolliNetFFI_getTombstoneCount(JNIEnv env,
                                                            JClass _class,
                                                            jlong handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Release a string returned by any `pollinet_*` function.
 *
 * # Safety
 * `ptr` must be a pointer previously returned by this library (or null).
 */
void pollinet_string_free(char *ptr);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Release a byte buffer returned by [`pollinet_next_outbound`].
 *
 * # Safety
 * `(ptr, len)` must be a pair previously returned by this library (or null).
 */
void pollinet_bytes_free(uint8_t *ptr, uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Initialize the PolliNet SDK with a BLE-backed engine.
 * `config_json` is the SdkConfig JSON. Returns a handle, or -1 on failure.
 *
 * # Safety
 * `config_json` must be a valid NUL-terminated string.
 */
int64_t pollinet_init(const char *config_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Initialize a standalone Wi-Fi transport handle (larger default MTU; on iOS this
 * backs the MultipeerConnectivity driver). Returns a handle, or -1 on failure.
 *
 * # Safety
 * `config_json` must be a valid NUL-terminated string.
 */
int64_t pollinet_init_wifi_direct(const char *config_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Initialize a Wi-Fi transport handle that shares the engine of an existing BLE
 * handle (one dedup set / queue across both radios). Returns -1 on failure.
 */
int64_t pollinet_init_wifi_direct_sharing(int64_t ble_handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Return the transport kind for a handle ("BLE" | "WIFI_DIRECT"), or "" if invalid.
 */
char *pollinet_transport_kind(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get SDK version.
 */
char *pollinet_version(void);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Return the pollicore base URL baked in at compile time (POLLICORE_URL), or "".
 */
char *pollinet_get_pollicore_url(void);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Derive the Associated Token Account (ATA) address for owner wallet + token mint.
 * Stateless. Returns the base58 ATA, or an empty string on invalid input.
 *
 * # Safety
 * `owner` and `mint` must be valid NUL-terminated strings.
 */
char *pollinet_derive_associated_token_account(const char *owner, const char *mint);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Shutdown the SDK handle and release its resources.
 */
void pollinet_shutdown(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Push inbound bytes received from the radio (GATT write/notify, MPC frame payload).
 *
 * # Safety
 * `data` must be valid for reads of `len` bytes.
 */
char *pollinet_push_inbound(int64_t handle, const uint8_t *data, uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get the next outbound frame to send, at most `max_len` bytes.
 * Returns a caller-owned buffer (release with [`pollinet_bytes_free`]) and writes
 * its length to `out_len`; returns null (out_len = 0) when the queue is empty.
 *
 * # Safety
 * `out_len` must be a valid pointer.
 */
uint8_t *pollinet_next_outbound(int64_t handle, uintptr_t max_len, uintptr_t *out_len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Periodic tick for retry/timeout handling. Returns frames as a JSON array of
 * base64 strings inside the envelope.
 */
char *pollinet_tick(int64_t handle, uint64_t now_ms);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get current metrics.
 */
char *pollinet_metrics(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Clear transaction from buffers.
 *
 * # Safety
 * `tx_id` must be a valid NUL-terminated string.
 */
char *pollinet_clear_transaction(int64_t handle, const char *tx_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Remove all outbound queue fragments that belong to `tx_id`.
 *
 * # Safety
 * `tx_id` must be a valid NUL-terminated string.
 */
char *pollinet_clear_outbound_transaction(int64_t handle, const char *tx_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Fragment a transaction for radio transmission. `max_payload` <= 0 means default.
 *
 * # Safety
 * `tx_bytes` must be valid for reads of `len` bytes.
 */
char *pollinet_fragment(int64_t handle,
                        const uint8_t *tx_bytes,
                        uintptr_t len,
                        int64_t max_payload);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Reconstruct a transaction from a JSON array of fragment objects with base64 data.
 *
 * # Safety
 * `fragments_json` must be a valid NUL-terminated string.
 */
char *pollinet_reconstruct_transaction(const char *fragments_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get fragmentation statistics for a transaction.
 *
 * # Safety
 * `tx_bytes` must be valid for reads of `len` bytes.
 */
char *pollinet_get_fragmentation_stats(const uint8_t *tx_bytes, uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Prepare a transaction broadcast (fragments + mesh packets). `handle` is unused
 * (kept for parity with the Android signature).
 *
 * # Safety
 * `tx_bytes` must be valid for reads of `len` bytes.
 */
char *pollinet_prepare_broadcast(int64_t _handle, const uint8_t *tx_bytes, uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get mesh health snapshot.
 */
char *pollinet_get_health_snapshot(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Record peer heartbeat.
 *
 * # Safety
 * `peer_id` must be a valid NUL-terminated string.
 */
char *pollinet_record_peer_heartbeat(int64_t handle, const char *peer_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Record peer latency measurement (milliseconds).
 *
 * # Safety
 * `peer_id` must be a valid NUL-terminated string.
 */
char *pollinet_record_peer_latency(int64_t handle, const char *peer_id, uint32_t latency_ms);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Record peer RSSI (dBm, typically negative).
 *
 * # Safety
 * `peer_id` must be a valid NUL-terminated string.
 */
char *pollinet_record_peer_rssi(int64_t handle, const char *peer_id, int32_t rssi);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Push a received transaction into the auto-submission queue.
 *
 * # Safety
 * `tx_bytes` must be valid for reads of `len` bytes.
 */
char *pollinet_push_received_transaction(int64_t handle, const uint8_t *tx_bytes, uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get next received transaction for auto-submission (data = null when empty).
 */
char *pollinet_next_received_transaction(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get count of transactions waiting for auto-submission.
 */
char *pollinet_get_received_queue_size(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get fragment reassembly info for all incomplete transactions.
 */
char *pollinet_get_fragment_reassembly_info(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Mark a transaction as successfully submitted (dedup).
 *
 * # Safety
 * `tx_bytes` must be valid for reads of `len` bytes.
 */
char *pollinet_mark_transaction_submitted(int64_t handle, const uint8_t *tx_bytes, uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Clean up old submitted transaction hashes.
 */
char *pollinet_cleanup_old_submissions(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get outbound queue size (non-destructive peek for debugging).
 */
char *pollinet_get_outbound_queue_size(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get outbound queue debug info (non-destructive peek).
 */
char *pollinet_debug_outbound_queue(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Save all queues to disk.
 */
char *pollinet_save_queues(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Trigger auto-save if needed (debounced).
 */
char *pollinet_auto_save_queues(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Push transaction to outbound queue (PushOutboundRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_push_outbound_transaction(int64_t handle, const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Accept and queue a pre-signed external transaction
 * (AcceptExternalTransactionRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_accept_and_queue_external_transaction(int64_t handle, const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Pop next transaction from outbound queue (data = null when empty).
 */
char *pollinet_pop_outbound_transaction(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Add transaction to retry queue (AddToRetryRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_add_to_retry_queue(int64_t handle, const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Pop next ready retry item (data = null when none ready).
 */
char *pollinet_pop_ready_retry(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get retry queue size.
 */
char *pollinet_get_retry_queue_size(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Cleanup expired confirmations and retry items.
 */
char *pollinet_cleanup_expired(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Confirm delivery of all fragments for `tx_id` to the current peer.
 *
 * # Safety
 * `tx_id` must be a valid NUL-terminated string.
 */
char *pollinet_confirm_delivered(int64_t handle, const char *tx_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Load the highest-relevance outbound transaction's fragments for sending.
 */
char *pollinet_load_for_sending(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Purge outbound transactions older than `max_age_secs`.
 */
char *pollinet_purge_stale_outbound(int64_t handle, int64_t max_age_secs);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Queue a confirmation for relay back to origin (QueueConfirmationRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_queue_confirmation(int64_t handle, const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Pop next confirmation from queue (data = null when empty).
 */
char *pollinet_pop_confirmation(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Cleanup stale fragments from the transaction cache.
 */
char *pollinet_cleanup_stale_fragments(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Relay a received confirmation (ConfirmationFFI JSON; increments hop count).
 *
 * # Safety
 * `confirmation_json` must be a valid NUL-terminated string.
 */
char *pollinet_relay_confirmation(int64_t handle, const char *confirmation_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Clear all queues (outbound, retry, confirmation, received) and reassembly buffers.
 */
char *pollinet_clear_all_queues(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Set the wallet address for this node session. Empty string clears it.
 *
 * # Safety
 * `address` must be a valid NUL-terminated string.
 */
char *pollinet_set_wallet_address(int64_t handle, const char *address);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get the wallet address currently set for this node session.
 */
char *pollinet_get_wallet_address(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Returns the executor PDA address for the pollinet-executor Anchor program.
 */
char *pollinet_get_executor_pda(void);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Build a batch `approve_checked` transaction (CreateApproveTransactionRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_create_approve_transaction(const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Build a batch `revoke` transaction (CreateRevokeTransactionRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_create_revoke_transaction(const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Serialize an Intent into the canonical 169-byte borsh layout
 * (CreateIntentBytesRequest JSON → base64 bytes + nonce).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_create_intent_bytes(const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Submit a signed intent to pollicore (SubmitIntentRequest JSON).
 *
 * # Safety
 * `request_json` must be a valid NUL-terminated string.
 */
char *pollinet_submit_intent(int64_t handle, const char *request_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Record a scan observation for density estimation.
 *
 * # Safety
 * `peer_id` must be a valid NUL-terminated string.
 */
char *pollinet_record_scan_result(int64_t handle, const char *peer_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Recompute and return adaptive session/cooldown parameters.
 */
char *pollinet_get_adaptive_params(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Add `peer_id` to the cooldown list for `cooldown_ms` milliseconds.
 *
 * # Safety
 * `peer_id` must be a valid NUL-terminated string.
 */
char *pollinet_add_peer_to_cooldown(int64_t handle, const char *peer_id, uint64_t cooldown_ms);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Returns true if `peer_id` is currently in cooldown.
 *
 * # Safety
 * `peer_id` must be a valid NUL-terminated string.
 */
char *pollinet_is_peer_in_cooldown(int64_t handle, const char *peer_id);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Sparse-network safety net: expire the oldest cooldown entry early.
 */
char *pollinet_expire_oldest_cooldown(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Log a session telemetry record (SessionTelemetry JSON).
 *
 * # Safety
 * `telemetry_json` must be a valid NUL-terminated string.
 */
char *pollinet_log_session_telemetry(int64_t handle, const char *telemetry_json);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Returns the tx_ids to send to `peer_id_hex` (4-byte compact ID, 8 hex chars).
 *
 * # Safety
 * `peer_id_hex` must be a valid NUL-terminated string.
 */
char *pollinet_outbound_for_peer(int64_t handle, const char *peer_id_hex);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Drain-conditional delivery confirmation (Subsystem 2). Call ONLY on mutual drain.
 *
 * # Safety
 * `tx_id` and `peer_id_hex` must be valid NUL-terminated strings.
 */
char *pollinet_confirm_delivered_by_peer(int64_t handle,
                                         const char *tx_id,
                                         const char *peer_id_hex);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Ingest a received (or locally generated) MeshConfirmation frame.
 *
 * # Safety
 * `confirmation_bytes` must be valid for reads of `len` bytes.
 */
char *pollinet_ingest_confirmation(int64_t handle,
                                   const uint8_t *confirmation_bytes,
                                   uintptr_t len);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Check if a tx_id_hash (hex) has an active tombstone.
 *
 * # Safety
 * `tx_id_hash_hex` must be a valid NUL-terminated string.
 */
char *pollinet_is_tombstoned(int64_t handle, const char *tx_id_hash_hex);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Evict expired tombstones and expired cooldowns. Call in the periodic 10s tick.
 */
char *pollinet_periodic_maintenance(int64_t handle);
#endif

#if ((defined(POLLINET_CBINDGEN_ANDROID_JNI) || defined(POLLINET_IOS)) && defined(POLLINET_IOS))
/**
 * Get the number of active tombstones (diagnostic only).
 */
char *pollinet_get_tombstone_count(int64_t handle);
#endif

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* POLLINET_H */
