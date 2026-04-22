package xyz.pollinet.sdk

/**
 * Low-level JNI interface to the Rust PolliNet core.
 * 
 * This object loads the native library and provides direct access to FFI functions.
 * Most applications should use the higher-level [PolliNetSDK] class instead.
 */
object PolliNetFFI {
    init {
        System.loadLibrary("pollinet")
    }

    // =========================================================================
    // Initialization and lifecycle
    // =========================================================================

    /**
     * Initialize the PolliNet SDK with the given configuration.
     * @param configBytes JSON-encoded SdkConfig
     * @return Handle to the initialized SDK instance, or -1 on error
     */
    external fun init(configBytes: ByteArray): Long

    /**
     * Get the SDK version string
     */
    external fun version(): String

    /**
     * Shutdown the SDK and release resources
     */
    external fun shutdown(handle: Long)

    // =========================================================================
    // Host-driven transport API
    // =========================================================================

    /**
     * Push inbound data received from GATT characteristic
     * @return JSON FfiResult
     */
    external fun pushInbound(handle: Long, data: ByteArray): String

    /**
     * Get next outbound frame to send via GATT
     * @param maxLen Maximum frame size (MTU)
     * @return Frame bytes, or null if queue is empty
     */
    external fun nextOutbound(handle: Long, maxLen: Long): ByteArray?

    /**
     * Periodic tick for retry/timeout handling
     * @param nowMs Current timestamp in milliseconds
     * @return JSON FfiResult with array of frames to send
     */
    external fun tick(handle: Long, nowMs: Long): String

    /**
     * Get current transport metrics
     * @return JSON FfiResult with MetricsSnapshot
     */
    external fun metrics(handle: Long): String

    /**
     * Clear a transaction from reassembly buffers
     */
    external fun clearTransaction(handle: Long, txId: String): String

    // =========================================================================
    // Fragmentation API
    // =========================================================================

    /**
     * Fragment a transaction for BLE transmission
     * @param txBytes Transaction bytes to fragment
     * @param maxPayload Optional maximum payload size (MTU - 10). Pass 0 to use default
     * @return JSON FfiResult with FragmentList
     */
    external fun fragment(handle: Long, txBytes: ByteArray, maxPayload: Long = 0): String

    // =========================================================================
    // BLE Mesh Operations
    // =========================================================================

    /**
     * Reconstruct a transaction from fragments
     * @param fragmentsJson JSON array of FragmentData objects
     * @return JSON FfiResult with base64-encoded transaction
     */
    external fun reconstructTransaction(fragmentsJson: ByteArray): String
    
    /**
     * Get fragmentation statistics for a transaction
     * @param transactionBytes Transaction bytes to analyze
     * @return JSON FfiResult with FragmentationStats
     */
    external fun getFragmentationStats(transactionBytes: ByteArray): String
    
    /**
     * Prepare a transaction for broadcast over BLE mesh
     * @param handle SDK handle
     * @param transactionBytes Signed transaction bytes
     * @return JSON FfiResult with BroadcastPreparation
     */
    external fun prepareBroadcast(handle: Long, transactionBytes: ByteArray): String
    
    // =========================================================================
    // Autonomous Transaction Relay System
    // =========================================================================
    
    /**
     * Push a received transaction into the auto-submission queue
     * @param handle SDK handle
     * @param transactionBytes Received transaction bytes
     * @return JSON FfiResult with PushResponse (added: boolean, queueSize: int)
     */
    external fun pushReceivedTransaction(handle: Long, transactionBytes: ByteArray): String
    
    /**
     * Get next received transaction for auto-submission
     * @param handle SDK handle
     * @return JSON FfiResult with ReceivedTransaction (txId, transactionBase64, receivedAt) or null
     */
    external fun nextReceivedTransaction(handle: Long): String
    
    /**
     * Get count of transactions waiting for auto-submission
     * @param handle SDK handle
     * @return JSON FfiResult with QueueSizeResponse (queueSize: int)
     */
    external fun getReceivedQueueSize(handle: Long): String
    
    /**
     * Get fragment reassembly info for all incomplete transactions
     * @param handle SDK handle
     * @return JSON FfiResult with FragmentReassemblyInfoList
     */
    external fun getFragmentReassemblyInfo(handle: Long): String
    
