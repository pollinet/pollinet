package xyz.pollinet.android.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import xyz.pollinet.sdk.BleService
import xyz.pollinet.sdk.FragmentFFI
import xyz.pollinet.sdk.PolliNetSDK
import xyz.pollinet.sdk.Priority
import java.security.MessageDigest

// ─── UI state ─────────────────────────────────────────────────────────────────

enum class SendStep {
    IDLE,
    CREATING_INTENT,
    AWAITING_SIGN,
    INTENT_READY,      // Intent created + signed; awaiting Transfer or Submit
    TRANSFERRING,      // Fragmenting + pushing to BLE outbound queue
    TRANSFERRED,       // BLE broadcast enqueued; can still Submit
    SUBMITTING,        // HTTP submit to pollicore
    SUCCESS,
    ERROR,
}

data class SendUiState(
    // Form fields
    val from: String = "",
    val recipient: String = "",
    val selectedToken: TokenAccount? = null,
    val amountText: String = "",
    val gasFeeText: String = "1000",
    val expiresInMinutes: Int = 5,

    // Flow
    val step: SendStep = SendStep.IDLE,
    val stepLabel: String = "",

    // Persisted intent — survives between the three actions
    val intentBytesBase64: String? = null,
    val signatureBase64: String? = null,
    val fromTokenAccount: String? = null,
    val nonceHex: String? = null,

    // Results
    val txSignature: String? = null,
    val fragmentCount: Int? = null,
    val error: String? = null,
) {
    val intentReady: Boolean get() =
        intentBytesBase64 != null && signatureBase64 != null && fromTokenAccount != null
}

// ─── ViewModel ────────────────────────────────────────────────────────────────

class SendViewModel : ViewModel() {
    private val _state = MutableStateFlow(SendUiState())
    val state: StateFlow<SendUiState> = _state.asStateFlow()

    fun setWallet(address: String) = _state.update { it.copy(from = address) }
    fun setRecipient(r: String) = _state.update { it.copy(recipient = r, error = null) }
    fun setToken(t: TokenAccount?) = _state.update { it.copy(selectedToken = t, error = null) }
    fun setAmount(a: String) = _state.update { it.copy(amountText = a, error = null) }
    fun setGasFee(g: String) = _state.update { it.copy(gasFeeText = g) }
    fun setExpiry(m: Int) = _state.update { it.copy(expiresInMinutes = m) }
    fun clearError() = _state.update { it.copy(error = null) }
    fun resetIntent() = _state.update {
        it.copy(
            intentBytesBase64 = null, signatureBase64 = null,
            fromTokenAccount = null, nonceHex = null,
            fragmentCount = null, txSignature = null,
            step = SendStep.IDLE, stepLabel = "",
        )
    }
    fun reset() = _state.update { SendUiState(from = it.from) }

    private fun log(msg: String) = android.util.Log.d("SendViewModel", msg)
    private fun logE(msg: String, t: Throwable? = null) = android.util.Log.e("SendViewModel", msg, t)

    // ─── Step 1: Create Intent ────────────────────────────────────────────────

