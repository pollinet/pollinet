#pragma once

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// =============================================================================
// Memory management
// Free every *char returned by any pollinet_ function.
// Free every *uint8_t returned by pollinet_next_outbound.
// =============================================================================

void pollinet_free_string(char *ptr);
void pollinet_free_bytes(uint8_t *ptr, size_t len);

// =============================================================================
// Lifecycle
// =============================================================================

/// Initialise the SDK. config_json is a null-terminated JSON SdkConfig.
/// Returns a non-negative handle on success, -1 on failure.
int64_t pollinet_init(const char *config_json);

/// Return the SDK version string. Caller must free.
char *pollinet_version(void);

/// Shut down the SDK instance and release all resources.
void pollinet_shutdown(int64_t handle);

// =============================================================================
// Transport
// =============================================================================

/// Push raw bytes received from a BLE characteristic into the Rust state machine.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_push_inbound(int64_t handle, const uint8_t *data, size_t len);

/// Get the next outbound BLE frame.
/// Writes byte count to *out_len. Returns NULL when the queue is empty.
/// Non-NULL return must be freed with pollinet_free_bytes.
uint8_t *pollinet_next_outbound(int64_t handle, size_t max_len, size_t *out_len);

/// Advance protocol timers (retry back-off, expiry).
/// Returns JSON FfiResult. Caller must free.
char *pollinet_tick(int64_t handle, int64_t now_ms);

/// Return current transport metrics as JSON FfiResult. Caller must free.
char *pollinet_metrics(int64_t handle);

// =============================================================================
// Received transaction queue
// =============================================================================

/// Pop the next reassembled transaction from the received queue.
/// Returns JSON FfiResult<ReceivedTransaction?>. Caller must free.
char *pollinet_next_received_transaction(int64_t handle);

/// Push a reassembled transaction into the received queue.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_push_received_transaction(int64_t handle, const uint8_t *tx_data, size_t tx_len);

// =============================================================================
// Submission
// =============================================================================

/// Submit a transaction to the Solana RPC.
/// request_json — JSON-encoded SubmitOfflineTransactionRequest.
/// Returns JSON FfiResult<String> (signature). Caller must free.
char *pollinet_submit_offline_transaction(int64_t handle, const char *request_json);

/// Mark a transaction as submitted (deduplication).
/// Returns JSON FfiResult. Caller must free.
char *pollinet_mark_transaction_submitted(int64_t handle, const uint8_t *tx_data, size_t tx_len);

// =============================================================================
// Retry queue
// =============================================================================

/// Add a transaction to the retry queue.
/// request_json — JSON-encoded AddToRetryRequest.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_add_to_retry_queue(int64_t handle, const char *request_json);

/// Pop the next retry item whose back-off has expired.
/// Returns JSON FfiResult<RetryItemFFI?>. Caller must free.
char *pollinet_pop_ready_retry(int64_t handle);

// =============================================================================
// Confirmation queue
// =============================================================================

/// Queue a success confirmation for relay.
/// request_json — JSON-encoded QueueConfirmationRequest.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_queue_confirmation(int64_t handle, const char *request_json);

/// Pop the next outbound confirmation.
/// Returns JSON FfiResult<ConfirmationFFI?>. Caller must free.
char *pollinet_pop_confirmation(int64_t handle);

/// Relay a received confirmation (increments hop count, re-queues).
/// confirmation_json — JSON-encoded Confirmation.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_relay_confirmation(int64_t handle, const char *confirmation_json);

// =============================================================================
// Fragmentation
// =============================================================================

/// Fragment a signed transaction for BLE transmission.
/// max_payload = 0 uses the SDK default.
/// Returns JSON FfiResult<FragmentList>. Caller must free.
char *pollinet_fragment_transaction(int64_t handle,
                                    const uint8_t *tx_data,
                                    size_t tx_len,
                                    size_t max_payload);

// =============================================================================
// Cleanup
// =============================================================================

/// Remove stale fragments from the reassembly buffer.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_cleanup_stale_fragments(int64_t handle);

/// Remove expired confirmations and retry items.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_cleanup_expired(int64_t handle);

// =============================================================================
// Queue metrics
// =============================================================================

/// Return queue size metrics as JSON FfiResult<QueueMetricsFFI>. Caller must free.
char *pollinet_get_queue_metrics(int64_t handle);

// =============================================================================
// Wallet address
// =============================================================================

/// Set the wallet address for reward attribution. Pass "" to clear.
/// Returns JSON FfiResult. Caller must free.
char *pollinet_set_wallet_address(int64_t handle, const char *address);

/// Return the wallet address for this session.
/// Returns JSON FfiResult<{address:String}>. Caller must free.
char *pollinet_get_wallet_address(int64_t handle);

#ifdef __cplusplus
} // extern "C"
#endif