    /**
     * Mark a transaction as successfully submitted (for deduplication)
     * @param handle SDK handle
     * @param transactionBytes Submitted transaction bytes
     * @return JSON FfiResult with success status
     */
    external fun markTransactionSubmitted(handle: Long, transactionBytes: ByteArray): String
    
    /**
     * Clean up old submitted transaction hashes (older than 24 hours)
     * @param handle SDK handle
     * @return JSON FfiResult with success status
     */
    external fun cleanupOldSubmissions(handle: Long): String
    
    
    /**
     * Debug outbound queue (non-destructive peek)
     * @param handle SDK handle
     * @return JSON FfiResult with queue debug info
     */
    external fun debugOutboundQueue(handle: Long): String
    
    // =========================================================================
    // Queue Management (Phase 2)
    // =========================================================================
    
    /**
     * Push transaction to outbound queue
     * @param handle SDK handle
     * @param requestJson JSON-encoded PushOutboundRequest
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun pushOutboundTransaction(handle: Long, requestJson: String): String
    
    /**
     * Accept and queue a pre-signed transaction from external partners
     * Verifies the transaction, compresses it if needed, fragments it, and adds to queue
     * @param handle SDK handle
     * @param requestJson JSON-encoded AcceptExternalTransactionRequest
     * @return JSON FfiResult<String> with transaction ID
     */
    external fun acceptAndQueueExternalTransaction(handle: Long, requestJson: String): String
    
    /**
     * Pop next transaction from outbound queue
     * @param handle SDK handle
     * @return JSON FfiResult<OutboundTransactionFFI?>
     */
    external fun popOutboundTransaction(handle: Long): String
    
    /**
     * Get outbound queue size
     * @param handle SDK handle
     * @return JSON FfiResult<QueueSizeResponse>
     */
    external fun getOutboundQueueSize(handle: Long): String
    
    /**
     * Add transaction to retry queue
     * @param handle SDK handle
     * @param requestJson JSON-encoded AddToRetryRequest
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun addToRetryQueue(handle: Long, requestJson: String): String
    
    /**
     * Pop next ready retry item
     * @param handle SDK handle
     * @return JSON FfiResult<RetryItemFFI?>
     */
    external fun popReadyRetry(handle: Long): String
    
    /**
     * Get retry queue size
     * @param handle SDK handle
     * @return JSON FfiResult<QueueSizeResponse>
     */
    external fun getRetryQueueSize(handle: Long): String
    
    /**
     * Queue confirmation for relay
     * @param handle SDK handle
     * @param requestJson JSON-encoded QueueConfirmationRequest
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun queueConfirmation(handle: Long, requestJson: String): String
    
    /**
     * Pop next confirmation from queue
     * @param handle SDK handle
     * @return JSON FfiResult<ConfirmationFFI?>
     */
    external fun popConfirmation(handle: Long): String
    
    /**
     * Get confirmation queue size
     * @param handle SDK handle
     * @return JSON FfiResult<QueueSizeResponse>
     */
    external fun getConfirmationQueueSize(handle: Long): String
    
    /**
     * Get metrics for all queues
     * @param handle SDK handle
     * @return JSON FfiResult<QueueMetricsFFI>
     */
    external fun getQueueMetrics(handle: Long): String
    
    /**
     * Cleanup stale fragments from reassembly buffer
     * @param handle SDK handle
     * @return JSON FfiResult with cleanup stats
     */
    external fun cleanupStaleFragments(handle: Long): String
    
    /**
     * Cleanup expired confirmations and retry items
     * @param handle SDK handle
     * @return JSON FfiResult with cleanup stats
     */
    external fun cleanupExpired(handle: Long): String

    /**
     * Purge outbound transactions older than [maxAgeSecs] seconds from all priority queues.
     * @return JSON FfiResult with { removed: Int }
     */
    external fun purgeStaleOutbound(handle: Long, maxAgeSecs: Long): String

    /**
     * Confirm delivery of [txId] to the current peer. Decrements relevance; returns
     * JSON FfiResult with { removed: Boolean } — true means evicted (relevance = 0).
     */
    external fun confirmDelivered(handle: Long, txId: String): String

    /**
     * Peek at the highest-relevance transaction in the outbound queue, load its
     * fragments into the transport BLE frame buffer, and return metadata.
     * Returns JSON FfiResult with { tx_id, relevance, fragment_count } or null.
     */
    external fun loadForSending(handle: Long): String

