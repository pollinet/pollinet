package xyz.pollinet.android.ui

import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Build
import android.os.IBinder
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import kotlinx.coroutines.launch
import xyz.pollinet.android.mwa.PolliNetMwaClient
import xyz.pollinet.android.mwa.MwaException
import xyz.pollinet.sdk.BleService
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
    activityResultSender: ActivityResultSender,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    
    // MWA client state
    var mwaClient by remember { mutableStateOf<PolliNetMwaClient?>(null) }
    
    // UI state
    var authorizedPubkey by remember { mutableStateOf<String?>(null) }
    var statusMessage by remember { mutableStateOf("Ready. Connect wallet to begin.") }
    var isLoading by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    
    // Transaction state
    var unsignedTxBase64 by remember { mutableStateOf<String?>(null) }
    var signedTxBase64 by remember { mutableStateOf<String?>(null) }
    var txSignature by remember { mutableStateOf<String?>(null) }
    var cachedNonceCount by remember { mutableStateOf(0) }
    
    // BLE state
    var bleService by remember { mutableStateOf<BleService?>(null) }
    var isBound by remember { mutableStateOf(false) }
    var bleMode by remember { mutableStateOf<String?>(null) } // "advertise" or "scan"
    var bleActivityLog by remember { mutableStateOf<List<String>>(emptyList()) }
    var receivedTransactions by remember { mutableStateOf<List<String>>(emptyList()) }
    
    // Collect BLE service state
    val bleLogs by bleService?.logs?.collectAsStateWithLifecycle(emptyList()) ?: remember { mutableStateOf(emptyList()) }
    val isAdvertising by bleService?.isAdvertising?.collectAsStateWithLifecycle(false) ?: remember { mutableStateOf(false) }
    val isScanning by bleService?.isScanning?.collectAsStateWithLifecycle(false) ?: remember { mutableStateOf(false) }
    val connectionState by bleService?.connectionState?.collectAsStateWithLifecycle(BleService.ConnectionState.DISCONNECTED) 
        ?: remember { mutableStateOf(BleService.ConnectionState.DISCONNECTED) }
    
    // Service connection
    val serviceConnection = remember {
        object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
                val binder = service as? BleService.LocalBinder
                bleService = binder?.getService()
                isBound = true
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                bleService = null
                isBound = false
            }
        }
    }
    
    // Initialize MWA client and BLE service
    LaunchedEffect(Unit) {
        mwaClient = PolliNetMwaClient.create(
            context = context,
            identityUri = "https://pollinet.xyz",
            iconUri = "favicon.ico",  // Relative to assets
            identityName = "PolliNet"
        )
        
        // Start and bind BLE service
        val intent = Intent(context, BleService::class.java).apply {
            action = BleService.ACTION_START
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent)
        } else {
            context.startService(intent)
        }
        context.bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
    }
    
    DisposableEffect(Unit) {
        onDispose {
            if (isBound) {
                context.unbindService(serviceConnection)
            }
        }
    }
    
    // Helper function to add BLE activity log
    fun addBleLog(message: String) {
        val timestamp = java.text.SimpleDateFormat("HH:mm:ss", java.util.Locale.getDefault())
            .format(java.util.Date())
        bleActivityLog = bleActivityLog + "[$timestamp] $message"
        if (bleActivityLog.size > 50) {
            bleActivityLog = bleActivityLog.takeLast(50)
        }
    }
    
    // Note: We track cachedNonceCount manually since there's no SDK method to retrieve it
    // The count is updated after successful nonce creation in the "Prepare Bundle" flow
    
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
                                // Call actual MWA authorization
                                val pubkey = mwaClient!!.authorize(activityResultSender)
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
                    enabled = !isLoading && mwaClient != null && authorizedPubkey == null,
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
                
                // Show cached nonce status
                if (cachedNonceCount > 0) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.primaryContainer
                        )
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = "✓",
                                fontSize = 20.sp,
                                color = MaterialTheme.colorScheme.primary
                            )
                            Spacer(modifier = Modifier.width(8.dp))
                            Column {
                                Text(
                                    text = "$cachedNonceCount nonce account(s) cached",
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 14.sp,
                                    color = MaterialTheme.colorScheme.onPrimaryContainer
                                )
                                Text(
                                    text = "Ready for offline transactions",
                                    fontSize = 12.sp,
                                    color = MaterialTheme.colorScheme.onPrimaryContainer
                                )
                            }
                        }
                    }
                }
                
                Spacer(modifier = Modifier.height(12.dp))
                
                // Show two buttons: Create New or Refresh Existing
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    // Button to create new nonces (or refresh if needed)
                    Button(
                        onClick = {
                            scope.launch {
                                isLoading = true
                                errorMessage = null
                                
                                // Check if we should create or refresh
                                if (cachedNonceCount == 0) {
                                    statusMessage = "Creating nonce account transactions..."
                                } else {
                                    statusMessage = "Refreshing nonce accounts..."
                                }
                                
                                try {
                                    // Step 1: Create unsigned nonce transactions
                                    val nonceCount = if (cachedNonceCount == 0) 1 else cachedNonceCount
                                val unsignedNonceTxs = sdk!!.createUnsignedNonceTransactions(
                                    count = nonceCount,
                                    payerPubkey = authorizedPubkey!!
                                ).getOrThrow()
                                
                                statusMessage = "Created $nonceCount nonce transactions. Preparing for wallet signing..."
                                
                                // Step 2: Prepare transactions for MWA signing
                                // Each transaction needs both: nonce keypair signature + payer signature
                                // For now, we pass unsigned transactions to MWA which will handle signing
                                val transactionsToSign = unsignedNonceTxs
                                
                                statusMessage = "Sending ${transactionsToSign.size} transactions to wallet for signing..."
                                
                                // Step 3: Send to MWA for payer to co-sign
                                // Note: MWA's signAndSendTransactions expects transactions
                                // Since we have multiple transactions, we'll send them one by one
                                
                                val noncePublicKeys = mutableListOf<String>()
                                var successCount = 0
                                
                                for ((index, nonceTx) in transactionsToSign.withIndex()) {
                                    try {
                                        statusMessage = "Signing transaction ${index + 1}/${transactionsToSign.size} with wallet (payer)..."
                                        
                                        // Step 1: MWA signs with payer key (first signature)
                                        val payerSignedTxBytes = mwaClient!!.signTransaction(
                                            sender = activityResultSender,
                                            unsignedTxBase64 = nonceTx.unsignedTransactionBase64
                                        )
                                        
                                        android.util.Log.d("MwaTransactionDemo", "Payer signature added by wallet")
                                        statusMessage = "Adding nonce signature ${index + 1}/${transactionsToSign.size}..."
                                        
                                        // Step 2: Add nonce signature locally (second signature)
                                        val payerSignedBase64 = android.util.Base64.encodeToString(
                                            payerSignedTxBytes,
                                            android.util.Base64.NO_WRAP
                                        )
                                        
                                        val fullySignedBase64 = sdk.addNonceSignature(
                                            payerSignedTransactionBase64 = payerSignedBase64,
                                            nonceKeypairBase64 = nonceTx.nonceKeypairBase64
                                        ).getOrThrow()
                                        
                                        android.util.Log.d("MwaTransactionDemo", "Nonce signature added (fully signed)")
                                        statusMessage = "Submitting transaction ${index + 1}/${transactionsToSign.size} to Solana..."
                                        
                                        // Step 3: Submit fully-signed transaction to Solana
                                        val txSignature = sdk.submitOfflineTransaction(
                                            transactionBase64 = fullySignedBase64,
                                            verifyNonce = false  // Don't verify nonce for creation txs
                                        ).getOrThrow()
                                        
                                        // Transaction was submitted successfully
                                        noncePublicKeys.add(nonceTx.noncePubkey)
                                        successCount++
                                        
                                        android.util.Log.d("MwaTransactionDemo", "Transaction ${index + 1} submitted: $txSignature")
                                        statusMessage = "Transaction ${index + 1}/${transactionsToSign.size} confirmed! Sig: ${txSignature.take(8)}..."
                                        
                                        // Wait a bit between transactions
                                        kotlinx.coroutines.delay(1500)
                                        
                                    } catch (e: MwaException) {
                                        android.util.Log.e("MwaTransactionDemo", "Failed to sign transaction ${index + 1}: ${e.message}", e)
                                        errorMessage = "Transaction ${index + 1} failed: ${e.message}"
                                        // Continue with remaining transactions
                                    } catch (e: Exception) {
                                        android.util.Log.e("MwaTransactionDemo", "Failed to process transaction ${index + 1}", e)
                                        errorMessage = "Transaction ${index + 1} failed: ${e.message}"
                                        android.util.Log.e("MwaTransactionDemo", "Error details", e)
                                        // Continue with remaining transactions
                                    }
                                }
                                
                                if (successCount == 0) {
                                    throw Exception("Failed to create any nonce accounts. Check logs for details.")
                                }
                                
                                if (successCount < transactionsToSign.size) {
                                    android.util.Log.w("MwaTransactionDemo", "Only $successCount/${transactionsToSign.size} nonce accounts created successfully")
                                }
                                
                                statusMessage = "Caching $successCount nonce accounts..."
                                
                                // Step 4: Cache the nonce data for offline use
                                val newCachedCount = sdk.cacheNonceAccounts(noncePublicKeys).getOrThrow()
                                cachedNonceCount = newCachedCount  // Update state
                                
                                statusMessage = "✅ Successfully prepared offline bundle with $newCachedCount nonce accounts!"
                                errorMessage = null
                                
                            } catch (e: MwaException) {
                                errorMessage = "Wallet error: ${e.message}"
                                statusMessage = "Failed to prepare bundle"
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Bundle preparation failed"
                                statusMessage = "Failed to prepare bundle"
                                android.util.Log.e("MwaTransactionDemo", "Bundle preparation error", e)
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                        enabled = !isLoading && authorizedPubkey != null && sdk != null,
                        modifier = Modifier.weight(1f)
                    ) {
                        if (isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(20.dp),
                                strokeWidth = 2.dp
                            )
                            Spacer(modifier = Modifier.width(8.dp))
                        }
                        Text(if (cachedNonceCount == 0) "Create Nonces" else "Refresh Nonces")
                    }
                    
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
                                // Random recipient addresses
                                val recipientAddresses = listOf(
                                    "A7B9f6dy4Up29g8XMTM4H6i5hMzR2bwYeao3UtuiZLiz",
                                    "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX",
                                    "AtHGwWe2cZQ1WbsPVHFsCm4FqUDW8pcPLYXWsA89iuDE",
                                    "7WPGEvxRzZ3ARQNQU5iEdRevFX2oh1mG1xMq6A2NazBy",
                                    "CrqmbxGbiTuPZkxz18pSvsZ8JurxGp7cb2mxmdfi5erG",
                                    "Acau8iLY9Rv115UDzWPkDAopB6t9iFxGQuebZxffqoMv",
                                    "DyBFbB2VWG6rp4Y3KRYqLU5KenU6yX3hahcts9Gomwmj"
                                )
                                
                                // Random amount between 0.1 to 0.99 SOL (in lamports)
                                // 0.1 SOL = 100,000,000 lamports
                                // 0.99 SOL = 990,000,000 lamports
                                val minAmount = 100_000_000L // 0.1 SOL
                                val maxAmount = 990_000_000L // 0.99 SOL
                                val randomAmount = (minAmount..maxAmount).random()
                                
                                // Random recipient
                                val randomRecipient = recipientAddresses.random()
                                
                                statusMessage = "Creating transaction: ${randomAmount / 1_000_000_000.0} SOL to ${randomRecipient.take(8)}..."
                                
                                val result = sdk!!.createUnsignedOfflineTransaction(
                                    senderPubkey = authorizedPubkey!!,
                                    nonceAuthorityPubkey = authorizedPubkey!!, // Same as sender for demo
                                    recipient = randomRecipient,
                                    amount = randomAmount
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
                                // Call signTransaction (NOT signAndSendTransaction)
                                // This signs the transaction and returns the signed bytes
                                val signedBytes = mwaClient!!.signTransaction(
                                    sender = activityResultSender,
                                    unsignedTxBase64 = unsignedTxBase64!!
                                )
                                
                                signedTxBase64 = android.util.Base64.encodeToString(
                                    signedBytes,
                                    android.util.Base64.NO_WRAP
                                )
                                statusMessage = "Transaction signed! Ready to submit."
                                
                            } catch (e: Exception) {
                                errorMessage = e.message ?: "Signing failed"
                                statusMessage = "Signing failed"
                                android.util.Log.e("MwaTransactionDemo", "Signing error", e)
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && unsignedTxBase64 != null,
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
        
        // Step 4b: BLE Transmission (Optional Alternative to Direct Submit)
        if (signedTxBase64 != null) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.tertiaryContainer
                )
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        text = "Step 4b: Send via BLE (Optional)",
                        fontWeight = FontWeight.Bold,
                        fontSize = 16.sp,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Compress, fragment, and transmit signed transaction over BLE mesh",
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    
                    // Mode selection
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Button(
                            onClick = {
                                scope.launch {
                                    addBleLog("Starting advertising mode...")
                                    bleMode = "advertise"
                                    bleService?.startAdvertising()
                                    addBleLog("Advertising started. Waiting for connection...")
                                }
                            },
                            enabled = bleMode == null && !isAdvertising,
                            modifier = Modifier.weight(1f),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.tertiary
                            )
                        ) {
                            Text("Advertise")
                        }
                        
                        Button(
                            onClick = {
                                scope.launch {
                                    addBleLog("Starting scanning mode...")
                                    bleMode = "scan"
                                    bleService?.startScanning()
                                    addBleLog("Scanning for devices...")
                                }
                            },
                            enabled = bleMode == null && !isScanning,
                            modifier = Modifier.weight(1f),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.tertiary
                            )
                        ) {
                            Text("Scan")
                        }
                    }
                    
                    // Connection status
                    if (bleMode != null) {
                        Spacer(modifier = Modifier.height(8.dp))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = "Mode: ${bleMode?.uppercase()}",
                                fontSize = 14.sp,
                                color = MaterialTheme.colorScheme.onTertiaryContainer
                            )
                            Text(
                                text = "Status: ${connectionState.name}",
                                fontSize = 14.sp,
                                color = if (connectionState == BleService.ConnectionState.CONNECTED) 
                                    MaterialTheme.colorScheme.tertiary 
                                else MaterialTheme.colorScheme.onTertiaryContainer
                            )
                        }
                    }
                    
                    // Send transaction button (only available when connected)
                    if (bleMode != null && connectionState == BleService.ConnectionState.CONNECTED) {
                        Spacer(modifier = Modifier.height(12.dp))
                        Button(
                            onClick = {
                                scope.launch {
                                    addBleLog("Fragmenting signed transaction...")
                                    try {
                                        val txBytes = android.util.Base64.decode(signedTxBase64, android.util.Base64.NO_WRAP)
                                        addBleLog("Transaction size: ${txBytes.size} bytes")
                                        
                                        // Queue the signed transaction with MTU-aware fragmentation
                                        bleService?.queueSignedTransaction(txBytes)?.onSuccess { fragmentCount ->
                                            addBleLog("✅ Fragmented into $fragmentCount MTU-optimized fragments")
                                            addBleLog("Transmitting fragments over BLE mesh...")
                                            addBleLog("✅ Fragments queued and sending (supports auto re-fragmentation)")
                                        }?.onFailure { error ->
                                            addBleLog("❌ Failed to queue transaction: ${error.message}")
                                        }
                                    } catch (e: Exception) {
                                        addBleLog("❌ Error: ${e.message}")
                                    }
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.tertiary
                            )
                        ) {
                            Text("Send Transaction via BLE")
                        }
                    }
                    
                    // Stop button
                    if (bleMode != null) {
                        Spacer(modifier = Modifier.height(8.dp))
                        OutlinedButton(
                            onClick = {
                                scope.launch {
                                    if (isAdvertising) {
                                        bleService?.stopAdvertising()
                                        addBleLog("Stopped advertising")
                                    }
                                    if (isScanning) {
                                        bleService?.stopScanning()
                                        addBleLog("Stopped scanning")
                                    }
                                    bleMode = null
                                }
                            },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text("Stop BLE")
                        }
                    }
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
        
        // BLE Receiver Section - Shows received transactions
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer
            )
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "📨 BLE Receiver",
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp,
                    color = MaterialTheme.colorScheme.onSecondaryContainer
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Received transactions will appear here. Reconstruct and submit to blockchain.",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSecondaryContainer
                )
                Spacer(modifier = Modifier.height(12.dp))
                
                // TODO: Monitor incoming fragments and reconstruct transactions
                // This would require extending BleService to expose reconstructed transactions
                if (receivedTransactions.isEmpty()) {
                    Text(
                        text = "No transactions received yet. Scan for devices to receive.",
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                        fontFamily = FontFamily.Monospace
                    )
                } else {
                    Text(
                        text = "Received ${receivedTransactions.size} transaction(s):",
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSecondaryContainer
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    
                    receivedTransactions.forEachIndexed { index, tx ->
                        Card(
                            modifier = Modifier.fillMaxWidth(),
                            colors = CardDefaults.cardColors(
                                containerColor = MaterialTheme.colorScheme.surface
                            )
                        ) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text(
                                    text = "Transaction ${index + 1}",
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 14.sp
                                )
                                Spacer(modifier = Modifier.height(4.dp))
                                Text(
                                    text = "${tx.take(64)}...",
                                    fontSize = 12.sp,
                                    fontFamily = FontFamily.Monospace
                                )
                                Spacer(modifier = Modifier.height(8.dp))
                                
                                Button(
                                    onClick = {
                                        scope.launch {
                                            isLoading = true
                                            statusMessage = "Submitting received transaction ${index + 1}..."
                                            try {
                                                val result = sdk?.submitOfflineTransaction(
                                                    transactionBase64 = tx,
                                                    verifyNonce = true
                                                )
                                                result?.fold(
                                                    onSuccess = { signature ->
                                                        addBleLog("✅ Transaction ${index + 1} submitted: ${signature.take(8)}...")
                                                        statusMessage = "Transaction ${index + 1} submitted successfully!"
                                                        // Remove from list after successful submission
                                                        receivedTransactions = receivedTransactions.filterIndexed { i, _ -> i != index }
                                                    },
                                                    onFailure = { error ->
                                                        addBleLog("❌ Failed to submit transaction ${index + 1}: ${error.message}")
                                                        errorMessage = "Submission failed: ${error.message}"
                                                    }
                                                )
                                            } catch (e: Exception) {
                                                addBleLog("❌ Error submitting transaction ${index + 1}: ${e.message}")
                                                errorMessage = "Submission error: ${e.message}"
                                            } finally {
                                                isLoading = false
                                            }
                                        }
                                    },
                                    enabled = !isLoading && sdk != null,
                                    modifier = Modifier.fillMaxWidth()
                                ) {
                                    Text("Submit to Blockchain")
                                }
                            }
                        }
                        Spacer(modifier = Modifier.height(8.dp))
                    }
                }
            }
        }
        
        // BLE Activity Log
        if (bleActivityLog.isNotEmpty() || bleLogs.isNotEmpty()) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                )
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "📋 Activity Log",
                            fontWeight = FontWeight.Bold,
                            fontSize = 16.sp
                        )
                        OutlinedButton(
                            onClick = {
                                bleActivityLog = emptyList()
                                bleService?.clearLogs()
                            },
                            modifier = Modifier.height(32.dp)
                        ) {
                            Text("Clear", fontSize = 12.sp)
                        }
                    }
                    Spacer(modifier = Modifier.height(8.dp))
                    
                    val logScrollState = rememberScrollState()
                    LaunchedEffect(bleActivityLog.size, bleLogs.size) {
                        logScrollState.scrollTo(logScrollState.maxValue)
                    }
                    
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(max = 200.dp)
                            .verticalScroll(logScrollState)
                    ) {
                        // Show custom BLE activity log
                        bleActivityLog.forEach { log ->
                            Text(
                                text = log,
                                fontSize = 12.sp,
                                fontFamily = FontFamily.Monospace,
                                color = when {
                                    log.contains("❌") -> MaterialTheme.colorScheme.error
                                    log.contains("✅") -> MaterialTheme.colorScheme.primary
                                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                                }
                            )
                        }
                        
                        // Also show BLE service logs
                        bleLogs.forEach { log ->
                            Text(
                                text = log,
                                fontSize = 12.sp,
                                fontFamily = FontFamily.Monospace,
                                color = when {
                                    log.contains("❌") -> MaterialTheme.colorScheme.error
                                    log.contains("✅") -> MaterialTheme.colorScheme.primary
                                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                                }
                            )
                        }
                    }
                }
            }
        }
        
        // Reset button
        if (authorizedPubkey != null || unsignedTxBase64 != null || bleMode != null) {
            OutlinedButton(
                onClick = {
                    authorizedPubkey = null
                    unsignedTxBase64 = null
                    signedTxBase64 = null
                    txSignature = null
                    cachedNonceCount = 0
                    bleMode = null
                    bleActivityLog = emptyList()
                    receivedTransactions = emptyList()
                    statusMessage = "Reset complete. Connect wallet to begin."
                    errorMessage = null
                    
                    // Stop BLE if active
                    if (isAdvertising) bleService?.stopAdvertising()
                    if (isScanning) bleService?.stopScanning()
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Reset Demo")
            }
        }
    }
}

