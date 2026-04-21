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
        encodeDefaults = true
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
    // BLE Mesh Operations
    // =========================================================================

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
     * Accept and queue a pre-signed transaction from external partners
     * 
     * This method is designed for accepting transactions from external partners.
     * It verifies the transaction is properly signed, compresses it if needed,
     * fragments it for BLE transmission, and adds it to the outbound queue for relay.
     * 
     * The transaction will be queued with NORMAL priority (external partner transactions).
     * 
     * @param base64SignedTx Base64-encoded pre-signed Solana transaction
     * @param maxPayload Optional maximum payload size (typically MTU - 10). If null, uses default.
     * @return Transaction ID (SHA-256 hash as hex string) for tracking
     */
    suspend fun acceptAndQueueExternalTransaction(
        base64SignedTx: String,
        maxPayload: Int? = null
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val request = AcceptExternalTransactionRequest(
                base64SignedTx = base64SignedTx,
                maxPayload = maxPayload
            )
            val requestJson = json.encodeToString(request)
            val resultJson = PolliNetFFI.acceptAndQueueExternalTransaction(handle, requestJson)
            parseResult<String>(resultJson)
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
    suspend fun getOutboundQueueSize(): Result<Int> = getQueueSize(PolliNetFFI::getOutboundQueueSize)
    
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
    suspend fun getRetryQueueSize(): Result<Int> = getQueueSize(PolliNetFFI::getRetryQueueSize)
    
    /**
     * Queue a SUCCESS confirmation for relay back to origin.
     * @param txId Transaction ID (hex string, SHA-256 of tx bytes)
     * @param signature On-chain transaction signature
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
     * Queue a FAILURE confirmation for relay back to origin.
     * Call this when a transaction is permanently dropped (stale nonce, max retries
     * exhausted, or any non-retryable error) so the originating node learns about it.
     * @param txId Transaction ID (hex string, SHA-256 of tx bytes)
     * @param error Human-readable reason for failure
     */
    suspend fun queueFailureConfirmation(txId: String, error: String): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val confirmation = Confirmation(
                txId = txId,
                status = ConfirmationStatus.Failed(error),
                timestamp = System.currentTimeMillis(),
                relayCount = 0
            )
            // relayConfirmation serialises to JSON and pushes into the Rust confirmation queue
            relayConfirmation(confirmation)
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
    suspend fun getConfirmationQueueSize(): Result<Int> = getQueueSize(PolliNetFFI::getConfirmationQueueSize)
    
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
    
    /**
     * Clear all queues (outbound, retry, confirmation, received) and reassembly buffers
     */
    suspend fun clearAllQueues(): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.clearAllQueues(handle)
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
    
    /**
     * Relay a received confirmation (increment hop count and re-queue for relay)
     * @param confirmation Confirmation to relay
     * @return Success if relayed, failure if max hops exceeded
     */
    suspend fun relayConfirmation(confirmation: Confirmation): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val confirmationJson = json.encodeToString(confirmation)
            val resultJson = PolliNetFFI.relayConfirmation(handle, confirmationJson)
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Peer / mesh health monitoring
    // =========================================================================

    /**
     * Get a full snapshot of all known peers and aggregate network health metrics.
     * Peers are populated as [recordPeerHeartbeat] / [recordPeerRssi] are called.
     */
    suspend fun getHealthSnapshot(): Result<HealthSnapshot> = withContext(Dispatchers.IO) {
        try {
            @Serializable data class HealthSnapshotWrapper(val snapshot: HealthSnapshot)
            parseResult<HealthSnapshotWrapper>(PolliNetFFI.getHealthSnapshot(handle)).map { it.snapshot }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Record a heartbeat for a BLE peer (marks it as Connected in the health monitor).
     * Call on every successful GATT connection.
     */
    suspend fun recordPeerHeartbeat(peerId: String): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            parseResult<SuccessResponse>(PolliNetFFI.recordPeerHeartbeat(handle, peerId)).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Record an RSSI reading for a peer.  Call from [ScanCallback.onScanResult] and
     * [BluetoothGattCallback.onReadRemoteRssi].
     */
    suspend fun recordPeerRssi(peerId: String, rssi: Int): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            parseResult<SuccessResponse>(PolliNetFFI.recordPeerRssi(handle, peerId, rssi)).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Record a round-trip latency measurement for a peer (in milliseconds).
     */
    suspend fun recordPeerLatency(peerId: String, latencyMs: Int): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            parseResult<SuccessResponse>(PolliNetFFI.recordPeerLatency(handle, peerId, latencyMs)).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Wallet address — reward attribution
    // =========================================================================

    /**
     * Update the wallet address at runtime.
     * Pass null or an empty string to clear a previously-set address.
     */
    suspend fun setWalletAddress(address: String?): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.setWalletAddress(handle, address ?: "")
            parseResult<SuccessResponse>(resultJson).map { }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get the wallet address currently stored on the Rust transport.
     * Returns null if no address has been set.
     */
    suspend fun getWalletAddress(): Result<String?> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getWalletAddress(handle)
            parseResult<WalletAddressResponse>(resultJson).map { r ->
                r.address.takeIf { it.isNotEmpty() }
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // =========================================================================
    // Intent protocol
    // =========================================================================

    /**
     * Fetches the gateway (pollicore) wallet public key.
     * Use [deriveAssociatedTokenAccount] on this address + the token mint to get the
     * correct [gasFeepayee] for [createIntentBytes].
     */
    suspend fun getGatewayWallet(): Result<String> = withContext(Dispatchers.IO) {
        try {
            val url = pollicoreUrl() + "/sdk/intents/gateway"
            val body = polliCoreGet(url)
            val resp = json.decodeFromString<GatewayResponse>(body)
            Result.success(resp.wallet)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Derives the Associated Token Account (ATA) address for a given wallet and mint.
     * This is the account that must be used as the `to` field in [createIntentBytes]
     * — the recipient's wallet address is NOT valid there; their token account is required.
     *
     * Stateless and offline-capable; the derivation is deterministic.
     *
     * @param ownerWallet Base58 wallet address of the recipient.
     * @param tokenMint   Base58 mint address of the token.
     * @return Base58 ATA address.
     */
    suspend fun deriveAssociatedTokenAccount(
        ownerWallet: String,
        tokenMint: String,
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val ata = PolliNetFFI.deriveAssociatedTokenAccount(ownerWallet, tokenMint)
            if (ata.isEmpty()) Result.failure(Exception("ATA derivation returned empty — check owner/mint are valid base58"))
            else Result.success(ata)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Returns the executor PDA address for the pollinet-executor Anchor program.
     * Store this address — users must call `approve` on each token account they want
     * to delegate before submitting intents.
     */
    suspend fun getExecutorPda(): Result<ExecutorPdaResponse> = withContext(Dispatchers.IO) {
        try {
            val resultJson = PolliNetFFI.getExecutorPda()
            parseResult<ExecutorPdaResponse>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Builds a single unsigned transaction that grants the executor PDA delegate
     * authority over each token account in [tokens].
     *
     * The caller must sign and submit this transaction (via MWA or KeystoreManager)
     * **before** creating any intents for those tokens.
     *
     * @param ownerWallet  Base58 public key of the wallet that owns the token accounts.
     * @param tokens       List of token accounts to approve and the amounts to delegate.
     * @param feePayer     Fee payer for the transaction (usually equals ownerWallet).
     * @param recentBlockhash Latest blockhash from `connection.getLatestBlockhash()`.
     * @return Base64-encoded unsigned transaction + the executor PDA address.
     */
    suspend fun createApproveTransaction(
        ownerWallet: String,
        tokens: List<TokenApprovalEntry>,
        feePayer: String = ownerWallet,
        recentBlockhash: String,
    ): Result<ApproveTransactionResponse> = withContext(Dispatchers.IO) {
        try {
            val req = CreateApproveTransactionRequest(
                ownerWallet = ownerWallet,
                feePayer = feePayer,
                recentBlockhash = recentBlockhash,
                tokens = tokens,
            )
            val resultJson = PolliNetFFI.createApproveTransaction(
                json.encodeToString(req).toByteArray()
            )
            parseResult<ApproveTransactionResponse>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Builds the canonical 169-byte borsh-encoded Intent and returns it as base64.
     * A random 16-byte nonce is generated automatically unless you supply [nonceHex].
     *
     * After this call:
     *  1. Sign [IntentBytesResponse.intentBytes] (decoded from base64) with the `from` wallet key.
     *  2. Submit via [submitIntent] or directly to pollicore `POST /sdk/intents/submit`.
     *
     * @param from          Source wallet public key (base58).
     * @param to            Destination wallet or token account (base58).
     * @param tokenMint     Token mint address (base58).
     * @param amount        Transfer amount in the token's smallest unit.
     * @param expiresAt     Unix timestamp (seconds) after which the intent is invalid.
     * @param gasFeeAmount  Amount paid to the gateway as a gas fee.
     * @param gasFeepayee   Gateway fee-recipient address (base58).
     * @param nonceHex      Optional 32-char lowercase hex nonce (16 bytes). Random if null.
     */
    suspend fun createIntentBytes(
        from: String,
        to: String,
        tokenMint: String,
        amount: Long,
        expiresAt: Long,
        gasFeeAmount: Long,
        gasFeepayee: String,
        nonceHex: String? = null,
    ): Result<IntentBytesResponse> = withContext(Dispatchers.IO) {
        try {
            val req = CreateIntentBytesRequest(
                from = from,
                to = to,
                tokenMint = tokenMint,
                amount = amount,
                expiresAt = expiresAt,
                gasFeeAmount = gasFeeAmount,
                gasFeepayee = gasFeepayee,
                nonceHex = nonceHex,
            )
            val resultJson = PolliNetFFI.createIntentBytes(
                json.encodeToString(req).toByteArray()
            )
            parseResult<IntentBytesResponse>(resultJson)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Builds a single unsigned transaction that revokes the executor PDA's delegate
     * authority over each of the specified token accounts.
     *
     * Sign the returned transaction with [ownerWallet] before submitting.
     *
     * @param ownerWallet  Base58 public key of the wallet that owns the token accounts.
     * @param tokenAccounts List of token account addresses to revoke.
     * @param feePayer     Fee payer (usually equals ownerWallet).
     * @param recentBlockhash Latest blockhash.
     * @param tokenProgram "spl-token" (default) or "token-2022".
     * @return Base64-encoded unsigned revoke transaction.
     */
    suspend fun createRevokeTransaction(
        ownerWallet: String,
        tokenAccounts: List<String>,
        feePayer: String = ownerWallet,
        recentBlockhash: String,
        tokenProgram: String = "spl-token",
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val req = CreateRevokeTransactionRequest(
                ownerWallet = ownerWallet,
                feePayer = feePayer,
                recentBlockhash = recentBlockhash,
                tokenAccounts = tokenAccounts,
                tokenProgram = tokenProgram,
            )
            val resultJson = PolliNetFFI.createRevokeTransaction(
                json.encodeToString(req).toByteArray()
            )
            parseResult<RevokeTransactionResponse>(resultJson).map { it.transaction }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Submits a signed intent to pollicore for on-chain execution.
     *
     * Prerequisites:
     *  - [initializeIntentState] has been called and confirmed.
     *  - The executor PDA has been granted delegate authority via [createApproveTransaction].
     *
     * @param intentBytesBase64   Base64-encoded 169-byte intent (from [createIntentBytes]).
     * @param signatureBase64     Base64-encoded 64-byte Ed25519 signature over [intentBytesBase64].
     * @param fromTokenAccount    SPL token account the tokens will be debited from.
     * @param polliCoreBaseUrl    Base URL of pollicore.
     * @param authToken           JWT access token.
     * @param tokenProgram        "spl-token" (default) or "token-2022".
     * @return The Solana transaction signature.
     */
    suspend fun submitIntent(
        intentBytesBase64: String,
        signatureBase64: String,
        fromTokenAccount: String,
        tokenProgram: String = "spl-token",
    ): Result<String> = withContext(Dispatchers.IO) {
        try {
            val reqJson = json.encodeToString(
                FfiSubmitIntentRequest(
                    intentBytes = intentBytesBase64,
                    signature = signatureBase64,
                    fromTokenAccount = fromTokenAccount,
                    tokenProgram = tokenProgram,
                )
            ).toByteArray(Charsets.UTF_8)
            val resultJson = PolliNetFFI.submitIntent(handle, reqJson)
            val result = parseResult<FfiSubmitIntentResponse>(resultJson)
            Result.success(result.getOrThrow().txSignature)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Returns the on-chain intent state for the authenticated wallet.
     * Use [IntentStateResponse.initialized] to decide whether [initializeIntentState]
     * still needs to be called.
     */
    /** Check whether the intent state PDA is initialized for [wallet] (no JWT). */
    suspend fun getIntentState(walletAddress: String): Result<IntentStateResponse> =
        withContext(Dispatchers.IO) {
            try {
                val url = pollicoreUrl() + "/sdk/intents/state?wallet=$walletAddress"
                Result.success(json.decodeFromString<IntentStateResponse>(polliCoreGet(url)))
            } catch (e: Exception) { Result.failure(e) }
        }

    /**
     * Fetch the partially-signed init transaction from pollicore.
     * Sign the returned [InitTxResponse.tx] with the user's wallet, then call
     * [initializeIntentState] to submit it and create the on-chain PDA.
     */
    suspend fun fetchInitTx(walletAddress: String): Result<InitTxResponse> =
        withContext(Dispatchers.IO) {
            try {
                val url = pollicoreUrl() + "/sdk/intents/init-tx?wallet=$walletAddress"
                Result.success(json.decodeFromString<InitTxResponse>(polliCoreGet(url)))
            } catch (e: Exception) { Result.failure(e) }
        }

    /**
     * Submit the user-signed init transaction to pollicore, which forwards it to Solana
     * to create the intent state PDA. Call this once per wallet before [submitIntent].
     *
     * @param signedTxBase64 Base64-encoded transaction after the user has signed it.
     * @param walletAddress  The user's base58 wallet address.
     */
    suspend fun initializeIntentState(
        signedTxBase64: String,
        walletAddress: String,
    ): Result<InitializeResponse> = withContext(Dispatchers.IO) {
        try {
            val body = json.encodeToString(
                InitializeRequest(tx = signedTxBase64, wallet = walletAddress)
            )
            val url = pollicoreUrl() + "/sdk/intents/initialize"
            Result.success(json.decodeFromString<InitializeResponse>(polliCorePostPublic(url, body)))
        } catch (e: Exception) { Result.failure(e) }
    }

    // Legacy overload kept for any callers that still pass a JWT
    @Deprecated("JWT no longer required; use getIntentState(walletAddress) instead")
    suspend fun getIntentState(
        polliCoreBaseUrl: String,
        authToken: String,
    ): Result<IntentStateResponse> = withContext(Dispatchers.IO) {
        try {
            val response = polliCoreGet("$polliCoreBaseUrl/sdk/intents/state", authToken)
            Result.success(json.decodeFromString<IntentStateResponse>(response))
        } catch (e: Exception) { Result.failure(e) }
    }

    // ─── HTTP helpers ─────────────────────────────────────────────────────────

    private fun pollicoreUrl(): String = PolliNetFFI.getPolliCoreUrl()

    private fun polliCoreGet(url: String): String {
        val conn = java.net.URL(url).openConnection() as java.net.HttpURLConnection
        conn.requestMethod = "GET"
        conn.connectTimeout = 10_000
        conn.readTimeout = 30_000
        return readPolliCoreResponse(conn)
    }

    private fun polliCoreGet(url: String, authToken: String): String {
        val conn = java.net.URL(url).openConnection() as java.net.HttpURLConnection
        conn.requestMethod = "GET"
        conn.setRequestProperty("Authorization", "Bearer $authToken")
        conn.connectTimeout = 10_000
        conn.readTimeout = 30_000
        return readPolliCoreResponse(conn)
    }

    private fun polliCorePostPublic(url: String, body: String): String {
        val conn = java.net.URL(url).openConnection() as java.net.HttpURLConnection
        conn.requestMethod = "POST"
        conn.setRequestProperty("Content-Type", "application/json")
        conn.doOutput = true
        conn.connectTimeout = 10_000
        conn.readTimeout = 30_000
        conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
        return readPolliCoreResponse(conn)
    }

    private fun readPolliCoreResponse(conn: java.net.HttpURLConnection): String {
        val statusCode = conn.responseCode
        val stream = if (statusCode in 200..299) conn.inputStream else conn.errorStream
        val body = stream?.bufferedReader(Charsets.UTF_8)?.readText() ?: ""
        if (statusCode !in 200..299) {
            throw PolliNetException("HTTP_$statusCode", "pollicore error $statusCode: $body")
        }
        return body
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    private suspend fun getQueueSize(ffiCall: (Long) -> String): Result<Int> =
        withContext(Dispatchers.IO) {
            try {
                parseResult<QueueSizeResponse>(ffiCall(handle)).map { it.queueSize }
            } catch (e: Exception) {
                Result.failure(e)
            }
        }

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
    val storageDirectory: String? = null,
    /** AES-256-GCM encryption key for nonce bundle storage. Required when [storageDirectory] is set. */
    val encryptionKey: String? = null,
    /**
     * Base58-encoded Solana wallet address that owns this node session.
     * When provided it is stored on the Rust transport and used to attribute
     * uptime, relay and submission rewards to the correct wallet.
     */
    @SerialName("walletAddress")
    val walletAddress: String? = null,
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
data class WalletAddressResponse(
    val address: String
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

/**
 * Request to accept and queue external pre-signed transaction
 */
@Serializable
internal data class AcceptExternalTransactionRequest(
    val version: Int = 1,
    @SerialName("base64SignedTx") val base64SignedTx: String,
    @SerialName("maxPayload") val maxPayload: Int? = null
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

// =============================================================================
// Peer / mesh health monitoring data types
// =============================================================================

/** Current connection state of a BLE mesh peer (mirrors Rust PeerState). */
@Serializable
enum class PeerState { Connected, Stale, Dead }

/** Per-peer health metrics returned by [PolliNetSDK.getHealthSnapshot]. */
@Serializable
data class PeerHealth(
    @SerialName("peer_id") val peerId: String,
    val state: PeerState,
    @SerialName("seconds_since_last_seen") val secondsSinceLastSeen: Long,
    @SerialName("latency_samples") val latencySamples: List<Int> = emptyList(),
    @SerialName("avg_latency_ms") val avgLatencyMs: Int = 0,
    val rssi: Int? = null,
    @SerialName("quality_score") val qualityScore: Int = 0,
    @SerialName("packets_sent") val packetsSent: Long = 0,
    @SerialName("packets_received") val packetsReceived: Long = 0,
    @SerialName("tx_failures") val txFailures: Long = 0,
    @SerialName("packet_loss_rate") val packetLossRate: Float = 0f
)

/** Network topology from the health monitor. */
@Serializable
data class NetworkTopology(
    @SerialName("direct_connections") val directConnections: List<String> = emptyList(),
    @SerialName("all_peers") val allPeers: List<String> = emptyList(),
    val connections: Map<String, List<String>> = emptyMap(),
    @SerialName("hop_counts") val hopCounts: Map<String, Int> = emptyMap()
)

/** Aggregate health metrics for the whole mesh. */
@Serializable
data class HealthMetrics(
    @SerialName("total_peers") val totalPeers: Int = 0,
    @SerialName("connected_peers") val connectedPeers: Int = 0,
    @SerialName("stale_peers") val stalePeers: Int = 0,
    @SerialName("dead_peers") val deadPeers: Int = 0,
    @SerialName("avg_latency_ms") val avgLatencyMs: Int = 0,
    @SerialName("max_latency_ms") val maxLatencyMs: Int = 0,
    @SerialName("min_latency_ms") val minLatencyMs: Int = 0,
    @SerialName("avg_packet_loss") val avgPacketLoss: Float = 0f,
    @SerialName("health_score") val healthScore: Int = 100,
    @SerialName("max_hops") val maxHops: Int = 0,
    val timestamp: String = ""
)

/** Full snapshot returned by [PolliNetSDK.getHealthSnapshot]. */
@Serializable
data class HealthSnapshot(
    val peers: List<PeerHealth> = emptyList(),
    val topology: NetworkTopology = NetworkTopology(),
    val metrics: HealthMetrics = HealthMetrics()
) {
    /** Peers currently in the Connected state. */
    val connectedPeers: List<PeerHealth> get() = peers.filter { it.state == PeerState.Connected }
    /** All peer addresses (connected + stale). */
    val knownPeerIds: List<String> get() = peers.map { it.peerId }
}

// =============================================================================
// Intent protocol data types
// =============================================================================

/** One token account to grant delegate authority in [PolliNetSDK.createApproveTransaction]. */
@Serializable
data class TokenApprovalEntry(
    @SerialName("mint_address")  val mintAddress: String,
    val amount: Long,
    val decimals: Int = 6,
    /** Owner's token account for this mint (ATA or custom). */
    @SerialName("token_account") val tokenAccount: String,
    /** "spl-token" (default) or "token-2022". */
    @SerialName("token_program") val tokenProgram: String = "spl-token",
)

/** Response from [PolliNetSDK.createApproveTransaction]. */
@Serializable
data class ApproveTransactionResponse(
    /** Base64-encoded unsigned transaction; sign with owner_wallet before submitting. */
    val transaction: String,
    /** The executor PDA that was granted delegate authority. */
    @SerialName("executor_pda") val executorPda: String,
)

/** Response from [PolliNetSDK.getExecutorPda]. */
@Serializable
data class ExecutorPdaResponse(
    val pda: String,
    val bump: Int,
)

/** Parameters for [PolliNetSDK.createIntentBytes]. */
@Serializable
internal data class CreateIntentBytesRequest(
    val from: String,
    val to: String,
    @SerialName("token_mint")      val tokenMint: String,
    val amount: Long,
    @SerialName("expires_at")      val expiresAt: Long,
    @SerialName("gas_fee_amount")  val gasFeeAmount: Long,
    @SerialName("gas_fee_payee")   val gasFeepayee: String,
    @SerialName("nonce_hex")       val nonceHex: String? = null,
)

/** Response from [PolliNetSDK.createIntentBytes]. */
@Serializable
data class IntentBytesResponse(
    /** Base64-encoded 169-byte intent — sign this with Ed25519 before submitting. */
    @SerialName("intent_bytes") val intentBytes: String,
    /** The 16-byte nonce used (32 lowercase hex chars). */
    @SerialName("nonce_hex")    val nonceHex: String,
)

/** Parameters for [PolliNetSDK.createApproveTransaction]. */
@Serializable
internal data class CreateApproveTransactionRequest(
    @SerialName("owner_wallet")      val ownerWallet: String,
    @SerialName("fee_payer")         val feePayer: String,
    @SerialName("recent_blockhash")  val recentBlockhash: String,
    val tokens: List<TokenApprovalEntry>,
)

/** Response from [PolliNetSDK.getIntentState]. */
@Serializable
data class IntentStateResponse(
    val initialized: Boolean,
    val user: String,
    val pda: String,
    @SerialName("total_executed") val totalExecuted: String? = null,
)

/** Response from [PolliNetSDK.fetchInitTx]. */
@Serializable
data class InitTxResponse(
    /** Base64-encoded partially-signed transaction. Sign this with the user's wallet. */
    val tx: String,
    val user: String,
)

/** Response from [PolliNetSDK.initializeIntentState]. */
@Serializable
data class InitializeResponse(
    val ok: Boolean,
    @SerialName("tx_signature") val txSignature: String,
)

// Internal pollicore API shapes (used via Rust FFI)

@Serializable
private data class FfiSubmitIntentRequest(
    @SerialName("intent_bytes")       val intentBytes: String,
    val signature: String,
    @SerialName("from_token_account") val fromTokenAccount: String,
    @SerialName("token_program")      val tokenProgram: String = "spl-token",
)

@Serializable
private data class FfiSubmitIntentResponse(
    val ok: Boolean,
    @SerialName("tx_signature") val txSignature: String,
)

@Serializable
internal data class PolliCoreSubmitIntentRequest(
    @SerialName("intent_bytes")        val intentBytes: String,
    val signature: String,
    @SerialName("from_token_account")  val fromTokenAccount: String,
    @SerialName("token_program")       val tokenProgram: String = "spl-token",
)

@Serializable
internal data class CreateRevokeTransactionRequest(
    @SerialName("owner_wallet")     val ownerWallet: String,
    @SerialName("fee_payer")        val feePayer: String,
    @SerialName("recent_blockhash") val recentBlockhash: String,
    @SerialName("token_accounts")   val tokenAccounts: List<String>,
    @SerialName("token_program")    val tokenProgram: String = "spl-token",
)

@Serializable
internal data class RevokeTransactionResponse(val transaction: String)

@Serializable
internal data class InitializeRequest(val tx: String, val wallet: String)

@Serializable
internal data class GatewayResponse(val wallet: String)

@Serializable
internal data class PolliCoreSubmitIntentResponse(
    val ok: Boolean,
    @SerialName("tx_signature") val txSignature: String,
)

