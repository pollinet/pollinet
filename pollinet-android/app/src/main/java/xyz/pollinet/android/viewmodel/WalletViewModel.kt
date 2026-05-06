package xyz.pollinet.android.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import xyz.pollinet.sdk.PolliNetSDK
import xyz.pollinet.sdk.TokenApprovalEntry

// ─── Domain models ────────────────────────────────────────────────────────────

data class TokenAccount(
    val pubkey: String,
    val mint: String,
    val symbol: String,
    val uiAmount: Double,
    val rawAmount: Long,
    val decimals: Int,
    val delegate: String?,
    val delegatedRawAmount: Long,
    // True iff `delegate` is the Pollinet executor PDA AND `delegatedRawAmount > 0`.
    // Source of truth comes from `PolliNetSDK.listTokenAccounts` which compares against the
    // canonical executor PDA from the Rust SDK — don't recompute this client-side.
    val isExecutorDelegated: Boolean = false,
) {
    /** True only when this token is delegated to the Pollinet executor PDA with a non-zero amount. */
    val isOfflineReady: Boolean get() = isExecutorDelegated

    val shortMint: String get() =
        if (mint.length > 12) "${mint.take(6)}…${mint.takeLast(4)}" else mint

    val shortPubkey: String get() =
        if (pubkey.length > 12) "${pubkey.take(6)}…${pubkey.takeLast(4)}" else pubkey
}

// Pending approval state for a single token
data class ApprovalDraft(
    val tokenAccount: TokenAccount,
    val amountText: String = "",
    val isProcessing: Boolean = false,
)

// ─── UI state ─────────────────────────────────────────────────────────────────

data class WalletUiState(
    val walletAddress: String? = null,
    val isConnecting: Boolean = false,
    val tokens: List<TokenAccount> = emptyList(),
    val isLoadingTokens: Boolean = false,
    val executorPda: String? = null,
    // Approval drafts: mint → draft (only set when user opens the approve panel)
    val approvalDrafts: Map<String, ApprovalDraft> = emptyMap(),
    val showSettings: Boolean = false,
    val statusMessage: String? = null,
    val error: String? = null,
    val rpcUrl: String = "https://devnet.helius-rpc.com/?api-key=a8d3dc32-abdb-43b6-8638-74bd01d728a4",
)

// ─── ViewModel ────────────────────────────────────────────────────────────────

class WalletViewModel : ViewModel() {
    private val _state = MutableStateFlow(WalletUiState())
    val state: StateFlow<WalletUiState> = _state.asStateFlow()

    // Called from MainActivity once wallet is connected
    fun onWalletConnected(address: String, sdk: PolliNetSDK?) {
        _state.update { it.copy(walletAddress = address, error = null) }
        if (sdk != null) {
            loadExecutorPda(sdk)
            loadTokenAccounts(sdk)
        }
    }

    fun onWalletDisconnected() {
        _state.update { WalletUiState() }
    }

    fun toggleSettings() = _state.update { it.copy(showSettings = !it.showSettings) }
    fun clearError() = _state.update { it.copy(error = null) }
    fun clearStatus() = _state.update { it.copy(statusMessage = null) }

    // Open the approve amount panel for a token
    fun openApprovePanel(token: TokenAccount) {
        _state.update { s ->
            val draft = ApprovalDraft(
                tokenAccount = token,
                amountText = if (token.isOfflineReady)
                    formatUiAmount(token.delegatedRawAmount, token.decimals) else "",
            )
            s.copy(approvalDrafts = s.approvalDrafts + (token.mint to draft))
        }
    }

    fun closeApprovePanel(mint: String) {
        _state.update { s -> s.copy(approvalDrafts = s.approvalDrafts - mint) }
    }

    fun updateApprovalAmount(mint: String, text: String) {
        _state.update { s ->
            val draft = s.approvalDrafts[mint] ?: return@update s
            s.copy(approvalDrafts = s.approvalDrafts + (mint to draft.copy(amountText = text)))
        }
    }

    // ─── Approve ─────────────────────────────────────────────────────────────

