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
    // Transaction builders
    // =========================================================================

    /**
     * Create an unsigned SOL transfer transaction
     * @param requestJson JSON-encoded CreateUnsignedTransactionRequest
     * @return JSON FfiResult with base64-encoded transaction
     */
    external fun createUnsignedTransaction(handle: Long, requestJson: ByteArray): String

    /**
     * Create an unsigned SPL token transfer transaction
     * @param requestJson JSON-encoded CreateUnsignedSplTransactionRequest
     * @return JSON FfiResult with base64-encoded transaction
     */
    external fun createUnsignedSplTransaction(handle: Long, requestJson: ByteArray): String

    /**
     * Create unsigned governance vote transaction for MWA or manual signing.
     * Uses a nonce account on-chain (online: fetches nonce data via RPC).
     *
     * @param handle SDK handle
     * @param requestJson JSON-encoded CastUnsignedVoteRequest
     * @return JSON FfiResult with base64-encoded unsigned vote transaction
     */
    external fun castUnsignedVote(handle: Long, requestJson: ByteArray): String

    // =========================================================================
    // Signature helpers
    // =========================================================================

    /**
     * Prepare sign payload - Extract message bytes from transaction
     * @param base64Tx Base64-encoded unsigned transaction
     * @return Message bytes to sign, or null on error
     */
    external fun prepareSignPayload(handle: Long, base64Tx: String): ByteArray?

    /**
     * Apply signature to transaction
     * @param base64Tx Base64-encoded transaction
     * @param signerPubkey Signer's public key
     * @param signatureBytes Signature bytes (64 bytes)
     * @return JSON FfiResult with updated base64 transaction
     */
    external fun applySignature(
        handle: Long,
        base64Tx: String,
        signerPubkey: String,
        signatureBytes: ByteArray
    ): String

    /**
     * Verify and serialize transaction for submission/fragmentation
     * @param base64Tx Base64-encoded signed transaction
     * @return JSON FfiResult with wire-format base64 transaction
     */
    external fun verifyAndSerialize(handle: Long, base64Tx: String): String

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
    // Offline Bundle Management (Core PolliNet Features)
    // =========================================================================

    /**
     * Prepare offline bundle for creating transactions without internet
     * This is a CORE PolliNet feature for offline/mesh transaction creation
     * @param requestJson JSON-encoded PrepareOfflineBundleRequest
     * @return JSON FfiResult with OfflineTransactionBundle JSON string
     */
    external fun prepareOfflineBundle(handle: Long, requestJson: ByteArray): String

    /**
     * Create transaction completely offline using cached nonce data
     * NO internet required - core PolliNet offline feature
     * @param requestJson JSON-encoded CreateOfflineTransactionRequest
     * @return JSON FfiResult with base64-encoded compressed transaction
     */
    external fun createOfflineTransaction(handle: Long, requestJson: ByteArray): String

    /**
     * Submit offline-created transaction to blockchain
     * @param requestJson JSON-encoded SubmitOfflineTransactionRequest
     * @return JSON FfiResult with transaction signature
     */
    external fun submitOfflineTransaction(handle: Long, requestJson: ByteArray): String

    // =========================================================================
    // MWA (Mobile Wallet Adapter) Support - Unsigned Transaction Flow
    // =========================================================================

    /**
     * Create UNSIGNED offline transaction for MWA/Seed Vault signing
     * Takes PUBLIC KEYS only (no private keys) - compatible with Solana Mobile Stack
     * Returns unsigned transaction that MWA will sign securely
     * @param requestJson JSON-encoded CreateUnsignedOfflineTransactionRequest
     * @return JSON FfiResult with base64-encoded unsigned transaction
     */
    external fun createUnsignedOfflineTransaction(handle: Long, requestJson: ByteArray): String

    /**
     * Create UNSIGNED offline SPL token transfer for MWA/Seed Vault signing
     * Uses cached nonce data from the offline bundle (no network required).
     *
     * @param handle SDK handle
     * @param requestJson JSON-encoded CreateUnsignedOfflineSplTransactionRequest
     * @return JSON FfiResult with base64-encoded unsigned SPL transaction
     */
    external fun createUnsignedOfflineSplTransaction(handle: Long, requestJson: ByteArray): String

    /**
     * Get transaction message bytes that need to be signed by MWA
     * Extracts the raw message from unsigned transaction for secure signing
     * @param requestJson JSON-encoded GetMessageToSignRequest
     * @return JSON FfiResult with base64-encoded message bytes
     */
    external fun getTransactionMessageToSign(handle: Long, requestJson: ByteArray): String

    /**
     * Get list of public keys that need to sign this transaction
     * Returns signers in the order required by Solana protocol
     * @param requestJson JSON-encoded GetRequiredSignersRequest
     * @return JSON FfiResult with array of public key strings
     */
    external fun getRequiredSigners(handle: Long, requestJson: ByteArray): String

    /**
     * Create unsigned nonce account creation transactions for MWA signing
     * Generates N unsigned transactions that create nonce accounts on-chain
     * Each transaction includes an ephemeral nonce keypair that must be co-signed
     * 
     * Workflow:
     * 1. Call this to get unsigned transactions + nonce keypairs
     * 2. Sign transactions with nonce keypairs locally
     * 3. Send to MWA for payer co-signing
     * 4. Submit fully signed transactions
     * 5. Cache nonce data for offline transaction creation
     * 
     * @param requestJson JSON-encoded CreateUnsignedNonceTransactionsRequest
     * @return JSON FfiResult with array of UnsignedNonceTransaction
     */
    external fun createUnsignedNonceTransactions(handle: Long, requestJson: ByteArray): String

    /**
     * Cache nonce account data from on-chain accounts
     * Fetches nonce data from blockchain and saves to secure storage
     * Call this after successfully creating nonce accounts via MWA
     * 
     * @param requestJson JSON-encoded CacheNonceAccountsRequest
     * @return JSON FfiResult with cached count
     */
    external fun cacheNonceAccounts(handle: Long, requestJson: ByteArray): String
    
    /**
     * Refresh all cached nonce data in the offline bundle
     * 
     * Fetches latest on-chain nonce state for all cached nonce accounts and updates
     * the stored OfflineTransactionBundle in secure storage. Marks all nonces as
     * available (used = false) after refresh.
     * 
     * @param handle SDK handle
     * @return JSON FfiResult with { refreshedCount: Int }
     */
    external fun refreshOfflineBundle(handle: Long): String

    external fun getAvailableNonce(handle: Long): String
    
    external fun addNonceSignature(handle: Long, requestJson: ByteArray): String
    
    /**
     * Refresh the blockhash in an unsigned transaction.
     * 
     * Use this right before sending an unsigned transaction to MWA for signing
     * to ensure the blockhash is fresh and won't expire during the signing process.
     * 
     * @param handle SDK handle
     * @param unsignedTxBase64 Base64-encoded unsigned transaction
     * @return JSON FfiResult with refreshed transaction (base64-encoded)
     */
    external fun refreshBlockhashInUnsignedTransaction(handle: Long, unsignedTxBase64: String): String
    
    // =========================================================================
    // BLE Mesh Operations
    // =========================================================================
    
    /**
     * Fragment a signed transaction for BLE transmission
     * @param transactionBytes Signed transaction bytes
     * @return JSON FfiResult with array of FragmentData
     */
    external fun fragmentTransaction(transactionBytes: ByteArray): String
    
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
}

