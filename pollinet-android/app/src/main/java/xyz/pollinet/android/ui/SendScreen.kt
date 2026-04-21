package xyz.pollinet.android.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import kotlinx.coroutines.launch
import xyz.pollinet.android.mwa.PolliNetMwaClient
import xyz.pollinet.android.viewmodel.SendStep
import xyz.pollinet.android.viewmodel.SendViewModel
import xyz.pollinet.android.viewmodel.TokenAccount
import xyz.pollinet.android.viewmodel.WalletViewModel
import xyz.pollinet.sdk.PolliNetSDK

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendScreen(
    sdk: PolliNetSDK?,
    sendViewModel: SendViewModel,
    walletViewModel: WalletViewModel,
    mwaClient: PolliNetMwaClient,
    mwaSender: ActivityResultSender,
) {
    val sendState by sendViewModel.state.collectAsState()
    val walletState by walletViewModel.state.collectAsState()
    val scope = rememberCoroutineScope()
    val snackbarHostState = remember { SnackbarHostState() }

    LaunchedEffect(walletState.walletAddress) {
        walletState.walletAddress?.let { sendViewModel.setWallet(it) }
    }

    LaunchedEffect(sendState.error) {
        sendState.error?.let {
            snackbarHostState.showSnackbar(it, duration = SnackbarDuration.Long)
            sendViewModel.clearError()
        }
    }

    val approvedTokens = walletState.tokens.filter { it.isOfflineReady }
    val isBusy = sendState.step in listOf(
        SendStep.CREATING_INTENT, SendStep.AWAITING_SIGN,
        SendStep.TRANSFERRING, SendStep.SUBMITTING,
    )

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text("Send via Pollinet", fontWeight = FontWeight.Bold) },
                actions = {
                    if (sendState.step != SendStep.IDLE) {
                        TextButton(onClick = { sendViewModel.reset() }) { Text("Reset") }
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (walletState.walletAddress == null) {
                NoWalletBanner(); return@Column
            }
            if (approvedTokens.isEmpty()) {
                NoApprovedTokensBanner(); return@Column
            }
            if (sendState.step == SendStep.SUCCESS) {
                SuccessCard(
                    txSignature = sendState.txSignature ?: "",
                    onSendAgain = { sendViewModel.reset() },
                )
                return@Column
            }

            // ── Transfer form (always shown until SUCCESS) ──
            TransferForm(
                state = sendState,
                approvedTokens = approvedTokens,
                isBusy = isBusy,
                onRecipientChange = sendViewModel::setRecipient,
                onTokenSelect = sendViewModel::setToken,
                onAmountChange = sendViewModel::setAmount,
                onGasFeeChange = sendViewModel::setGasFee,
                onExpiryChange = sendViewModel::setExpiry,
            )

            // ── Action buttons ──
            ActionButtons(
                state = sendState,
                isBusy = isBusy,
                onCreateIntent = {
                    val s = sdk ?: return@ActionButtons
                    scope.launch {
                        sendViewModel.createIntent(
                            sdk = s,
                            signIntentFn = { bytes -> mwaClient.signMessage(mwaSender, bytes) },
                        )
                    }
                },
                onTransferViaBle = {
                    val s = sdk ?: return@ActionButtons
                    scope.launch { sendViewModel.transferViaBle(s) }
                },
                onSubmit = {
                    val s = sdk ?: return@ActionButtons
                    scope.launch { sendViewModel.submitToPollicore(s) }
                },
                onClearIntent = { sendViewModel.resetIntent() },
            )

            // ── Intent ready card ──
            AnimatedVisibility(
                visible = sendState.intentReady && sendState.step !in listOf(SendStep.CREATING_INTENT, SendStep.AWAITING_SIGN),
                enter = expandVertically(),
                exit = shrinkVertically(),
            ) {
                IntentReadyCard(state = sendState)
            }

            // ── Progress indicator ──
            AnimatedVisibility(visible = isBusy) {
                ProgressCard(step = sendState.step, label = sendState.stepLabel)
            }
        }
    }
}

// ─── Action buttons ───────────────────────────────────────────────────────────