    // =========================================================================
    // Queue Persistence (Phase 5)
    // =========================================================================
    
    /**
     * Save all queues to disk (force save, bypass debouncing)
     * @param handle SDK handle
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun saveQueues(handle: Long): String
    
    /**
     * Auto-save queues if needed (with debouncing)
     * @param handle SDK handle
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun autoSaveQueues(handle: Long): String
    
    /**
     * Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
     * @param handle SDK handle
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun clearAllQueues(handle: Long): String
    
    /**
     * Relay a received confirmation (increment hop count and re-queue for relay)
     * @param handle SDK handle
     * @param confirmationJson JSON-encoded Confirmation
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun relayConfirmation(handle: Long, confirmationJson: String): String

    // =========================================================================
    // Peer / mesh health monitoring
    // =========================================================================

    /**
     * Return a full snapshot of all known peers and network health metrics.
     * @return JSON FfiResult<HealthSnapshotResponse>
     */
    external fun getHealthSnapshot(handle: Long): String

    /**
     * Record a heartbeat for a peer (marks it as alive / connected).
     * Call this whenever a BLE connection is established or a fragment is received.
     * @param peerId peer MAC address or unique identifier
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun recordPeerHeartbeat(handle: Long, peerId: String): String

    /**
     * Record a latency measurement for a peer (round-trip time in ms).
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun recordPeerLatency(handle: Long, peerId: String, latencyMs: Int): String

    /**
     * Record the RSSI (signal strength) reading for a peer.
     * Call this from onScanResult and after each GATT RSSI read.
     * @param rssi value in dBm (negative integer)
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun recordPeerRssi(handle: Long, peerId: String, rssi: Int): String

    // =========================================================================
    // Wallet address — reward attribution
    // =========================================================================

    /**
     * Set the wallet address for this node session.
     * Pass an empty string to clear a previously-set address.
     * @return JSON FfiResult<SuccessResponse>
     */
    external fun setWalletAddress(handle: Long, address: String): String

    /**
     * Get the wallet address currently set for this node session.
     * @return JSON FfiResult<WalletAddressResponse> — address field is empty if none set
     */
    external fun getWalletAddress(handle: Long): String

    // =========================================================================
    // Intent protocol — stateless helpers (no SDK handle needed)
    // =========================================================================

    /**
     * Returns the executor PDA address for the pollinet-executor Anchor program.
     * Stateless — no SDK handle required.
     * @return JSON FfiResult<ExecutorPdaResponse>
     */
    external fun getExecutorPda(): String

    /**
     * Builds a single unsigned transaction with one `approve_checked` instruction per
     * token entry. The owner_wallet must sign the returned transaction before submission.
     * @param requestJson JSON-encoded CreateApproveTransactionRequest
     * @return JSON FfiResult<ApproveTransactionResponse>
     */
    external fun createApproveTransaction(requestJson: ByteArray): String

    /**
     * Serializes an Intent into the canonical 169-byte borsh layout and returns it
     * as base64. Generates a random nonce unless nonce_hex is supplied.
     * @param requestJson JSON-encoded CreateIntentBytesRequest
     * @return JSON FfiResult<IntentBytesResponse>
     */
    external fun createIntentBytes(requestJson: ByteArray): String

    /**
     * Submit a signed intent to pollicore.
     * The pollicore URL is resolved from SdkConfig.pollicoreUrl or the POLLICORE_URL env var.
     * No JWT — authenticated by the Ed25519 wallet signature in the request body.
     * @param handle  SDK handle from [init]
     * @param requestJson JSON-encoded SubmitIntentRequest
     * @return JSON FfiResult<SubmitIntentResponse>
     */
    external fun submitIntent(handle: Long, requestJson: ByteArray): String

    /**
     * Builds a single unsigned transaction with one `revoke` instruction per token account,
     * clearing the executor PDA's delegate authority.
     * @param requestJson JSON-encoded CreateRevokeTransactionRequest
     * @return JSON FfiResult<RevokeTransactionResponse>
     */
    external fun createRevokeTransaction(requestJson: ByteArray): String

    /**
     * Returns the pollicore base URL baked in at compile time from POLLICORE_URL env var.
     * Returns an empty string if POLLICORE_URL was not set when the native library was built.
     */
    external fun getPolliCoreUrl(): String

