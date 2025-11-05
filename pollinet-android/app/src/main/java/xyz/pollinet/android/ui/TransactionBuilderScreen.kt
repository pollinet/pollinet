package xyz.pollinet.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import xyz.pollinet.sdk.*

@Composable
fun TransactionBuilderScreen(
    sdk: PolliNetSDK?
) {
    val scope = rememberCoroutineScope()
    var transactionType by remember { mutableStateOf(TransactionType.SOL) }
    
    // SOL Transfer fields
    var solSender by remember { mutableStateOf("") }
    var solRecipient by remember { mutableStateOf("") }
    var solFeePayer by remember { mutableStateOf("") }
    var solAmount by remember { mutableStateOf("") }
    var solNonceAccount by remember { mutableStateOf("") }
    
    // SPL Transfer fields
    var splSenderWallet by remember { mutableStateOf("") }
    var splRecipientWallet by remember { mutableStateOf("") }
    var splFeePayer by remember { mutableStateOf("") }
    var splMintAddress by remember { mutableStateOf("") }
    var splAmount by remember { mutableStateOf("") }
    var splNonceAccount by remember { mutableStateOf("") }
    
    var unsignedTxResult by remember { mutableStateOf<String?>(null) }
    var isBuilding by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Text(
            text = "Transaction Builder",
            style = MaterialTheme.typography.headlineMedium
        )

        if (sdk == null) {
            Text(
                text = "SDK not initialized",
                color = MaterialTheme.colorScheme.error
            )
            return@Column
        }

        HorizontalDivider()

        // Transaction Type Selector
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant
            )
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Transaction Type",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.primary
                )
                Spacer(modifier = Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(
                        selected = transactionType == TransactionType.SOL,
                        onClick = { transactionType = TransactionType.SOL },
                        label = { Text("SOL Transfer") }
                    )
                    FilterChip(
                        selected = transactionType == TransactionType.SPL,
                        onClick = { transactionType = TransactionType.SPL },
                        label = { Text("SPL Token") }
                    )
                }
            }
        }

        // Transaction Form
        when (transactionType) {
            TransactionType.SOL -> {
                SolTransferForm(
                    sender = solSender,
                    onSenderChange = { solSender = it },
                    recipient = solRecipient,
                    onRecipientChange = { solRecipient = it },
                    feePayer = solFeePayer,
                    onFeePayerChange = { solFeePayer = it },
                    amount = solAmount,
                    onAmountChange = { solAmount = it },
                    nonceAccount = solNonceAccount,
                    onNonceAccountChange = { solNonceAccount = it }
                )
            }
            TransactionType.SPL -> {
                SplTransferForm(
                    senderWallet = splSenderWallet,
                    onSenderWalletChange = { splSenderWallet = it },
                    recipientWallet = splRecipientWallet,
                    onRecipientWalletChange = { splRecipientWallet = it },
                    feePayer = splFeePayer,
                    onFeePayerChange = { splFeePayer = it },
                    mintAddress = splMintAddress,
                    onMintAddressChange = { splMintAddress = it },
                    amount = splAmount,
                    onAmountChange = { splAmount = it },
                    nonceAccount = splNonceAccount,
                    onNonceAccountChange = { splNonceAccount = it }
                )
            }
        }

        // Build Button
        Button(
            onClick = {
                scope.launch {
                    isBuilding = true
                    errorMessage = null
                    unsignedTxResult = null
                    
                    val result = when (transactionType) {
                        TransactionType.SOL -> {
                            sdk.createUnsignedTransaction(
                                CreateUnsignedTransactionRequest(
                                    sender = solSender,
                                    recipient = solRecipient,
                                    feePayer = solFeePayer,
                                    amount = solAmount.toLongOrNull() ?: 0,
                                    nonceAccount = solNonceAccount
                                )
                            )
                        }
                        TransactionType.SPL -> {
                            sdk.createUnsignedSplTransaction(
                                CreateUnsignedSplTransactionRequest(
                                    senderWallet = splSenderWallet,
                                    recipientWallet = splRecipientWallet,
                                    feePayer = splFeePayer,
                                    mintAddress = splMintAddress,
                                    amount = splAmount.toLongOrNull() ?: 0,
                                    nonceAccount = splNonceAccount
                                )
                            )
                        }
                    }
                    
                    isBuilding = false
                    
                    result.fold(
                        onSuccess = { tx -> unsignedTxResult = tx },
                        onFailure = { e -> errorMessage = e.message }
                    )
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isBuilding && sdk != null
        ) {
            if (isBuilding) {
                CircularProgressIndicator(
                    modifier = Modifier.size(24.dp),
                    color = MaterialTheme.colorScheme.onPrimary
                )
                Spacer(modifier = Modifier.width(8.dp))
            }
            Text(if (isBuilding) "Building..." else "Build Unsigned Transaction")
        }

        // Result Display
        errorMessage?.let { error ->
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer
                )
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        text = "Error",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.error
                    )
                    Text(
                        text = error,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onErrorContainer
                    )
                }
            }
        }

        unsignedTxResult?.let { tx ->
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer
                )
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        text = "✅ Unsigned Transaction Created",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Base64 Length: ${tx.length} bytes",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Transaction (truncated):",
                        style = MaterialTheme.typography.bodySmall
                    )
                    Text(
                        text = tx.take(100) + "...",
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace
                    )
                }
            }
        }
    }
}