@Composable
private fun ActionButtons(
    state: xyz.pollinet.android.viewmodel.SendUiState,
    isBusy: Boolean,
    onCreateIntent: () -> Unit,
    onTransferViaBle: () -> Unit,
    onSubmit: () -> Unit,
    onClearIntent: () -> Unit,
) {
    val formFilled = state.selectedToken != null && state.recipient.isNotBlank() && state.amountText.isNotBlank()

    if (!state.intentReady) {
        // ── Create Intent button ──
        Button(
            onClick = onCreateIntent,
            enabled = !isBusy && formFilled,
            modifier = Modifier.fillMaxWidth(),
            contentPadding = PaddingValues(vertical = 14.dp),
        ) {
            if (isBusy && state.step in listOf(SendStep.CREATING_INTENT, SendStep.AWAITING_SIGN)) {
                CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary)
                Spacer(Modifier.width(8.dp))
                Text(state.stepLabel)
            } else {
                Icon(Icons.Filled.Edit, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text("Create Intent", fontWeight = FontWeight.Bold)
            }
        }
    } else {
        // ── Transfer + Submit buttons ──
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            // Transfer via BLE
            OutlinedButton(
                onClick = onTransferViaBle,
                enabled = !isBusy,
                modifier = Modifier.weight(1f),
                contentPadding = PaddingValues(vertical = 12.dp),
            ) {
                if (isBusy && state.step == SendStep.TRANSFERRING) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(6.dp))
                    Text("Sending…", style = MaterialTheme.typography.labelMedium)
                } else {
                    Icon(Icons.Filled.Bluetooth, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(6.dp))
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("Transfer Intent", fontWeight = FontWeight.SemiBold,
                            style = MaterialTheme.typography.labelLarge)
                        Text("via BLE mesh", style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }

            // Submit to Pollicore
            Button(
                onClick = onSubmit,
                enabled = !isBusy,
                modifier = Modifier.weight(1f),
                contentPadding = PaddingValues(vertical = 12.dp),
            ) {
                if (isBusy && state.step == SendStep.SUBMITTING) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp,
                        color = MaterialTheme.colorScheme.onPrimary)
                    Spacer(Modifier.width(6.dp))
                    Text("Submitting…", style = MaterialTheme.typography.labelMedium)
                } else {
                    Icon(Icons.Filled.Send, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(6.dp))
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("Submit Intent", fontWeight = FontWeight.SemiBold,
                            style = MaterialTheme.typography.labelLarge)
                        Text("via Pollicore", style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.8f))
                    }
                }
            }
        }

        // ── Clear intent ──
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
            TextButton(
                onClick = onClearIntent,
                enabled = !isBusy,
                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
            ) {
                Icon(Icons.Filled.Close, contentDescription = null, modifier = Modifier.size(14.dp))
                Spacer(Modifier.width(4.dp))
                Text("Discard intent", style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}

// ─── Intent ready card ────────────────────────────────────────────────────────

@Composable
private fun IntentReadyCard(state: xyz.pollinet.android.viewmodel.SendUiState) {
    val clipboard = LocalClipboardManager.current
    OutlinedCard(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Icon(
                    if (state.step == SendStep.TRANSFERRED) Icons.Filled.Wifi else Icons.Filled.CheckCircle,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(16.dp),
                )
                Text(
                    if (state.step == SendStep.TRANSFERRED) "Intent queued for BLE relay (${state.fragmentCount} fragments)"
                    else "Intent signed and ready",
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.primary,
                )
            }

            val intentPreview = state.intentBytesBase64?.take(24) + "…"
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    "Intent: $intentPreview",
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                IconButton(
                    onClick = { clipboard.setText(AnnotatedString(state.intentBytesBase64 ?: "")) },
                    modifier = Modifier.size(28.dp),
                ) {
                    Icon(Icons.Filled.ContentCopy, contentDescription = "Copy intent bytes",
                        modifier = Modifier.size(14.dp))
                }
            }
            state.nonceHex?.let {
                Text(
                    "Nonce: ${it.take(16)}…",
                    style = MaterialTheme.typography.labelSmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

// ─── Progress card ────────────────────────────────────────────────────────────

@Composable
private fun ProgressCard(step: SendStep, label: String) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
            Column {
                Text(
                    when (step) {
                        SendStep.CREATING_INTENT -> "Building Intent"
                        SendStep.AWAITING_SIGN   -> "Waiting for Signature"
                        SendStep.TRANSFERRING    -> "Fragmenting for BLE"
                        SendStep.SUBMITTING      -> "Submitting to Pollicore"
                        else                     -> "Working…"
                    },
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                if (label.isNotBlank()) {
                    Text(label, style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

// ─── TransferForm ─────────────────────────────────────────────────────────────

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TransferForm(
    state: xyz.pollinet.android.viewmodel.SendUiState,
    approvedTokens: List<TokenAccount>,
    isBusy: Boolean,
    onRecipientChange: (String) -> Unit,
    onTokenSelect: (TokenAccount?) -> Unit,
    onAmountChange: (String) -> Unit,
    onGasFeeChange: (String) -> Unit,
    onExpiryChange: (Int) -> Unit,
) {
    val formLocked = isBusy || state.intentReady
    var tokenDropdownExpanded by remember { mutableStateOf(false) }

    OutlinedTextField(
        value = state.from.ifEmpty { "No wallet connected" },
        onValueChange = {},
        label = { Text("From") },
        enabled = false,
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
        leadingIcon = { Icon(Icons.Filled.AccountBalanceWallet, contentDescription = null) },
    )

    OutlinedTextField(
        value = state.recipient,
        onValueChange = onRecipientChange,
        label = { Text("Recipient Wallet Address") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
        enabled = !formLocked,
        placeholder = { Text("Base58 wallet address") },
        supportingText = { Text("Token account derived automatically") },
        leadingIcon = { Icon(Icons.Filled.Send, contentDescription = null) },
    )

    ExposedDropdownMenuBox(
        expanded = tokenDropdownExpanded,
        onExpandedChange = { if (!formLocked) tokenDropdownExpanded = it },
    ) {
        OutlinedTextField(
            value = state.selectedToken?.let { "${it.symbol} · ${WalletViewModel.formatUiAmount(it.delegatedRawAmount, it.decimals)} approved" }
                ?: "Select token",
            onValueChange = {},
            readOnly = true,
            label = { Text("Token") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = tokenDropdownExpanded) },
            modifier = Modifier.fillMaxWidth().menuAnchor(),
            enabled = !formLocked,
        )
        ExposedDropdownMenu(
            expanded = tokenDropdownExpanded,
            onDismissRequest = { tokenDropdownExpanded = false },
        ) {
            approvedTokens.forEach { token ->
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(token.symbol, fontWeight = FontWeight.SemiBold)
                            Text(
                                "Approved: ${WalletViewModel.formatUiAmount(token.delegatedRawAmount, token.decimals)} · Balance: ${WalletViewModel.formatUiAmount(token.rawAmount, token.decimals)}",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    },
                    onClick = { onTokenSelect(token); tokenDropdownExpanded = false },
                )
            }
        }
    }

    OutlinedTextField(
        value = state.amountText,
        onValueChange = onAmountChange,
        label = { Text("Amount") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
        enabled = !formLocked,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
        trailingIcon = {
            state.selectedToken?.let {
                Text(it.symbol, modifier = Modifier.padding(end = 12.dp),
                    style = MaterialTheme.typography.labelMedium)
            }
        },
        supportingText = state.selectedToken?.let {
            { Text("Max: ${WalletViewModel.formatUiAmount(it.delegatedRawAmount, it.decimals)} ${it.symbol}") }
        },
    )

    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Expires in:", style = MaterialTheme.typography.bodyMedium)
        listOf(5 to "5m", 15 to "15m", 60 to "1h", 1440 to "24h").forEach { (min, label) ->
            FilterChip(
                selected = state.expiresInMinutes == min,
                onClick = { onExpiryChange(min) },
                label = { Text(label) },
                enabled = !formLocked,
            )
        }
    }

    OutlinedTextField(
        value = state.gasFeeText,
        onValueChange = onGasFeeChange,
        label = { Text("Gas Fee (lamports)") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
        enabled = !formLocked,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        supportingText = { Text("Paid to the gateway for submitting the transaction") },
    )
}

// ─── Success card ─────────────────────────────────────────────────────────────

@Composable
private fun SuccessCard(txSignature: String, onSendAgain: () -> Unit) {
    val clipboard = LocalClipboardManager.current
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Icon(Icons.Filled.CheckCircle, contentDescription = null,
                tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(48.dp))
            Text("Intent Submitted!", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            Text(
                "Your intent was accepted by pollicore and broadcast to Solana.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )
            if (txSignature.isNotBlank()) {
                OutlinedCard(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("Transaction Signature", style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(txSignature, style = MaterialTheme.typography.bodySmall,
                            fontFamily = FontFamily.Monospace, maxLines = 2,
                            overflow = TextOverflow.Ellipsis)
                        TextButton(
                            onClick = { clipboard.setText(AnnotatedString(txSignature)) },
                            contentPadding = PaddingValues(0.dp),
                        ) {
                            Icon(Icons.Filled.ContentCopy, contentDescription = null, modifier = Modifier.size(14.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("Copy", style = MaterialTheme.typography.labelSmall)
                        }
                    }
                }
            }
            Button(onClick = onSendAgain, modifier = Modifier.fillMaxWidth()) {
                Text("Send Another")
            }
        }
    }
}

// ─── Empty state banners ──────────────────────────────────────────────────────

@Composable
private fun NoWalletBanner() {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(24.dp).fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(Icons.Filled.AccountBalanceWallet, contentDescription = null,
                modifier = Modifier.size(40.dp), tint = MaterialTheme.colorScheme.outline)
            Text("No Wallet Connected", style = MaterialTheme.typography.titleMedium)
            Text("Go to the Wallet tab and connect your Solana wallet first.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun NoApprovedTokensBanner() {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(24.dp).fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(Icons.Filled.Lock, contentDescription = null,
                modifier = Modifier.size(40.dp), tint = MaterialTheme.colorScheme.outline)
            Text("No Approved Tokens", style = MaterialTheme.typography.titleMedium)
            Text(
                "Enable offline capability for at least one token in the Wallet tab before sending an intent.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
