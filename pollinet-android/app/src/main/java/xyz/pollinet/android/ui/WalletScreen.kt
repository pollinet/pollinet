package xyz.pollinet.android.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import xyz.pollinet.android.mwa.PolliNetMwaClient
import xyz.pollinet.android.mwa.MwaException
import xyz.pollinet.android.viewmodel.TokenAccount
import xyz.pollinet.android.viewmodel.WalletViewModel
import xyz.pollinet.sdk.PolliNetSDK

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WalletScreen(
    sdk: PolliNetSDK?,
    viewModel: WalletViewModel,
    mwaClient: PolliNetMwaClient,
    mwaSender: ActivityResultSender,
    onWalletConnected: (String) -> Unit,
) {
    val state by viewModel.state.collectAsState()
    val scope = rememberCoroutineScope()
    val snackbarHostState = remember { SnackbarHostState() }
    var isConnecting by remember { mutableStateOf(false) }

    // If the SDK initializes after the wallet was already connected (or vice versa), kick off
    // the initial token load. listTokenAccounts depends on the SDK, which may arrive on a
    // different frame than the wallet authorize callback.
    LaunchedEffect(sdk, state.walletAddress) {
        if (sdk != null && state.walletAddress != null && state.tokens.isEmpty() && !state.isLoadingTokens) {
            viewModel.loadTokenAccounts(sdk)
        }
    }

    // Show errors / status as snackbar
    LaunchedEffect(state.error) {
        state.error?.let { snackbarHostState.showSnackbar(it, duration = SnackbarDuration.Short) }
        if (state.error != null) viewModel.clearError()
    }
    LaunchedEffect(state.statusMessage) {
        state.statusMessage?.let { snackbarHostState.showSnackbar(it, duration = SnackbarDuration.Short) }
        if (state.statusMessage != null) viewModel.clearStatus()
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text("Wallet", fontWeight = FontWeight.Bold) },
                actions = {
                    IconButton(onClick = { viewModel.toggleSettings() }) {
                        Icon(Icons.Filled.Settings, contentDescription = "Settings")
                    }
                }
            )
        }
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // ── Settings panel ──
            item {
                AnimatedVisibility(visible = state.showSettings) {
                    SettingsPanel(
                        rpcUrl = state.rpcUrl,
                        onRefreshTokens = { sdk?.let { viewModel.loadTokenAccounts(it) } },
                    )
                }
            }

            // ── Wallet card ──
            item {
                WalletCard(
                    address = state.walletAddress,
                    executorPda = state.executorPda,
                    isConnecting = isConnecting,
                    onConnect = {
                        scope.launch {
                            isConnecting = true
                            try {
                                val pubkey = mwaClient.authorize(mwaSender)
                                viewModel.onWalletConnected(pubkey, sdk)
                                onWalletConnected(pubkey)
                            } catch (e: Exception) {
                                snackbarHostState.showSnackbar(
                                    e.message ?: "Wallet connection failed",
                                    duration = SnackbarDuration.Long,
                                )
                            } finally {
                                isConnecting = false
                            }
                        }
                    },
                    onDisconnect = {
                        scope.launch {
                            try {
                                mwaClient.deauthorize(mwaSender)
                            } catch (_: Exception) {}
                            viewModel.onWalletDisconnected()
                        }
                    }
                )
            }

            // ── Token list header ──
            if (state.walletAddress != null) {
                item {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            "Token Accounts",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        if (state.isLoadingTokens) {
                            CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                        } else {
                            IconButton(
                                onClick = { sdk?.let { viewModel.loadTokenAccounts(it) } },
                                modifier = Modifier.size(32.dp),
                                enabled = sdk != null,
                            ) {
                                Icon(Icons.Filled.Refresh, contentDescription = "Refresh", modifier = Modifier.size(18.dp))
                            }
                        }
                    }
                }

                if (state.tokens.isEmpty() && !state.isLoadingTokens) {
                    item {
                        Text(
                            "No SPL token accounts found for this wallet.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                // ── Token cards ──
                items(state.tokens, key = { it.pubkey }) { token ->
                    val draft = state.approvalDrafts[token.mint]
                    TokenCard(
                        token = token,
                        draft = draft,
                        sdk = sdk,
                        onToggleApprove = { enabled ->
                            if (enabled) viewModel.openApprovePanel(token)
                            else viewModel.closeApprovePanel(token.mint)
                        },
                        onAmountChange = { viewModel.updateApprovalAmount(token.mint, it) },
                        onApprove = {
                            if (sdk == null) return@TokenCard
                            viewModel.approveToken(
                                sdk = sdk,
                                mint = token.mint,
                                signTx = { txBase64 ->
                                    val signedBytes = mwaClient.signTransaction(mwaSender, txBase64)
                                    signedBytes
                                },
                                submitTx = { signedBytes ->
                                    // Submit raw signed transaction via Solana RPC
                                    submitSignedTx(signedBytes, state.rpcUrl)
                                },
                            )
                        },
                        onRevoke = {
                            if (sdk == null) return@TokenCard
                            viewModel.revokeToken(
                                sdk = sdk,
                                token = token,
                                signTx = { txBase64 ->
                                    mwaClient.signTransaction(mwaSender, txBase64)
                                },
                                submitTx = { signedBytes ->
                                    submitSignedTx(signedBytes, state.rpcUrl)
                                },
                            )
                        },
                    )
                }
            }
        }
    }
}

