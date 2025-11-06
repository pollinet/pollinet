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
    // Offline Bundle Management - Core PolliNet Features
    // =========================================================================

    /**
     * Prepare offline bundle for creating transactions without internet
     * This is a CORE PolliNet feature for offline/mesh transaction creation
     * 
     * Smart bundle management:
     * - Refreshes used nonces (FREE!)
     * - Only creates new nonce accounts if needed (~$0.20 each)
     * - Reuses existing nonce accounts to save money
     * 
     * @param count Number of nonces to prepare
     * @param senderKeypair Sender keypair as raw bytes (64 bytes)
     * @param bundleFile Optional file path to load/save bundle
     * @return OfflineTransactionBundle with available nonces
     */
    suspend fun prepareOfflineBundle(
        count: Int,
        senderKeypair: ByteArray,
        bundleFile: String? = null
    ): Result<OfflineTransactionBundle> = withContext(Dispatchers.IO) {
        try {
            val request = PrepareOfflineBundleRequest(
                count = count,
                senderKeypairBase64 = android.util.Base64.encodeToString(
                    senderKeypair,
                    android.util.Base64.NO_WRAP
                ),
                bundleFile = bundleFile
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.prepareOfflineBundle(handle, requestJson)
            
            // Parse the bundle JSON string from the result
            val bundleJsonResult = parseResult<String>(resultJson)
            bundleJsonResult.map { bundleJsonStr ->
                json.decodeFromString<OfflineTransactionBundle>(bundleJsonStr)
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Create transaction completely offline using cached nonce data
     * NO internet required - core PolliNet offline feature
     * 
     * @param senderKeypair Sender keypair as raw bytes (64 bytes)
     * @param nonceAuthorityKeypair Nonce authority keypair as raw bytes (64 bytes)
     * @param recipient Recipient public key
     * @param amount Amount in lamports
     * @param cachedNonce Cached nonce data from bundle
     * @return Base64-encoded compressed transaction ready for BLE
     */
    suspend fun createOfflineTransaction(
        senderKeypair: ByteArray,
        nonceAuthorityKeypair: ByteArray,
        recipient: String,
        amount: Long
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = CreateOfflineTransactionRequest(
                senderKeypairBase64 = android.util.Base64.encodeToString(
                    senderKeypair,
                    android.util.Base64.NO_WRAP
                ),
                nonceAuthorityKeypairBase64 = android.util.Base64.encodeToString(
                    nonceAuthorityKeypair,
                    android.util.Base64.NO_WRAP
                ),
                recipient = recipient,
                amount = amount
                // Nonce is automatically picked from stored bundle
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.createOfflineTransaction(handle, requestJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Submit offline-created transaction to blockchain
     * 
     * @param transactionBase64 Base64-encoded transaction from createOfflineTransaction
     * @param verifyNonce Whether to verify nonce is still valid before submission
     * @return Transaction signature if successful
     */
    suspend fun submitOfflineTransaction(
        transactionBase64: String,
        verifyNonce: Boolean = true
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = SubmitOfflineTransactionRequest(
                transactionBase64 = transactionBase64,
                verifyNonce = verifyNonce
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.submitOfflineTransaction(handle, requestJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // MWA (Mobile Wallet Adapter) Support - Unsigned Transaction Flow
    // =========================================================================

    /**
     * Create UNSIGNED offline transaction for MWA/Seed Vault signing
     * Takes PUBLIC KEYS only (no private keys) - compatible with Solana Mobile Stack
     * 
     * This allows secure transaction signing where private keys never leave
     * the Seed Vault hardware security module.
     * 
     * @param senderPubkey Sender's public key as base58 string
     * @param nonceAuthorityPubkey Nonce authority's public key as base58 string
     * @param recipient Recipient's public key as base58 string
     * @param amount Amount in lamports
     * @return Base64-encoded unsigned transaction ready for MWA signing
     */
    suspend fun createUnsignedOfflineTransaction(
        senderPubkey: String,
        nonceAuthorityPubkey: String,
        recipient: String,
        amount: Long
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = CreateUnsignedOfflineTransactionRequest(
                senderPubkey = senderPubkey,
                nonceAuthorityPubkey = nonceAuthorityPubkey,
                recipient = recipient,
                amount = amount
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.createUnsignedOfflineTransaction(handle, requestJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get transaction message bytes that need to be signed by MWA
     * 
     * This extracts the raw message from an unsigned transaction so that
     * MWA/Seed Vault can sign it securely.
     * 
     * @param unsignedTransactionBase64 Base64-encoded unsigned transaction
     * @return Base64-encoded message bytes to sign
     */
    suspend fun getTransactionMessageToSign(
        unsignedTransactionBase64: String
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = GetMessageToSignRequest(
                unsignedTransactionBase64 = unsignedTransactionBase64
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.getTransactionMessageToSign(handle, requestJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get list of public keys that need to sign this transaction
     * 
     * Returns the signers in the order required by Solana protocol.
     * This is useful for MWA authorization requests.
     * 
     * @param unsignedTransactionBase64 Base64-encoded unsigned transaction
     * @return List of public key strings (base58) that need to sign
     */
    suspend fun getRequiredSigners(
        unsignedTransactionBase64: String
    ): Result<List<String>> = withContext(Dispatchers.IO) {
        try {
            val request = GetRequiredSignersRequest(
                unsignedTransactionBase64 = unsignedTransactionBase64
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.getRequiredSigners(handle, requestJson)
            parseResult<List<String>>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Create unsigned nonce account creation transactions for MWA signing
     * 
     * This generates N unsigned transactions that create nonce accounts on-chain.
     * Each transaction must be co-signed by:
     * 1. The ephemeral nonce keypair (returned here, sign locally)
     * 2. The payer (sign with MWA)
     * 
     * @param count Number of nonce accounts to create
     * @param payerPubkey Public key of the account paying for nonce accounts (base58)
     * @return Result containing list of unsigned transactions with nonce keypairs
     */
    suspend fun createUnsignedNonceTransactions(
        count: Int,
        payerPubkey: String
    ): Result<List<UnsignedNonceTransaction>> = withContext(Dispatchers.IO) {
        try {
            val request = CreateUnsignedNonceTransactionsRequest(
                count = count,
                payerPubkey = payerPubkey
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.createUnsignedNonceTransactions(handle, requestJson)
            parseResult<List<UnsignedNonceTransaction>>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Cache nonce account data from on-chain accounts
     * 
     * This fetches nonce data from the blockchain and saves it to secure storage
     * for offline transaction creation. Call this after successfully creating
     * nonce accounts via MWA.
     * 
     * @param nonceAccounts List of nonce account public keys (base58)
     * @return Result containing the number of accounts cached
     */
    suspend fun cacheNonceAccounts(
        nonceAccounts: List<String>
    ): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val request = CacheNonceAccountsRequest(
                nonceAccounts = nonceAccounts
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.cacheNonceAccounts(handle, requestJson)
            val response = parseResult<CacheNonceAccountsResponse>(resultJson)
            response.map { it.cachedCount }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Add nonce signature to a payer-signed transaction
     * 
     * After MWA signs the transaction with the payer key (first signature),
     * this function adds the nonce keypair signature (second signature).
     * 
     * @param payerSignedTransactionBase64 Transaction with payer signature from MWA
     * @param nonceKeypairBase64 Nonce keypair to sign with
     * @return Fully-signed transaction ready for submission
     */
    suspend fun addNonceSignature(
        payerSignedTransactionBase64: String,
        nonceKeypairBase64: String
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = AddNonceSignatureRequest(
                payerSignedTransactionBase64 = payerSignedTransactionBase64,
                nonceKeypairBase64 = nonceKeypairBase64
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.addNonceSignature(handle, requestJson)
            parseResult<String>(resultJson)
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
    val logLevel: String? = "info",
    val storageDirectory: String? = null
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

// ============================================================================
// Offline Bundle Management - Core PolliNet Features
// ============================================================================

@Serializable
data class PrepareOfflineBundleRequest(
    val version: Int = 1,
    val count: Int,
    val senderKeypairBase64: String,
    val bundleFile: String? = null
)

@Serializable
data class CachedNonceData(
    val version: Int = 1,
    val nonceAccount: String,
    val authority: String,
    val blockhash: String,
    val lamportsPerSignature: Long,
    val cachedAt: Long,
    val used: Boolean
)

@Serializable
data class OfflineTransactionBundle(
    val version: Int = 1,
    val nonceCaches: List<CachedNonceData>,
    val maxTransactions: Int,
    val createdAt: Long
) {
    fun availableNonces(): Int = nonceCaches.count { !it.used }
    fun usedNonces(): Int = nonceCaches.count { it.used }
    fun totalNonces(): Int = nonceCaches.size
}

@Serializable
data class CreateOfflineTransactionRequest(
    val version: Int = 1,
    val senderKeypairBase64: String,
    val nonceAuthorityKeypairBase64: String,
    val recipient: String,
    val amount: Long
    // NOTE: Nonce is automatically picked from stored bundle - no need to send it
)

@Serializable
data class SubmitOfflineTransactionRequest(
    val version: Int = 1,
    val transactionBase64: String,
    val verifyNonce: Boolean = true
)

// ============================================================================
// MWA (Mobile Wallet Adapter) Support - Unsigned Transaction Flow
// ============================================================================

@Serializable
data class CreateUnsignedOfflineTransactionRequest(
    val version: Int = 1,
    val senderPubkey: String,
    val nonceAuthorityPubkey: String,
    val recipient: String,
    val amount: Long
)

@Serializable
data class GetMessageToSignRequest(
    val version: Int = 1,
    val unsignedTransactionBase64: String
)

@Serializable
data class GetRequiredSignersRequest(
    val version: Int = 1,
    val unsignedTransactionBase64: String
)

// ============================================================================
// Nonce Account Creation for MWA
// ============================================================================

@Serializable
data class CreateUnsignedNonceTransactionsRequest(
    val version: Int = 1,
    val count: Int,
    val payerPubkey: String
)

@Serializable
data class UnsignedNonceTransaction(
    val unsignedTransactionBase64: String,
    val nonceKeypairBase64: String,
    val noncePubkey: String
)

@Serializable
data class CacheNonceAccountsRequest(
    val version: Int = 1,
    val nonceAccounts: List<String>
)

@Serializable
data class CacheNonceAccountsResponse(
    val cachedCount: Int
)

@Serializable
data class AddNonceSignatureRequest(
    val version: Int = 1,
    val payerSignedTransactionBase64: String,
    val nonceKeypairBase64: String
)

