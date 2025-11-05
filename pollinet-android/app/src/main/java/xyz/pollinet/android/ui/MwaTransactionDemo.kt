package xyz.pollinet.android.ui

import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import kotlinx.coroutines.launch
import xyz.pollinet.android.mwa.PolliNetMwaClient
import xyz.pollinet.android.mwa.MwaException
import xyz.pollinet.sdk.PolliNetSDK

/**
 * Demo screen for MWA (Mobile Wallet Adapter) integration with PolliNet
 * 
 * Shows the complete flow:
 * 1. Authorize with Solana Mobile wallet (Solflare, Phantom, etc.)
 * 2. Prepare offline bundle (durable nonces)
 * 3. Create UNSIGNED transaction using public keys only
 * 4. Sign transaction securely with MWA/Seed Vault
 * 5. Submit signed transaction to blockchain
 */
@Composable
fun MwaTransactionDemo(
    sdk: PolliNetSDK?,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    
    // MWA client state
    var mwaClient by remember { mutableStateOf<PolliNetMwaClient?>(null) }
    var activityResultSender by remember { mutableStateOf<ActivityResultSender?>(null) }
    
    // UI state
    var authorizedPubkey by remember { mutableStateOf<String?>(null) }
    var statusMessage by remember { mutableStateOf("Ready. Connect wallet to begin.") }
    var isLoading by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    
    // Transaction state
    var unsignedTxBase64 by remember { mutableStateOf<String?>(null) }
    var signedTxBase64 by remember { mutableStateOf<String?>(null) }
    var txSignature by remember { mutableStateOf<String?>(null) }
    
    // Create ActivityResultSender SYNCHRONOUSLY at top level (BEFORE lifecycle STARTED)
    // This MUST be done with remember, not in LaunchedEffect or DisposableEffect
    val activityContext = context as? ComponentActivity
    
    // Initialize ActivityResultSender synchronously
    remember(activityContext) {
        if (activityContext != null) {
            activityResultSender = ActivityResultSender(activityContext)
        } else {
            errorMessage = "MWA requires ComponentActivity. Current context: ${context.javaClass.simpleName}"
        }
        Unit  // remember must return something
    }
    
    // Initialize MWA client
    LaunchedEffect(Unit) {
        mwaClient = PolliNetMwaClient.create(
            context = context,
            identityUri = "https://pollinet.xyz",
            iconUri = "favicon.ico",  // Relative to assets
            identityName = "PolliNet"
        )
    }
    
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        // Header
        Text(
            text = "MWA Transaction Demo",
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.primary
        )
        
        Divider()
        
        // Status card
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = when {
                    errorMessage != null -> MaterialTheme.colorScheme.errorContainer
                    isLoading -> MaterialTheme.colorScheme.secondaryContainer
                    authorizedPubkey != null -> MaterialTheme.colorScheme.primaryContainer
                    else -> MaterialTheme.colorScheme.surfaceVariant
                }
            )
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Status",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(text = statusMessage)
                
                if (errorMessage != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Error: $errorMessage",
                        color = MaterialTheme.colorScheme.error,
                        fontSize = 14.sp
                    )
                }
                
                if (authorizedPubkey != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Wallet: ${authorizedPubkey!!.take(8)}...${authorizedPubkey!!.takeLast(8)}",
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Medium
                    )
                }
            }
        }
        
        // Step 1: Authorization
        Card(
            modifier = Modifier.fillMaxWidth()
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Step 1: Authorize Wallet",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Connect to Solana Mobile wallet (Solflare, Phantom). Your private keys stay secure in Seed Vault.",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(modifier = Modifier.height(12.dp))
                
                Button(
                    onClick = {
                        scope.launch {
                            isLoading = true
                            errorMessage = null
                            statusMessage = "Opening wallet..."
                            try {
                                val sender = activityResultSender
                                    ?: throw MwaException("ActivityResultSender not initialized")
                                
                                // Call actual MWA authorization
                                val pubkey = mwaClient!!.authorize(sender)
                                authorizedPubkey = pubkey
                                statusMessage = "Wallet connected successfully!"
                                
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Authorization failed"
                                statusMessage = "Authorization failed"
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && mwaClient != null && activityResultSender != null && authorizedPubkey == null,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (isLoading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                    }
                    Text("Connect Wallet")
                }
            }
        }
        
        // Step 2: Prepare offline bundle
        Card(
            modifier = Modifier.fillMaxWidth()
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Step 2: Prepare Offline Bundle",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Create durable nonce accounts for offline transactions. This requires the wallet to sign nonce creation transactions.",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(modifier = Modifier.height(12.dp))
                
                Button(
                    onClick = {
                        scope.launch {
                            isLoading = true
                            errorMessage = null
                            statusMessage = "Preparing offline bundle..."
                            try {
                                // TODO: This step needs MWA to sign nonce creation
                                // For now, we'll note it requires wallet signing
                                statusMessage = "Note: Bundle preparation requires wallet signing for nonce accounts"
                                errorMessage = "TODO: Implement nonce creation with MWA signing"
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Bundle preparation failed"
                                statusMessage = "Failed to prepare bundle"
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && authorizedPubkey != null && sdk != null,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (isLoading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                    }
                    Text("Prepare Bundle")
                }
            }
        }
        
        // Step 3: Create unsigned transaction
        Card(
            modifier = Modifier.fillMaxWidth()
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Step 3: Create Unsigned Transaction",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Create transaction using PUBLIC KEYS only. No private keys are sent to PolliNet - they stay in Seed Vault!",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(modifier = Modifier.height(12.dp))
                
                Button(
                    onClick = {
                        scope.launch {
                            isLoading = true
                            errorMessage = null
                            statusMessage = "Creating unsigned transaction..."
                            try {
                                val result = sdk!!.createUnsignedOfflineTransaction(
                                    senderPubkey = authorizedPubkey!!,
                                    nonceAuthorityPubkey = authorizedPubkey!!, // Same as sender for demo
                                    recipient = "11111111111111111111111111111111", // System program for demo
                                    amount = 1000000L // 0.001 SOL
                                )
                                
                                result.fold(
                                    onSuccess = { base64Tx ->
                                        unsignedTxBase64 = base64Tx
                                        statusMessage = "Unsigned transaction created! Ready to sign."
                                    },
                                    onFailure = { error ->
                                        errorMessage = error.message ?: "Unknown error"
                                        statusMessage = "Failed to create transaction"
                                    }
                                )
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Transaction creation failed"
                                statusMessage = "Failed to create transaction"
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && authorizedPubkey != null && sdk != null,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (isLoading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                    }
                    Text("Create Unsigned TX")
                }
                
                if (unsignedTxBase64 != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "✅ Unsigned TX: ${unsignedTxBase64!!.take(32)}...",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
            }
        }
        
        // Step 4: Sign with MWA
        Card(
            modifier = Modifier.fillMaxWidth()
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Step 4: Sign with MWA",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Sign transaction securely with your wallet. Private keys NEVER leave Seed Vault!",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(modifier = Modifier.height(12.dp))
                
                Button(
                    onClick = {
                        scope.launch {
                            isLoading = true
                            errorMessage = null
                            statusMessage = "Requesting signature from wallet..."
                            try {
                                val sender = activityResultSender
                                    ?: throw MwaException("ActivityResultSender not initialized")
                                
                                // Call actual MWA signing
                                val signedBytes = mwaClient!!.signAndSendTransaction(
                                    sender = sender,
                                    unsignedTransactionBase64 = unsignedTxBase64!!
                                )
                                
                                signedTxBase64 = android.util.Base64.encodeToString(
                                    signedBytes,
                                    android.util.Base64.NO_WRAP
                                )
                                statusMessage = "Transaction signed! Ready to submit."
                                
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Signing failed"
                                statusMessage = "Signing failed"
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && unsignedTxBase64 != null && activityResultSender != null,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (isLoading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                    }
                    Text("Sign Transaction")
                }
                
                if (signedTxBase64 != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "✅ Signed TX: ${signedTxBase64!!.take(32)}...",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
            }
        }
        
        // Step 5: Submit transaction
        Card(
            modifier = Modifier.fillMaxWidth()
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Step 5: Submit Transaction",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Submit signed transaction to Solana blockchain",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(modifier = Modifier.height(12.dp))
                
                Button(
                    onClick = {
                        scope.launch {
                            isLoading = true
                            errorMessage = null
                            statusMessage = "Submitting transaction to blockchain..."
                            try {
                                val result = sdk!!.submitOfflineTransaction(
                                    transactionBase64 = signedTxBase64!!,
                                    verifyNonce = true
                                )
                                
                                result.fold(
                                    onSuccess = { signature ->
                                        txSignature = signature
                                        statusMessage = "Transaction submitted successfully!"
                                    },
                                    onFailure = { error ->
                                        errorMessage = error.message ?: "Unknown error"
                                        statusMessage = "Submission failed"
                                    }
                                )
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Submission failed"
                                statusMessage = "Submission failed"
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && signedTxBase64 != null && sdk != null,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (isLoading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                    }
                    Text("Submit to Blockchain")
                }
                
                if (txSignature != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "✅ Signature: $txSignature",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
            }
        }
        
        // Reset button
        if (authorizedPubkey != null || unsignedTxBase64 != null) {
            OutlinedButton(
                onClick = {
                    authorizedPubkey = null
                    unsignedTxBase64 = null
                    signedTxBase64 = null
                    txSignature = null
                    statusMessage = "Reset complete. Connect wallet to begin."
                    errorMessage = null
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Reset Demo")
            }
        }
    }
}