// ─── WalletCard ───────────────────────────────────────────────────────────────

@Composable
private fun WalletCard(
    address: String?,
    executorPda: String?,
    isConnecting: Boolean,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(Icons.Filled.AccountBalanceWallet, contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary)
                Text("Connected Wallet", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            }

            if (address != null) {
                Text(
                    address,
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (executorPda != null) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("Executor PDA:", style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f))
                        Text(
                            "${executorPda.take(6)}…${executorPda.takeLast(4)}",
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                        )
                    }
                }
                OutlinedButton(onClick = onDisconnect, modifier = Modifier.align(Alignment.End)) {
                    Text("Disconnect")
                }
            } else {
                Text(
                    "Connect your Solana wallet to manage token approvals and send intents.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.8f),
                )
                Button(
                    onClick = onConnect,
                    enabled = !isConnecting,
                    modifier = Modifier.align(Alignment.End),
                ) {
                    if (isConnecting) {
                        CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(8.dp))
                    }
                    Text(if (isConnecting) "Connecting…" else "Connect Wallet")
                }
            }
        }
    }
}

// ─── TokenCard ────────────────────────────────────────────────────────────────

@Composable
private fun TokenCard(
    token: TokenAccount,
    draft: xyz.pollinet.android.viewmodel.ApprovalDraft?,
    sdk: PolliNetSDK?,
    onToggleApprove: (Boolean) -> Unit,
    onAmountChange: (String) -> Unit,
    onApprove: () -> Unit,
    onRevoke: () -> Unit,
) {
    val isPanelOpen = draft != null
    val isProcessing = draft?.isProcessing == true

    Card(
        modifier = Modifier.fillMaxWidth(),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            // ── Token header row ──
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                // Token icon / badge
                Box(
                    modifier = Modifier
                        .size(40.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.secondaryContainer),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        token.symbol.take(2).uppercase(),
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }

                Column(modifier = Modifier.weight(1f)) {
                    Text(token.symbol, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                    Text(
                        token.shortPubkey,
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                // Balance
                Column(horizontalAlignment = Alignment.End) {
                    Text(
                        WalletViewModel.formatUiAmount(token.rawAmount, token.decimals),
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                    )
                    Text(
                        token.symbol,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            Spacer(Modifier.height(8.dp))

            // ── Offline capability row ──
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        if (token.isOfflineReady) {
                            Icon(
                                Icons.Filled.CheckCircle,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.size(14.dp),
                            )
                            Text(
                                "Offline Ready · ${WalletViewModel.formatUiAmount(token.delegatedRawAmount, token.decimals)} delegated",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.primary,
                            )
                        } else {
                            Text(
                                "Offline capability off",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }

                // Revoke button (only when approved)
                if (token.isOfflineReady) {
                    TextButton(
                        onClick = onRevoke,
                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                    ) {
                        Icon(Icons.Filled.Close, contentDescription = null, modifier = Modifier.size(14.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Revoke", style = MaterialTheme.typography.labelSmall)
                    }
                }

                // Toggle switch
                Switch(
                    checked = isPanelOpen || token.isOfflineReady,
                    onCheckedChange = { enabled ->
                        if (!enabled && token.isOfflineReady) onRevoke()
                        else onToggleApprove(enabled)
                    },
                    enabled = !isProcessing,
                )
            }

            // ── Approval panel (animated) ──
            AnimatedVisibility(
                visible = isPanelOpen,
                enter = expandVertically(),
                exit = shrinkVertically(),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 8.dp)
                        .clip(RoundedCornerShape(8.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(12.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        "Set delegation limit",
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        "The executor PDA will be allowed to spend up to this amount on your behalf when relaying intents offline.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )

                    OutlinedTextField(
                        value = draft?.amountText ?: "",
                        onValueChange = onAmountChange,
                        label = { Text("Amount (${token.symbol})") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        trailingIcon = {
                            Text(
                                token.symbol,
                                style = MaterialTheme.typography.labelMedium,
                                modifier = Modifier.padding(end = 12.dp),
                            )
                        },
                    )

                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                        OutlinedButton(
                            onClick = { onToggleApprove(false) },
                            modifier = Modifier.weight(1f),
                        ) { Text("Cancel") }

                        Button(
                            onClick = onApprove,
                            enabled = !isProcessing && (draft?.amountText?.isNotBlank() == true),
                            modifier = Modifier.weight(1f),
                        ) {
                            if (isProcessing) {
                                CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp,
                                    color = MaterialTheme.colorScheme.onPrimary)
                            } else {
                                Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(16.dp))
                            }
                            Spacer(Modifier.width(6.dp))
                            Text(if (isProcessing) "Approving…" else "Approve")
                        }
                    }
                }
            }
        }
    }
}

// ─── Settings panel ───────────────────────────────────────────────────────────

@Composable
private fun SettingsPanel(
    rpcUrl: String,
    onRefreshTokens: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Settings", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)
            Button(onClick = onRefreshTokens, modifier = Modifier.align(Alignment.End)) {
                Icon(Icons.Filled.Refresh, contentDescription = null, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text("Reload Tokens")
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/** Submit a fully-signed raw transaction to Solana via JSON-RPC. Returns the signature. */
private suspend fun submitSignedTx(signedTxBytes: ByteArray, rpcUrl: String): String =
    withContext(Dispatchers.IO) {
        val encoded = android.util.Base64.encodeToString(signedTxBytes, android.util.Base64.NO_WRAP)
        val body = """{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["$encoded",{"encoding":"base64","preflightCommitment":"confirmed"}]}"""
        val conn = java.net.URL(rpcUrl).openConnection() as java.net.HttpURLConnection
        conn.requestMethod = "POST"
        conn.setRequestProperty("Content-Type", "application/json")
        conn.doOutput = true
        conn.connectTimeout = 15_000
        conn.readTimeout = 30_000
        conn.outputStream.use { it.write(body.toByteArray()) }
        val responseCode = conn.responseCode
        val responseBody = if (responseCode in 200..299) {
            conn.inputStream.bufferedReader().readText()
        } else {
            val err = conn.errorStream?.bufferedReader()?.readText() ?: ""
            throw Exception("RPC HTTP $responseCode: $err")
        }
        val match = Regex("\"result\"\\s*:\\s*\"([^\"]+)\"").find(responseBody)
        match?.groupValues?.get(1) ?: throw Exception("sendTransaction failed: $responseBody")
    }