    fun approveToken(
        sdk: PolliNetSDK,
        mint: String,
        signTx: suspend (txBase64: String) -> ByteArray,
        submitTx: suspend (signedTxBytes: ByteArray) -> String,
    ) {
        viewModelScope.launch {
            val s = _state.value
            val draft = s.approvalDrafts[mint] ?: return@launch
            val wallet = s.walletAddress ?: return@launch

            val rawAmount = parseUiAmount(draft.amountText, draft.tokenAccount.decimals)
            if (rawAmount <= 0) {
                _state.update { it.copy(error = "Enter a valid amount to approve") }
                return@launch
            }

            _state.update { st ->
                val d = st.approvalDrafts[mint] ?: return@update st
                st.copy(approvalDrafts = st.approvalDrafts + (mint to d.copy(isProcessing = true)))
            }

            try {
                // Step 0: ensure the on-chain intent state PDA is initialized (once per wallet)
                _state.update { it.copy(statusMessage = "Checking intent state…") }
                val intentState = sdk.getIntentState(wallet).getOrNull()
                if (intentState?.initialized != true) {
                    _state.update { it.copy(statusMessage = "Initializing intent state (sign in wallet)…") }
                    val initTx = sdk.fetchInitTx(wallet).getOrThrow()
                    val signedInitBytes = signTx(initTx.tx)
                    val signedInitBase64 = android.util.Base64.encodeToString(signedInitBytes, android.util.Base64.NO_WRAP)
                    sdk.initializeIntentState(signedInitBase64, wallet).getOrThrow()
                }

                // Step 1: approve token for executor PDA
                _state.update { it.copy(statusMessage = "Sign approval transaction in wallet…") }
                val blockhash = sdk.fetchRecentBlockhash().getOrThrow()

                val approveTx = sdk.createApproveTransaction(
                    ownerWallet = wallet,
                    tokens = listOf(
                        TokenApprovalEntry(
                            mintAddress = mint,
                            amount = rawAmount,
                            decimals = draft.tokenAccount.decimals,
                            tokenAccount = draft.tokenAccount.pubkey,
                        )
                    ),
                    recentBlockhash = blockhash,
                ).getOrThrow()

                val signedBytes = signTx(approveTx.transaction)
                submitTx(signedBytes)

                _state.update { it.copy(
                    statusMessage = "Approved ${formatUiAmount(rawAmount, draft.tokenAccount.decimals)} ${draft.tokenAccount.symbol} for offline use",
                    approvalDrafts = it.approvalDrafts - mint,
                ) }
                loadTokenAccounts(sdk)
            } catch (e: Exception) {
                _state.update { st ->
                    val d = st.approvalDrafts[mint]
                    val drafts = if (d != null) st.approvalDrafts + (mint to d.copy(isProcessing = false))
                                 else st.approvalDrafts
                    st.copy(error = "Setup failed: ${e.message ?: e.javaClass.simpleName}", approvalDrafts = drafts)
                }
            }
        }
    }

    // ─── Revoke ──────────────────────────────────────────────────────────────

    fun revokeToken(
        sdk: PolliNetSDK,
        token: TokenAccount,
        signTx: suspend (txBase64: String) -> ByteArray,
        submitTx: suspend (signedTxBytes: ByteArray) -> String,
    ) {
        viewModelScope.launch {
            val s = _state.value
            val wallet = s.walletAddress ?: return@launch

            _state.update { it.copy(statusMessage = "Revoking ${token.symbol}…") }

            try {
                val blockhash = sdk.fetchRecentBlockhash().getOrThrow()

                val revokeTx = sdk.createRevokeTransaction(
                    ownerWallet = wallet,
                    tokenAccounts = listOf(token.pubkey),
                    recentBlockhash = blockhash,
                ).getOrThrow()

                val signedBytes = signTx(revokeTx)
                submitTx(signedBytes)

                _state.update { it.copy(statusMessage = "${token.symbol} offline capability revoked") }
                loadTokenAccounts(sdk)
            } catch (e: Exception) {
                _state.update { it.copy(error = "Revoke failed: ${e.message}", statusMessage = null) }
            }
        }
    }

    // ─── Load token accounts ─────────────────────────────────────────────────

    fun loadTokenAccounts(sdk: PolliNetSDK) {
        val wallet = _state.value.walletAddress ?: return
        viewModelScope.launch {
            _state.update { it.copy(isLoadingTokens = true) }
            try {
                val delegated = sdk.listTokenAccounts(wallet).getOrThrow()
                val accounts = delegated.map { dt ->
                    TokenAccount(
                        pubkey = dt.pubkey,
                        mint = dt.mint,
                        symbol = KNOWN_TOKENS[dt.mint] ?: dt.mint.take(6),
                        uiAmount = if (dt.decimals > 0) {
                            dt.rawBalance.toDouble() / Math.pow(10.0, dt.decimals.toDouble())
                        } else dt.rawBalance.toDouble(),
                        rawAmount = dt.rawBalance,
                        decimals = dt.decimals,
                        delegate = dt.delegate,
                        delegatedRawAmount = dt.delegatedRawAmount,
                        isExecutorDelegated = dt.isExecutorDelegated,
                    )
                }
                _state.update { it.copy(tokens = accounts, isLoadingTokens = false) }
            } catch (e: Exception) {
                _state.update { it.copy(isLoadingTokens = false, error = "Failed to load tokens: ${e.message}") }
            }
        }
    }

    /** Resolve the canonical executor PDA from the SDK and cache it on UI state. */
    fun loadExecutorPda(sdk: PolliNetSDK) {
        viewModelScope.launch {
            sdk.getExecutorPda()
                .onSuccess { resp -> _state.update { it.copy(executorPda = resp.pda) } }
                .onFailure { /* leave executorPda as null; UI handles missing value */ }
        }
    }

    companion object {
        val KNOWN_TOKENS = mapOf(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" to "USDC",
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" to "USDT",
            "So11111111111111111111111111111111111111112"  to "SOL",
            "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs" to "ETH",
            "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"  to "mSOL",
            "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263" to "BONK",
        )

        fun formatUiAmount(raw: Long, decimals: Int): String {
            val divisor = Math.pow(10.0, decimals.toDouble())
            val ui = raw / divisor
            return if (ui == ui.toLong().toDouble()) ui.toLong().toString()
            else "%.${decimals.coerceAtMost(6)}f".format(ui).trimEnd('0').trimEnd('.')
        }

        fun parseUiAmount(text: String, decimals: Int): Long {
            val d = text.trim().toDoubleOrNull() ?: return -1L
            if (d <= 0) return -1L
            return (d * Math.pow(10.0, decimals.toDouble())).toLong()
        }
    }
}
