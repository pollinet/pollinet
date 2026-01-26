//
//  PolliNetFFI.h
//  pollinet-ios
//
//  C FFI header for PolliNet Rust library
//

#ifndef PolliNetFFI_h
#define PolliNetFFI_h

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Memory management
void pollinet_free_string(char *ptr);

// Core SDK functions
int64_t pollinet_init(const char *config_json);
const char *pollinet_version(void);
int pollinet_shutdown(int64_t handle);

// Transport API
int pollinet_push_inbound(int64_t handle, const uint8_t *data, size_t data_len);
int pollinet_next_outbound(int64_t handle, uint8_t *out_data, size_t *out_len);
int pollinet_tick(int64_t handle);
char *pollinet_metrics(int64_t handle);
int pollinet_clear_transaction(int64_t handle, const char *tx_id);

// Transaction building
char *pollinet_create_unsigned_transaction(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_create_unsigned_spl_transaction(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_cast_unsigned_vote(int64_t handle, const uint8_t *request_json, size_t request_len);

// Signature operations
int pollinet_prepare_sign_payload(const char *base64_tx, uint8_t *out_payload, size_t *out_len);
char *pollinet_apply_signature(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_verify_and_serialize(int64_t handle, const char *base64_tx);

// Fragmentation
char *pollinet_fragment(int64_t handle, const char *base64_tx);

// Offline bundle
char *pollinet_prepare_offline_bundle(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_create_offline_transaction(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_submit_offline_transaction(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_create_unsigned_offline_transaction(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_create_unsigned_offline_spl_transaction(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_get_transaction_message_to_sign(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_get_required_signers(int64_t handle, const uint8_t *request_json, size_t request_len);

// Nonce management
char *pollinet_create_unsigned_nonce_transactions(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_cache_nonce_accounts(int64_t handle, const uint8_t *request_json, size_t request_len);
char *pollinet_refresh_offline_bundle(int64_t handle);
char *pollinet_get_available_nonce(int64_t handle);
char *pollinet_add_nonce_signature(int64_t handle, const uint8_t *request_json, size_t request_len);

// Transaction refresh
char *pollinet_refresh_blockhash_in_unsigned_transaction(int64_t handle, const char *unsigned_tx_base64);

// BLE Mesh
char *pollinet_fragment_transaction(const uint8_t *transaction_bytes, size_t transaction_len);
char *pollinet_reconstruct_transaction(const uint8_t *fragments_json, size_t fragments_len);
char *pollinet_get_fragmentation_stats(const uint8_t *transaction_bytes, size_t transaction_len);
char *pollinet_prepare_broadcast(int64_t handle, const uint8_t *transaction_bytes, size_t transaction_len);

// Health monitoring
char *pollinet_get_health_snapshot(int64_t handle);
char *pollinet_record_peer_heartbeat(int64_t handle, const char *peer_id);
char *pollinet_record_peer_latency(int64_t handle, const char *peer_id, int latency_ms);
char *pollinet_record_peer_rssi(int64_t handle, const char *peer_id, int rssi);

// Received queue
char *pollinet_push_received_transaction(int64_t handle, const uint8_t *transaction_bytes, size_t transaction_len);
char *pollinet_next_received_transaction(int64_t handle);
char *pollinet_get_received_queue_size(int64_t handle);
char *pollinet_get_fragment_reassembly_info(int64_t handle);
char *pollinet_mark_transaction_submitted(int64_t handle, const uint8_t *transaction_bytes, size_t transaction_len);
char *pollinet_cleanup_old_submissions(int64_t handle);

// Queue management
char *pollinet_debug_outbound_queue(int64_t handle);
char *pollinet_save_queues(int64_t handle);
char *pollinet_auto_save_queues(int64_t handle);
char *pollinet_push_outbound_transaction(int64_t handle, const char *request_json);
char *pollinet_pop_outbound_transaction(int64_t handle);
char *pollinet_get_outbound_queue_size(int64_t handle);
char *pollinet_add_to_retry_queue(int64_t handle, const char *request_json);
char *pollinet_pop_ready_retry(int64_t handle);
char *pollinet_get_retry_queue_size(int64_t handle);
char *pollinet_cleanup_expired(int64_t handle);
char *pollinet_queue_confirmation(int64_t handle, const char *request_json);
char *pollinet_pop_confirmation(int64_t handle);
char *pollinet_cleanup_stale_fragments(int64_t handle);

#ifdef __cplusplus
}
#endif

#endif /* PolliNetFFI_h */
