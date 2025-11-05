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
     * @return JSON FfiResult with FragmentList
     */
    external fun fragment(handle: Long, txBytes: ByteArray): String
}