    /**
     * Builds the 169-byte intent, signs it with the user's wallet, and stores the result.
     * After this completes the user can press Transfer or Submit independently.
     */
    fun createIntent(
        sdk: PolliNetSDK,
        signIntentFn: suspend (messageBytes: ByteArray) -> ByteArray,
    ) {
        viewModelScope.launch {
            val s = _state.value
            val token = s.selectedToken ?: return@launch setError("Select a token")
            val recipient = s.recipient.trim()
            val from = s.from.trim()
            if (recipient.isBlank()) return@launch setError("Enter a recipient address")

            val rawAmount = WalletViewModel.parseUiAmount(s.amountText, token.decimals)
            if (rawAmount <= 0) return@launch setError("Enter a valid amount")

            val gasFee = s.gasFeeText.trim().toLongOrNull() ?: 1000L
            val expiresAt = System.currentTimeMillis() / 1000L + (s.expiresInMinutes * 60L)

            log("createIntent: from=$from recipient=$recipient mint=${token.mint} amount=$rawAmount gasFee=$gasFee expiresAt=$expiresAt")
            step(SendStep.CREATING_INTENT, "Deriving token accounts…")

            // The executor program requires token accounts (not wallet addresses) for both
            // to_token_account and gateway_fee_account. Derive ATAs deterministically.
            val recipientTokenAccount = try {
                sdk.deriveAssociatedTokenAccount(recipient, token.mint).getOrThrow()
            } catch (e: Exception) {
                logE("createIntent: failed to derive recipient ATA", e)
                return@launch setError("Failed to derive recipient token account: ${e.message}")
            }
            log("createIntent: recipientTokenAccount=$recipientTokenAccount")

            // Resolve the gateway's token account for the gas fee.
            // If gas_fee_amount = 0 and the gateway wallet is unreachable, fall back to
            // the user's own token account (Anchor skips the transfer when amount == 0).
            val (resolvedGasFeepayee, resolvedGasFee) = run {
                val gatewayWallet = sdk.getGatewayWallet().getOrNull()
                log("createIntent: gatewayWallet=$gatewayWallet")
                if (gatewayWallet != null) {
                    val gatewayAta = sdk.deriveAssociatedTokenAccount(gatewayWallet, token.mint).getOrNull()
                    log("createIntent: gatewayAta=$gatewayAta")
                    if (gatewayAta != null) {
                        gatewayAta to gasFee
                    } else {
                        log("createIntent: gateway ATA derivation failed — skipping fee")
                        token.pubkey to 0L
                    }
                } else {
                    log("createIntent: gateway unreachable (offline) — skipping fee")
                    token.pubkey to 0L
                }
            }

            step(SendStep.CREATING_INTENT, "Building intent…")
            log("createIntent: building intent bytes to=$recipientTokenAccount gasFeepayee=$resolvedGasFeepayee resolvedGasFee=$resolvedGasFee")
            val intentPayload = try {
                sdk.createIntentBytes(
                    from = from,
                    to = recipientTokenAccount,
                    tokenMint = token.mint,
                    amount = rawAmount,
                    expiresAt = expiresAt,
                    gasFeeAmount = resolvedGasFee,
                    gasFeepayee = resolvedGasFeepayee,
                ).getOrThrow()
            } catch (e: Exception) {
                logE("createIntent: createIntentBytes failed", e)
                return@launch setError("Failed to build intent: ${e.message}")
            }
            log("createIntent: intent built nonce=${intentPayload.nonceHex} bytes=${intentPayload.intentBytes.take(24)}…")

            step(SendStep.AWAITING_SIGN, "Waiting for wallet signature…")
            val intentBytes = try {
                android.util.Base64.decode(intentPayload.intentBytes, android.util.Base64.NO_WRAP)
            } catch (e: Exception) {
                logE("createIntent: failed to decode intent bytes", e)
                return@launch setError("Failed to decode intent bytes: ${e.message}")
            }
            log("createIntent: requesting wallet signature for ${intentBytes.size}-byte intent")
            val signature = try {
                signIntentFn(intentBytes)
            } catch (e: Exception) {
                logE("createIntent: signing cancelled/failed", e)
                return@launch setError("Signing cancelled or failed: ${e.message}")
            }
            log("createIntent: signature obtained (${signature.size} bytes) — intent ready")

            _state.update { it.copy(
                step = SendStep.INTENT_READY,
                stepLabel = "Intent ready",
                intentBytesBase64 = intentPayload.intentBytes,
                signatureBase64 = android.util.Base64.encodeToString(signature, android.util.Base64.NO_WRAP),
                fromTokenAccount = token.pubkey,
                nonceHex = intentPayload.nonceHex,
                error = null,
            ) }
        }
    }

    // ─── Step 2: Transfer via BLE ─────────────────────────────────────────────