@Composable
private fun SolTransferForm(
    sender: String,
    onSenderChange: (String) -> Unit,
    recipient: String,
    onRecipientChange: (String) -> Unit,
    feePayer: String,
    onFeePayerChange: (String) -> Unit,
    amount: String,
    onAmountChange: (String) -> Unit,
    nonceAccount: String,
    onNonceAccountChange: (String) -> Unit
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant
        )
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Text(
                text = "SOL Transfer Details",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            OutlinedTextField(
                value = sender,
                onValueChange = onSenderChange,
                label = { Text("Sender Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = recipient,
                onValueChange = onRecipientChange,
                label = { Text("Recipient Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = feePayer,
                onValueChange = onFeePayerChange,
                label = { Text("Fee Payer Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = amount,
                onValueChange = onAmountChange,
                label = { Text("Amount (lamports)") },
                modifier = Modifier.fillMaxWidth(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true
            )

            OutlinedTextField(
                value = nonceAccount,
                onValueChange = onNonceAccountChange,
                label = { Text("Nonce Account Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )
        }
    }
}

@Composable
private fun SplTransferForm(
    senderWallet: String,
    onSenderWalletChange: (String) -> Unit,
    recipientWallet: String,
    onRecipientWalletChange: (String) -> Unit,
    feePayer: String,
    onFeePayerChange: (String) -> Unit,
    mintAddress: String,
    onMintAddressChange: (String) -> Unit,
    amount: String,
    onAmountChange: (String) -> Unit,
    nonceAccount: String,
    onNonceAccountChange: (String) -> Unit
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant
        )
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Text(
                text = "SPL Token Transfer Details",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            OutlinedTextField(
                value = senderWallet,
                onValueChange = onSenderWalletChange,
                label = { Text("Sender Wallet Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = recipientWallet,
                onValueChange = onRecipientWalletChange,
                label = { Text("Recipient Wallet Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = feePayer,
                onValueChange = onFeePayerChange,
                label = { Text("Fee Payer Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = mintAddress,
                onValueChange = onMintAddressChange,
                label = { Text("Token Mint Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            OutlinedTextField(
                value = amount,
                onValueChange = onAmountChange,
                label = { Text("Amount (token units)") },
                modifier = Modifier.fillMaxWidth(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true
            )

            OutlinedTextField(
                value = nonceAccount,
                onValueChange = onNonceAccountChange,
                label = { Text("Nonce Account Address") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )
        }
    }
}

private enum class TransactionType {
    SOL, SPL
}