    /**
     * Derives the Associated Token Account (ATA) address for the given owner wallet and mint.
     * Stateless — no SDK handle required. Returns the base58 ATA address, or empty string on error.
     */
    external fun deriveAssociatedTokenAccount(ownerBase58: String, mintBase58: String): String

    // =========================================================================
    // Subsystem 1 — Density-adaptive rotation
    // =========================================================================

    /**
     * Record a BLE scan observation. Call on every onScanResult with the device address.
     * Updates the sliding-window density estimator.
     * @return JSON FfiResult<Boolean>
     */
    external fun recordScanResult(handle: Long, peerId: String): String

    /**
     * Recompute and return adaptive session/cooldown parameters from current density N.
     * Call every 10 seconds. Returns JSON FfiResult<AdaptiveParams>.
     */
    external fun getAdaptiveParams(handle: Long): String

    /**
     * Add a peer to the cooldown list for [cooldownMs] milliseconds.
     * Call after every session ends (both mutual-drain and force-close paths).
     * @return JSON FfiResult<Boolean>
     */
    external fun addPeerToCooldown(handle: Long, peerId: String, cooldownMs: Long): String

    /**
     * Returns true if [peerId] is currently in the cooldown list.
     * @return JSON FfiResult<Boolean>
     */
    external fun isPeerInCooldown(handle: Long, peerId: String): String

    /**
     * Sparse-network safety net: expire the oldest cooldown entry early.
     * Call when idle duration > 2 × session_target_ms AND all known peers are in cooldown.
     * @return JSON FfiResult<String?> — the peer_id that was released, or null if empty.
     */
    external fun expireOldestCooldown(handle: Long): String

    /**
     * Log a session telemetry record.
     * @param telemetryJson JSON-encoded SessionTelemetry
     * @return JSON FfiResult<Boolean>
     */
    external fun logSessionTelemetry(handle: Long, telemetryJson: String): String

    // =========================================================================
    // Subsystem 2 — Per-peer materialized queue
    // =========================================================================

    /**
     * Get the list of tx_ids to send to [peerIdHex] (8-char hex = 4-byte compact ID).
     * Filters by deliveredTo exclusion, TTL, and relevance > 0.
     * Sorted: confirmations first, then priority desc, relevance desc, age asc.
     * @return JSON FfiResult<List<String>>
     */
    external fun outboundForPeer(handle: Long, peerIdHex: String): String

    /**
     * Drain-conditional delivery confirmation.
     * Call ONLY after mutual drain is achieved with [peerIdHex].
     * Adds peer to deliveredTo, decrements relevance. Evicts if relevance reaches 0.
     * @param txId transaction ID
     * @param peerIdHex 8-char hex compact peer ID
     * @return JSON FfiResult<{ removed: Boolean }>
     */
    external fun confirmDeliveredByPeer(handle: Long, txId: String, peerIdHex: String): String

    // =========================================================================
    // Subsystem 3 — Confirmation-driven purge
    // =========================================================================

    /**
     * Ingest a received or locally-generated Pollicore confirmation.
     * Verifies signature, purges matching carrier entry, creates tombstone,
     * and re-queues the confirmation at HIGH priority for further propagation.
     * Silently drops tampered confirmations.
     * @param confirmationBytes bincode-serialized MeshConfirmation
     * @return JSON FfiResult<{ purged: Boolean, added_to_carrier: Boolean }>
     */
    external fun ingestConfirmation(handle: Long, confirmationBytes: ByteArray): String

    /**
     * Returns true if [txIdHashHex] has an active tombstone.
     * Call before buffering inbound reassembly fragments for a transaction.
     * @return JSON FfiResult<{ tombstoned: Boolean }>
     */
    external fun isTombstoned(handle: Long, txIdHashHex: String): String

    /**
     * Periodic maintenance: evict expired tombstones and cooldowns.
     * Call from the 10-second adaptive params recomputation loop.
     * @return JSON FfiResult<Boolean>
     */
    external fun periodicMaintenance(handle: Long): String

    /**
     * Diagnostic: get the number of active tombstones.
     * @return JSON FfiResult<{ count: Int }>
     */
    external fun getTombstoneCount(handle: Long): String
}