    /**
     * Serialises the signed intent payload, fragments it for BLE MTU, and pushes it
     * to the outbound relay queue. Nearby nodes with internet will submit to pollicore.
     */
    fun transferViaBle(sdk: PolliNetSDK) {
        viewModelScope.launch {
            val s = _state.value
            val intentBytes = s.intentBytesBase64 ?: return@launch setError("Create an intent first")
            val signature  = s.signatureBase64   ?: return@launch setError("Create an intent first")
            val fromAcc    = s.fromTokenAccount  ?: return@launch setError("Create an intent first")

            // We must route through the running BleService — its internal SDK is the one whose
            // outbound_queue the BLE sending loop polls. Pushing to any other PolliNetSDK
            // instance lands the tx in a queue nobody reads, and the fragment never goes on the
            // wire. queueSignedTransaction also fires WorkEvent.OutboundReady so the loop wakes
            // up immediately rather than waiting for the next 800ms tick.
            val bleService = BleService.get() ?: return@launch setError(
                "BLE service not running — open Dev tab once to start it"
            )

            step(SendStep.TRANSFERRING, "Fragmenting for BLE…")

            // Compact JSON payload that a relay node can POST to pollicore directly
            val payloadJson = """{"intent_bytes":"$intentBytes","signature":"$signature","from_token_account":"$fromAcc","token_program":"spl-token"}"""
            val payloadBytes = payloadJson.toByteArray(Charsets.UTF_8)
            val txId = sha256Hex(payloadBytes)
            log("transferViaBle: payload=${payloadBytes.size}B txId=${txId.take(16)}…")

            val pushResult = bleService.queueSignedTransaction(payloadBytes, Priority.HIGH)

            pushResult.onFailure { e ->
                val isDuplicate = e.message?.contains("already in queue", ignoreCase = true) == true ||
                                  e.message?.contains("duplicate", ignoreCase = true) == true
                if (isDuplicate) {
                    log("transferViaBle: txId=${txId.take(16)}… already in BLE outbound queue — treating as TRANSFERRED")
                    _state.update { it.copy(
                        step = SendStep.TRANSFERRED,
                        stepLabel = "Already queued for BLE relay",
                        error = null,
                    ) }
                    return@launch
                }
                logE("transferViaBle: queueSignedTransaction failed", e)
                return@launch setError("Failed to queue for BLE: ${e.message}")
            }

            val fragmentCount = pushResult.getOrNull() ?: 0
            log("transferViaBle: pushed $fragmentCount fragments to BLE outbound queue — TRANSFERRED")
            _state.update { it.copy(
                step = SendStep.TRANSFERRED,
                stepLabel = "Queued for BLE relay",
                fragmentCount = fragmentCount,
                error = null,
            ) }
        }
    }

    // ─── Step 3: Submit to Pollicore ──────────────────────────────────────────

    /**
     * Directly submits the signed intent to pollicore over HTTP.
     * Can be called regardless of whether [transferViaBle] was called first.
     */
    fun submitToPollicore(sdk: PolliNetSDK) {
        viewModelScope.launch {
            val s = _state.value
            val intentBytes = s.intentBytesBase64 ?: return@launch setError("Create an intent first")
            val signature  = s.signatureBase64   ?: return@launch setError("Create an intent first")
            val fromAcc    = s.fromTokenAccount  ?: return@launch setError("Create an intent first")

            log("submitToPollicore: fromTokenAccount=$fromAcc intent=${intentBytes.take(24)}…")
            step(SendStep.SUBMITTING, "Submitting to Pollinet…")
            try {
                val txSig = sdk.submitIntent(
                    intentBytesBase64 = intentBytes,
                    signatureBase64 = signature,
                    fromTokenAccount = fromAcc,
                ).getOrThrow()

                log("submitToPollicore: SUCCESS txSignature=$txSig")
                _state.update { it.copy(
                    step = SendStep.SUCCESS,
                    stepLabel = "Submitted!",
                    txSignature = txSig,
                ) }
            } catch (e: Exception) {
                logE("submitToPollicore: failed", e)
                val friendlyMsg = when {
                    e.message?.contains("AccountNotInitialized") == true ||
                    e.message?.contains("to_token_account") == true ->
                        "Recipient's token account doesn't exist on-chain yet. " +
                        "They need to receive tokens for this mint at least once to initialize their account."
                    e.message?.contains("already in queue", ignoreCase = true) == true ->
                        "This intent is already queued for submission."
                    e.message?.contains("422") == true ->
                        "Pollicore rejected the transaction (422). Details: ${e.message}"
                    else -> "Submission failed: ${e.message}"
                }
                setError(friendlyMsg)
            }
        }
    }

    // ─── Helpers ─────────────────────────────────────────────────────────────

    private fun step(s: SendStep, label: String) =
        _state.update { it.copy(step = s, stepLabel = label, error = null) }

    private fun setError(msg: String) =
        _state.update { it.copy(step = if (it.intentReady) SendStep.INTENT_READY else SendStep.ERROR, stepLabel = "Failed", error = msg) }

    private fun sha256Hex(bytes: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
        return digest.joinToString("") { "%02x".format(it) }
    }
}
