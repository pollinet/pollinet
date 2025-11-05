package xyz.pollinet.sdk

import kotlinx.coroutines.*
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString

/**
 * High-level Kotlin SDK for PolliNet.
 * 
 * This class provides a convenient Kotlin API over the low-level JNI interface,
 * handling JSON serialization, error handling, and coroutine integration.
 */
class PolliNetSDK private constructor(
    private val handle: Long,
    private val json: Json = Json { 
        ignoreUnknownKeys = true
        prettyPrint = false
    }
) {
    companion object {
        /**
         * Initialize a new PolliNet SDK instance
         */
        suspend fun initialize(config: SdkConfig): Result<PolliNetSDK> = withContext(Dispatchers.IO) {
            try {
                val configJson = Json.encodeToString(config)
                val handle = PolliNetFFI.init(configJson.toByteArray())
                
                if (handle < 0) {
                    Result.failure(Exception("Failed to initialize SDK: invalid handle"))
                } else {
                    Result.success(PolliNetSDK(handle))
                }
            } catch (e: Exception) {
                Result.failure(e)
            }
        }
        
        /**
         * Get the SDK version
         */
        fun version(): String = PolliNetFFI.version()
    }

    /**
     * Shutdown and release resources
     */
    fun shutdown() {
        PolliNetFFI.shutdown(handle)
    }

    // =========================================================================
    // Transport API
    // =========================================================================

    /**
     * Push inbound data from GATT characteristic
     */
    suspend fun pushInbound(data: ByteArray): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.pushInbound(handle, data)
            parseResult<Unit>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get next outbound frame to send
     */
    suspend fun nextOutbound(maxLen: Int = 512): ByteArray? = withContext(Dispatchers.IO) {
        PolliNetFFI.nextOutbound(handle, maxLen.toLong())
    }

    /**
     * Periodic tick for protocol state machine
     */
    suspend fun tick(): Result<List<String>> = withContext(Dispatchers.IO) {
        try {
            val nowMs = System.currentTimeMillis()
            val resultJson = PolliNetFFI.tick(handle, nowMs)
            parseResult<List<String>>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get current transport metrics
     */
    suspend fun metrics(): Result<MetricsSnapshot> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.metrics(handle)
            parseResult<MetricsSnapshot>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Clear a transaction from buffers
     */
    suspend fun clearTransaction(txId: String): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.clearTransaction(handle, txId)
            parseResult<Unit>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Transaction builders
    // =========================================================================

    /**
     * Create an unsigned SOL transfer transaction
     */
    suspend fun createUnsignedTransaction(
        request: CreateUnsignedTransactionRequest
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val requestJson = json.encodeToString(request).toByteArray()
            val resultJson = PolliNetFFI.createUnsignedTransaction(handle, requestJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Create an unsigned SPL token transfer transaction
     */
    suspend fun createUnsignedSplTransaction(
        request: CreateUnsignedSplTransactionRequest
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val requestJson = json.encodeToString(request).toByteArray()
            val resultJson = PolliNetFFI.createUnsignedSplTransaction(handle, requestJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Signature helpers
    // =========================================================================

    /**
     * Prepare sign payload - Extract message bytes that need to be signed
     */
    suspend fun prepareSignPayload(base64Tx: String): ByteArray? = withContext(Dispatchers.IO) {
        PolliNetFFI.prepareSignPayload(handle, base64Tx)
    }

    /**
     * Apply signature to a transaction
     */
    suspend fun applySignature(
        base64Tx: String,
        signerPubkey: String,
        signatureBytes: ByteArray
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.applySignature(handle, base64Tx, signerPubkey, signatureBytes)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Verify and serialize transaction for submission/fragmentation
     */
    suspend fun verifyAndSerialize(base64Tx: String): Result<String> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.verifyAndSerialize(handle, base64Tx)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Fragmentation API
    // =========================================================================

    /**
     * Fragment a transaction for BLE transmission
     */
    suspend fun fragment(txBytes: ByteArray): Result<FragmentList> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.fragment(handle, txBytes)
            parseResult<FragmentList>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    private inline fun <reified T> parseResult(json: String): Result<T> {
        return try {
            // Try to parse as success first
            val successResult = this.json.decodeFromString<FfiResultSuccess<T>>(json)
            if (successResult.ok) {
                Result.success(successResult.data)
            } else {
                // Shouldn't happen, but handle gracefully
                Result.failure(Exception("Unexpected result format"))
            }
        } catch (e: Exception) {
            // Try to parse as error
            try {
                val errorResult = this.json.decodeFromString<FfiResultError>(json)
                Result.failure(PolliNetException(errorResult.code, errorResult.message))
            } catch (e2: Exception) {
                Result.failure(Exception("Failed to parse FFI result: ${e.message}"))
            }
        }
    }
}

/**
 * Exception thrown by PolliNet SDK operations
 */
class PolliNetException(
    val code: String,
    message: String
) : Exception("[$code] $message")

// =============================================================================
// Data types
// =============================================================================

@Serializable
data class SdkConfig(
    val version: Int = 1,
    val rpcUrl: String? = null,
    val enableLogging: Boolean = true,
    val logLevel: String? = "info"
)

@Serializable
private data class FfiResultSuccess<T>(
    val ok: Boolean,
    val data: T
)

@Serializable
private data class FfiResultError(
    val ok: Boolean,
    val code: String,
    val message: String
)

@Serializable
data class CreateUnsignedTransactionRequest(
    val version: Int = 1,
    val sender: String,
    val recipient: String,
    val feePayer: String,
    val amount: Long,
    val nonceAccount: String
)

@Serializable
data class CreateUnsignedSplTransactionRequest(
    val version: Int = 1,
    val senderWallet: String,
    val recipientWallet: String,
    val feePayer: String,
    val mintAddress: String,
    val amount: Long,
    val nonceAccount: String
)

@Serializable
data class Fragment(
    val id: String,
    val index: Int,
    val total: Int,
    val data: String, // base64
    val fragmentType: String,
    val checksum: String // base64
)

@Serializable
data class FragmentList(
    val fragments: List<Fragment>
)

@Serializable
data class MetricsSnapshot(
    val fragmentsBuffered: Int,
    val transactionsComplete: Int,
    val reassemblyFailures: Int,
    val lastError: String,
    val updatedAt: Long
)

