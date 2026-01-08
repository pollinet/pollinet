package xyz.pollinet.sdk

import kotlinx.coroutines.*
import kotlinx.serialization.SerialName
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
    suspend fun nextOutbound(maxLen: Int = 1024): ByteArray? = withContext(Dispatchers.IO) {
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
     * @param txBytes Transaction bytes to fragment
     * @param maxPayload Optional maximum payload size (typically MTU - 10). If null, uses default
     */
    suspend fun fragment(txBytes: ByteArray, maxPayload: Int? = null): Result<FragmentList> = withContext(Dispatchers.IO) {
        try {
            val maxPayloadLong = maxPayload?.toLong() ?: 0L
            val resultJson = PolliNetFFI.fragment(handle, txBytes, maxPayloadLong)
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
     * @param onSuccess Optional callback invoked with transaction signature after successful submission
     * @return Transaction signature if successful
     */
    suspend fun submitOfflineTransaction(
        transactionBase64: String,
        verifyNonce: Boolean = true,
        onSuccess: ((String) -> Unit)? = null
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = SubmitOfflineTransactionRequest(
                transactionBase64 = transactionBase64,
                verifyNonce = verifyNonce
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.submitOfflineTransaction(handle, requestJson)
            val result = parseResult<String>(resultJson)
            
            // Invoke callback on success
            result.onSuccess { signature ->
                onSuccess?.invoke(signature)
            }
            
            result
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Submit a nonce account creation transaction and automatically cache the nonce accounts.
     * 
     * This is a convenience method that:
     * 1. Submits the transaction to the blockchain
     * 2. Automatically caches all nonce accounts in the offline bundle after successful submission
     * 
     * Use this when submitting nonce account creation transactions created via
     * [createUnsignedNonceAccountsAndCache] to automatically cache them.
     * 
     * Note: Transactions can contain up to 5 nonce accounts (batched).
     * 
     * @param unsignedTransaction The unsigned nonce account transaction (may contain multiple nonce accounts)
     * @param finalSignedTransactionBase64 The fully signed transaction (after MWA + nonce signatures)
     * @return Transaction signature if successful, and all nonce accounts are automatically cached
     */
    suspend fun submitNonceAccountCreationAndCache(
        unsignedTransaction: UnsignedNonceTransaction,
        finalSignedTransactionBase64: String
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            // Submit the transaction
            val submitResult = submitOfflineTransaction(
                transactionBase64 = finalSignedTransactionBase64,
                verifyNonce = false  // Don't verify for creation transactions
            )
            
            // If successful, automatically cache all nonce accounts in this transaction
            submitResult.onSuccess { signature ->
                val cacheResult = cacheNonceAccounts(unsignedTransaction.noncePubkey)
                cacheResult.onFailure { cacheError ->
                    // Log but don't fail - submission was successful
                    android.util.Log.w("PolliNetSDK", "Failed to auto-cache ${unsignedTransaction.noncePubkey.size} nonce accounts after submission: ${cacheError.message}")
                }
            }
            
            submitResult
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
     * Create UNSIGNED offline SPL token transfer for MWA/Seed Vault signing.
     *
     * This variant:
     * - Takes only PUBLIC KEYS (no private keys)
     * - Uses cached nonce data from the offline bundle (no network required)
     * - Returns a base64-encoded unsigned SPL transaction that MWA will sign
     */
    suspend fun createUnsignedOfflineSplTransaction(
        senderWallet: String,
        recipientWallet: String,
        mintAddress: String,
        amount: Long,
        feePayer: String
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = CreateUnsignedOfflineSplTransactionRequest(
                senderWallet = senderWallet,
                recipientWallet = recipientWallet,
                mintAddress = mintAddress,
                amount = amount,
                feePayer = feePayer
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.createUnsignedOfflineSplTransaction(handle, requestJson)
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
     * Create an unsigned governance vote transaction (durable nonce, MWA-friendly).
     *
     * This method builds an unsigned vote transaction using a nonce account on-chain.
     * The returned base64-encoded transaction can be:
     * - Sent to MWA/Seed Vault for signing, or
     * - Signed manually with local keys.
     *
     * NOTE: This is an online operation (uses RPC to fetch nonce data).
     */
    suspend fun createUnsignedVote(
        voter: String,
        proposalId: String,
        voteAccount: String,
        choice: Int,
        feePayer: String,
        nonceAccount: String
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = CastUnsignedVoteRequest(
                voter = voter,
                proposalId = proposalId,
                voteAccount = voteAccount,
                choice = choice.toUByte(),
                feePayer = feePayer,
                nonceAccount = nonceAccount
            )
            val requestJson = json.encodeToString(request).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.castUnsignedVote(handle, requestJson)
            parseResult<String>(resultJson)
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
     * Convenience helper: create unsigned nonce account transactions.
     *
     * ⚠️ IMPORTANT: This method does NOT cache nonce accounts because the accounts
     * don't exist on-chain yet! You must:
     * 1. Sign the transactions with MWA (payer signature)
     * 2. Add nonce signature using [addNonceSignature]
     * 3. Submit transactions using [submitOfflineTransaction] to create accounts on-chain
     * 4. THEN call [cacheNonceAccounts] to cache the newly created accounts
     *
     * Workflow:
     * 1. Calls [createUnsignedNonceTransactions] to generate unsigned nonce account TXs.
     * 2. Returns the list of [UnsignedNonceTransaction] objects ready for MWA signing.
     *
     * This method is suitable for both MWA (co-sign with payer) and non-MWA flows.
     * 
     * @param count Number of nonce accounts to create (1-10 recommended)
     * @param payerPubkey Public key of the account that will pay for account creation
     * @param onCreated Optional callback invoked with nonce pubkeys after transactions are created.
     *                 Use this to automatically cache accounts after successful submission.
     * @return List of unsigned nonce account transactions ready for signing
     */
    suspend fun createUnsignedNonceAccountsAndCache(
        count: Int,
        payerPubkey: String,
        onCreated: ((List<String>) -> Unit)? = null
    ): Result<List<UnsignedNonceTransaction>> = withContext(Dispatchers.IO) {
        try {
            // Create unsigned transactions
            val result = createUnsignedNonceTransactions(count, payerPubkey)
            
            // Invoke callback with nonce pubkeys if provided (flatten all nonce pubkeys from all transactions)
            result.onSuccess { transactions ->
                val noncePubkeys = transactions.flatMap { it.noncePubkey }
                onCreated?.invoke(noncePubkeys)
            }
            
            result
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Cache nonce accounts after successful submission.
     * 
     * Helper method that takes the unsigned transactions and a list of successful signatures,
     * then automatically caches the corresponding nonce accounts.
     * 
     * Use this after submitting nonce account creation transactions to automatically
     * cache them in the offline bundle.
     * 
     * @param transactions The original list of unsigned nonce transactions
     * @param successfulSignatures List of transaction signatures that were successfully submitted.
     *                            Must match the order of transactions.
     * @return Result containing the number of accounts cached
     */
    suspend fun cacheNonceAccountsAfterSubmission(
        transactions: List<UnsignedNonceTransaction>,
        successfulSignatures: List<String>
    ): Result<Int> = withContext(Dispatchers.IO) {
        try {
            if (transactions.size != successfulSignatures.size) {
                return@withContext Result.failure(
                    IllegalArgumentException(
                        "Transaction count (${transactions.size}) doesn't match signature count (${successfulSignatures.size})"
                    )
                )
            }
            
            // Extract and flatten all nonce pubkeys from successfully submitted transactions
            val noncePubkeys = transactions.flatMap { it.noncePubkey }
            
            // Cache all nonce accounts
            cacheNonceAccounts(noncePubkeys)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Refresh all cached nonce data in the offline bundle.
     *
     * This quietly:
     * - Loads the existing OfflineTransactionBundle from secure storage
     * - For each cached nonce account, fetches the latest on-chain nonce state
     * - Updates blockhash / fee data and marks all nonces as available (used = false)
     *
     * Safe to call whenever internet connectivity is restored.
     *
     * @return Number of nonce entries refreshed (0 if none or no bundle)
     */
    /**
     * Refresh the blockhash in an unsigned transaction.
     * 
     * Use this right before sending an unsigned transaction to MWA for signing
     * to ensure the blockhash is fresh and won't expire during the signing process.
     * 
     * This is particularly useful for nonce account creation transactions, which
     * may take time to be signed by the user, causing the original blockhash to expire.
     * 
     * @param unsignedTxBase64 Base64-encoded unsigned transaction
     * @return Result containing the refreshed transaction (base64-encoded)
     */
    suspend fun refreshBlockhashInUnsignedTransaction(
        unsignedTxBase64: String
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.refreshBlockhashInUnsignedTransaction(
                handle,
                unsignedTxBase64
            )
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    suspend fun refreshOfflineBundle(): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.refreshOfflineBundle(handle)
            val response = parseResult<RefreshOfflineBundleResponse>(resultJson)
            response.map { it.refreshedCount }
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
        nonceKeypairBase64: List<String>  // Multiple keypairs for batched transactions
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
    // BLE Mesh Operations
    // =========================================================================
    
    /**
     * Fragment a signed transaction for BLE transmission
     * 
     * Splits a complete signed transaction into smaller fragments that
     * can be transmitted over BLE with MTU constraints.
     * 
     * @param transactionBytes Signed transaction bytes
     * @return List of fragments ready for BLE transmission
     */
    suspend fun fragmentTransaction(transactionBytes: ByteArray): Result<List<FragmentData>> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.fragmentTransaction(transactionBytes)
            parseResult<List<FragmentData>>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Reconstruct a transaction from BLE fragments
     * 
     * Takes a collection of fragments received over BLE and reconstructs
     * the original signed transaction. Fragments can be provided in any order.
     * 
     * @param fragments List of fragments to reconstruct
     * @return Base64-encoded reconstructed transaction
     */
    suspend fun reconstructTransaction(fragments: List<FragmentData>): Result<String> = withContext(Dispatchers.IO) {
        try {
            val fragmentsJson = json.encodeToString(fragments).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.reconstructTransaction(fragmentsJson)
            parseResult<String>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Get fragmentation statistics for a transaction
     * 
     * Analyzes a transaction and returns statistics about how it would
     * be fragmented, including efficiency metrics.
     * 
     * @param transactionBytes Transaction bytes to analyze
     * @return Fragmentation statistics
     */
    suspend fun getFragmentationStats(transactionBytes: ByteArray): Result<FragmentationStats> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getFragmentationStats(transactionBytes)
            parseResult<FragmentationStats>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Prepare a transaction for broadcast over BLE mesh
     * 
     * Fragments the transaction and wraps each fragment in a mesh packet.
     * Returns packets ready to send via BLE GATT to connected peers.
     * 
     * Each packet includes:
     * - Transaction ID (for tracking)
     * - Fragment index and total count
     * - Complete mesh packet bytes (base64-encoded) ready for BLE transmission
     * 
     * Usage:
     * ```kotlin
     * val prep = sdk.prepareBroadcast(signedTxBytes).getOrThrow()
     * for (packet in prep.fragmentPackets) {
     *     val bytes = Base64.decode(packet.packetBytes, Base64.NO_WRAP)
     *     bleService.sendToAllPeers(bytes)
     * }
     * ```
     * 
     * @param transactionBytes Signed Solana transaction
     * @return BroadcastPreparation with transaction ID and fragment packets
     */
    suspend fun prepareBroadcast(transactionBytes: ByteArray): Result<BroadcastPreparation> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.prepareBroadcast(handle, transactionBytes)
            parseResult<BroadcastPreparation>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Autonomous Transaction Relay System
    // =========================================================================

    /**
     * Push a received transaction into the auto-submission queue
     * Returns true if added, false if duplicate
     */
    suspend fun pushReceivedTransaction(transactionBytes: ByteArray): Result<PushResponse> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.pushReceivedTransaction(handle, transactionBytes)
            parseResult<PushResponse>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get next received transaction for auto-submission
     * Returns null if queue is empty
     */
    suspend fun nextReceivedTransaction(): Result<ReceivedTransaction?> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.nextReceivedTransaction(handle)
            parseResult<ReceivedTransaction?>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get count of transactions waiting for auto-submission
     */
    suspend fun getReceivedQueueSize(): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getReceivedQueueSize(handle)
            android.util.Log.d("PolliNetSDK", "🔍 getReceivedQueueSize() - Raw JSON: $resultJson")
            val response = parseResult<QueueSizeResponse>(resultJson)
            response.onFailure { error ->
                android.util.Log.e("PolliNetSDK", "❌ getReceivedQueueSize() - Parse error: ${error.message}")
            }
            response.map { 
                android.util.Log.d("PolliNetSDK", "✅ getReceivedQueueSize() - Parsed queueSize: ${it.queueSize}")
                it.queueSize 
            }
        } catch (e: Exception) {
            android.util.Log.e("PolliNetSDK", "💥 getReceivedQueueSize() - Exception: ${e.message}", e)
            Result.failure(e)
        }
    }

    /**
     * Get fragment reassembly info for all incomplete transactions
     */
    suspend fun getFragmentReassemblyInfo(): Result<FragmentReassemblyInfoList> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getFragmentReassemblyInfo(handle)
            parseResult<FragmentReassemblyInfoList>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Mark a transaction as successfully submitted (for deduplication)
     */
    suspend fun markTransactionSubmitted(transactionBytes: ByteArray): Result<Boolean> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.markTransactionSubmitted(handle, transactionBytes)
            parseResult<SuccessResponse>(resultJson).map { it.success }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Clean up old submitted transaction hashes (older than 24 hours)
     */
    suspend fun cleanupOldSubmissions(): Result<Boolean> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.cleanupOldSubmissions(handle)
            parseResult<SuccessResponse>(resultJson).map { it.success }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Debug outbound queue (non-destructive peek)
     */
    suspend fun debugOutboundQueue(): Result<OutboundQueueDebug> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.debugOutboundQueue(handle)
            parseResult<OutboundQueueDebug>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Queue Management (Phase 2)
    // =========================================================================
    
    /**
     * Push transaction to outbound queue with priority
     * @param txBytes Signed transaction bytes
     * @param txId Transaction ID (SHA-256 hash)
     * @param fragments List of fragments  
     * @param priority Transaction priority (HIGH, NORMAL, LOW)
     */
    suspend fun pushOutboundTransaction(
        txBytes: ByteArray,
        txId: String,
        fragments: List<FragmentFFI>,
        priority: Priority = Priority.NORMAL
    ): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val request = PushOutboundRequest(
                txBytes = android.util.Base64.encodeToString(txBytes, android.util.Base64.NO_WRAP),
                txId = txId,
                fragments = fragments,
                priority = priority
            )
            val requestJson = json.encodeToString(request)
            val resultJson = PolliNetFFI.pushOutboundTransaction(handle, requestJson)
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Pop next transaction from outbound queue (priority-based)
     * @return OutboundTransaction or null if queue empty
     */
    suspend fun popOutboundTransaction(): Result<OutboundTransaction?> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.popOutboundTransaction(handle)
            parseResult<OutboundTransaction?>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Get outbound queue size
     * @return Number of transactions in outbound queue
     */
    suspend fun getOutboundQueueSize(): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getOutboundQueueSize(handle)
            val response = parseResult<QueueSizeResponse>(resultJson)
            response.map { it.queueSize }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Add transaction to retry queue
     * @param txBytes Transaction bytes
     * @param txId Transaction ID
     * @param error Error message from failed submission
     */
    suspend fun addToRetryQueue(
        txBytes: ByteArray,
        txId: String,
        error: String
    ): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val request = AddToRetryRequest(
                txBytes = android.util.Base64.encodeToString(txBytes, android.util.Base64.NO_WRAP),
                txId = txId,
                error = error
            )
            val requestJson = json.encodeToString(request)
            val resultJson = PolliNetFFI.addToRetryQueue(handle, requestJson)
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Pop next ready retry item
     * @return RetryItem or null if no items ready
     */
    suspend fun popReadyRetry(): Result<RetryItem?> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.popReadyRetry(handle)
            parseResult<RetryItem?>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Get retry queue size
     * @return Number of items in retry queue
     */
    suspend fun getRetryQueueSize(): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getRetryQueueSize(handle)
            val response = parseResult<QueueSizeResponse>(resultJson)
            response.map { it.queueSize }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Queue confirmation for relay back to origin
     * @param txId Transaction ID (hex string)
     * @param signature Blockchain signature
     */
    suspend fun queueConfirmation(txId: String, signature: String): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val request = QueueConfirmationRequest(
                txId = txId,
                signature = signature
            )
            val requestJson = json.encodeToString(request)
            val resultJson = PolliNetFFI.queueConfirmation(handle, requestJson)
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Pop next confirmation from queue
     * @return Confirmation or null if queue empty
     */
    suspend fun popConfirmation(): Result<Confirmation?> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.popConfirmation(handle)
            parseResult<Confirmation?>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Get confirmation queue size
     * @return Number of confirmations in queue
     */
    suspend fun getConfirmationQueueSize(): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getConfirmationQueueSize(handle)
            val response = parseResult<QueueSizeResponse>(resultJson)
            response.map { it.queueSize }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Get metrics for all queues
     * @return QueueMetrics with sizes and statistics
     */
    suspend fun getQueueMetrics(): Result<QueueMetrics> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getQueueMetrics(handle)
            parseResult<QueueMetrics>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Cleanup stale fragments from reassembly buffer
     * @return Number of fragments cleaned
     */
    suspend fun cleanupStaleFragments(): Result<Int> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.cleanupStaleFragments(handle)
            
            @Serializable
            data class CleanupResponse(
                @SerialName("fragments_cleaned") val fragmentsCleaned: Int
            )
            
            val response = parseResult<CleanupResponse>(resultJson)
            response.map { it.fragmentsCleaned }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Cleanup expired confirmations and retry items
     * @return Pair of (confirmations cleaned, retries cleaned)
     */
    suspend fun cleanupExpired(): Result<Pair<Int, Int>> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.cleanupExpired(handle)
            
            @Serializable
            data class CleanupExpiredResponse(
                @SerialName("confirmations_cleaned") val confirmationsCleaned: Int,
                @SerialName("retries_cleaned") val retriesCleaned: Int
            )
            
            val response = parseResult<CleanupExpiredResponse>(resultJson)
            response.map { Pair(it.confirmationsCleaned, it.retriesCleaned) }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    // =========================================================================
    // Queue Persistence (Phase 5)
    // =========================================================================
    
    /**
     * Save all queues to disk (force save)
     * Call this before app shutdown or when user explicitly requests save
     */
    suspend fun saveQueues(): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.saveQueues(handle)
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Auto-save queues if needed (debounced - saves at most every 5 seconds)
     * Call this after queue operations to enable auto-persistence
     */
    suspend fun autoSaveQueues(): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.autoSaveQueues(handle)
            parseResult<SuccessResponse>(resultJson).map { }
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
                // Handle nullable data field (for Unit return types)
                @Suppress("UNCHECKED_CAST")
                val data = successResult.data ?: run {
                    // If T is nullable, allow null; otherwise return Unit as before
                    if (null is T) null as T else (Unit as T)
                }
                Result.success(data)
            } else {
                // Shouldn't happen, but handle gracefully
                android.util.Log.e("PolliNetSDK", "⚠️ parseResult() - Unexpected result format: ok=false")
                Result.failure(Exception("Unexpected result format"))
            }
        } catch (e: Exception) {
            // Try to parse as error
            try {
                val errorResult = this.json.decodeFromString<FfiResultError>(json)
                android.util.Log.e("PolliNetSDK", "❌ parseResult() - FFI error: [${errorResult.code}] ${errorResult.message}")
                Result.failure(PolliNetException(errorResult.code, errorResult.message))
            } catch (e2: Exception) {
                android.util.Log.e("PolliNetSDK", "💥 parseResult() - JSON parse failed: ${e.message}\nJSON: $json", e)
                Result.failure(Exception("Failed to parse FFI result: ${e.message}\nJSON input: $json"))
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
    val data: T?  // Nullable to handle Unit return types where Rust returns null
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
    @SerialName("fragment_type")
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

@Serializable
data class FragmentReassemblyInfo(
    @SerialName("transactionId") val transactionId: String,
    @SerialName("totalFragments") val totalFragments: Int,
    @SerialName("receivedFragments") val receivedFragments: Int,
    @SerialName("receivedIndices") val receivedIndices: List<Int>,
    @SerialName("fragmentSizes") val fragmentSizes: List<Int>,
    @SerialName("totalBytesReceived") val totalBytesReceived: Int
)

@Serializable
data class FragmentReassemblyInfoList(
    val transactions: List<FragmentReassemblyInfo>
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
data class CreateUnsignedOfflineSplTransactionRequest(
    val version: Int = 1,
    val senderWallet: String,
    val recipientWallet: String,
    val mintAddress: String,
    val amount: Long,
    val feePayer: String
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
    val nonceKeypairBase64: List<String>,  // Multiple keypairs for batched transactions
    val noncePubkey: List<String>  // Multiple pubkeys for batched transactions
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
data class RefreshOfflineBundleResponse(
    val refreshedCount: Int
)

@Serializable
data class CastUnsignedVoteRequest(
    val version: Int = 1,
    val voter: String,
    @SerialName("proposal_id") val proposalId: String,
    @SerialName("vote_account") val voteAccount: String,
    val choice: UByte,
    @SerialName("fee_payer") val feePayer: String,
    @SerialName("nonce_account") val nonceAccount: String
)

@Serializable
data class AddNonceSignatureRequest(
    val version: Int = 1,
    val payerSignedTransactionBase64: String,
    val nonceKeypairBase64: List<String>  // Multiple keypairs for batched transactions
)

// =============================================================================
// BLE Mesh Data Types
// =============================================================================

@Serializable
data class FragmentData(
    val transactionId: String,
    val fragmentIndex: Int,
    val totalFragments: Int,
    val dataBase64: String
)

@Serializable
data class FragmentationStats(
    val originalSize: Int,
    val fragmentCount: Int,
    val maxFragmentSize: Int,
    val avgFragmentSize: Int,
    val totalOverhead: Int,
    val efficiency: Float
)

@Serializable
data class FragmentPacket(
    val transactionId: String,
    val fragmentIndex: Int,
    val totalFragments: Int,
    val packetBytes: String  // Base64-encoded mesh packet
)

@Serializable
data class BroadcastPreparation(
    val transactionId: String,
    val fragmentPackets: List<FragmentPacket>
)

// =============================================================================
// Autonomous Transaction Relay Data Types
// =============================================================================

@Serializable
data class PushResponse(
    val added: Boolean,
    val queueSize: Int
)

@Serializable
data class ReceivedTransaction(
    val txId: String,
    val transactionBase64: String,
    val receivedAt: Long
)

@Serializable
data class QueueSizeResponse(
    val queueSize: Int
)

@Serializable
data class SuccessResponse(
    val success: Boolean
)

@Serializable
data class OutboundQueueDebug(
    @SerialName("total_fragments") val totalFragments: Int,
    val fragments: List<FragmentDebugInfo>
)

@Serializable
data class FragmentDebugInfo(
    val index: Int,
    val size: Int
)

// =============================================================================
// Queue Management Types (Phase 2)
// =============================================================================

/**
 * Transaction priority levels
 */
enum class Priority {
    HIGH,
    NORMAL,
    LOW
}

/**
 * Outbound transaction awaiting BLE transmission
 */
@Serializable
data class OutboundTransaction(
    @SerialName("txId") val txId: String,
    @SerialName("originalBytes") val originalBytes: String, // base64
    @SerialName("fragmentCount") val fragmentCount: Int,
    val priority: Priority,
    @SerialName("createdAt") val createdAt: Long,
    @SerialName("retryCount") val retryCount: Int
)

/**
 * Retry item with backoff scheduling
 */
@Serializable
data class RetryItem(
    @SerialName("txBytes") val txBytes: String, // base64
    @SerialName("txId") val txId: String,
    @SerialName("attemptCount") val attemptCount: Int,
    @SerialName("lastError") val lastError: String,
    @SerialName("nextRetryInSecs") val nextRetryInSecs: Long,
    @SerialName("ageSeconds") val ageSeconds: Long
)

/**
 * Confirmation status
 */
@Serializable
sealed class ConfirmationStatus {
    @Serializable
    @SerialName("SUCCESS")
    data class Success(val signature: String) : ConfirmationStatus()
    
    @Serializable
    @SerialName("FAILED")
    data class Failed(val error: String) : ConfirmationStatus()
}

/**
 * Transaction confirmation for relay
 */
@Serializable
data class Confirmation(
    @SerialName("txId") val txId: String, // hex
    val status: ConfirmationStatus,
    val timestamp: Long,
    @SerialName("relayCount") val relayCount: Int
)

/**
 * Queue metrics for all queues
 */
@Serializable
data class QueueMetrics(
    @SerialName("outboundSize") val outboundSize: Int,
    @SerialName("outboundHighPriority") val outboundHighPriority: Int,
    @SerialName("outboundNormalPriority") val outboundNormalPriority: Int,
    @SerialName("outboundLowPriority") val outboundLowPriority: Int,
    @SerialName("confirmationSize") val confirmationSize: Int,
    @SerialName("retrySize") val retrySize: Int,
    @SerialName("retryAvgAttempts") val retryAvgAttempts: Float
)

/**
 * Fragment for FFI (public for use in BleService)
 */
@Serializable
data class FragmentFFI(
    @SerialName("transactionId") val transactionId: String, // hex
    @SerialName("fragmentIndex") val fragmentIndex: Int,
    @SerialName("totalFragments") val totalFragments: Int,
    @SerialName("dataBase64") val dataBase64: String
)

/**
 * Internal request types for FFI
 */
@Serializable
internal data class PushOutboundRequest(
    val version: Int = 1,
    @SerialName("txBytes") val txBytes: String,
    @SerialName("txId") val txId: String,
    val fragments: List<FragmentFFI>,
    val priority: Priority
)

@Serializable
internal data class AddToRetryRequest(
    val version: Int = 1,
    @SerialName("txBytes") val txBytes: String,
    @SerialName("txId") val txId: String,
    val error: String
)

@Serializable
internal data class QueueConfirmationRequest(
    val version: Int = 1,
    @SerialName("txId") val txId: String,
    val signature: String
)

