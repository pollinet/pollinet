package xyz.pollinet.android.ui

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.IBinder
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import kotlinx.coroutines.launch
import xyz.pollinet.android.mwa.PolliNetMwaClient
import xyz.pollinet.android.mwa.MwaException
import xyz.pollinet.sdk.BleService
import xyz.pollinet.sdk.CreateUnsignedTransactionRequest
import xyz.pollinet.sdk.MetricsSnapshot
import xyz.pollinet.sdk.PolliNetSDK
import xyz.pollinet.sdk.SdkConfig
import xyz.pollinet.sdk.UnsignedNonceTransaction

@Composable
fun DiagnosticsScreen(
    mwaActivityResultSender: ActivityResultSender,
    mainSdk: PolliNetSDK?  // SDK with RPC from MainActivity
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    
    var bleService by remember { mutableStateOf<BleService?>(null) }
    var isBound by remember { mutableStateOf(false) }
    
    val connectionState by bleService?.connectionState?.collectAsStateWithLifecycle(BleService.ConnectionState.DISCONNECTED) 
        ?: remember { mutableStateOf(BleService.ConnectionState.DISCONNECTED) }
    
    val metrics by bleService?.metrics?.collectAsStateWithLifecycle(null) 
        ?: remember { mutableStateOf(null) }

    val advertisingState = bleService?.isAdvertising?.collectAsStateWithLifecycle(false)
    val isAdvertising = advertisingState?.value ?: false

    val scanningState = bleService?.isScanning?.collectAsStateWithLifecycle(false)
    val isScanning = scanningState?.value ?: false

    val logsState = bleService?.logs?.collectAsStateWithLifecycle(emptyList())
    val bleLogs = logsState?.value ?: emptyList()
    
    var permissionsGranted by remember { mutableStateOf(false) }
    var sdkVersion by remember { mutableStateOf("Unknown") }
    var testLogs by remember { mutableStateOf(listOf<String>()) }
    var isTestingSdk by remember { mutableStateOf(false) }
    
    fun addLog(message: String) {
        val timestamp = java.text.SimpleDateFormat("HH:mm:ss", java.util.Locale.getDefault())
            .format(java.util.Date())
        testLogs = testLogs + "[$timestamp] $message"
        if (testLogs.size > 20) {
            testLogs = testLogs.takeLast(20)
        }
    }

    // Service connection
    val serviceConnection = remember {
        object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
                val binder = service as? BleService.LocalBinder
                bleService = binder?.getService()
                isBound = true
                
                // Initialize SDK
                scope.launch {
                    bleService?.initializeSdk(
                        SdkConfig(
                            // Use Helius devnet RPC for BLE service operations
                            rpcUrl = "https://devnet.helius-rpc.com/?api-key=ce433fae-db6e-4cec-8eb4-38ffd30658c0",
                            enableLogging = true,
                            logLevel = "info",
                            storageDirectory = context.filesDir.absolutePath
                        )
                    )?.onSuccess {
                        addLog("✅ BLE Service SDK initialized successfully")
                    }?.onFailure { e ->
                        addLog("❌ Failed to initialize BLE Service SDK: ${e.message}")
                    }
                }
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                bleService = null
                isBound = false
            }
        }
    }

    // Permission launcher
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        permissionsGranted = permissions.all { it.value }
        if (permissionsGranted) {
            // Start and bind service
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
    }

    // Check permissions on launch
    LaunchedEffect(Unit) {
        try {
            sdkVersion = PolliNetSDK.version()
            addLog("✓ FFI initialized, version: $sdkVersion")
        } catch (e: Exception) {
            sdkVersion = "Error: ${e.message}"
            addLog("✗ FFI initialization failed: ${e.message}")
        }
        
        val permissions = buildList {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                add(Manifest.permission.BLUETOOTH_SCAN)
                add(Manifest.permission.BLUETOOTH_CONNECT)
                add(Manifest.permission.BLUETOOTH_ADVERTISE)
            } else {
                add(Manifest.permission.BLUETOOTH)
                add(Manifest.permission.BLUETOOTH_ADMIN)
                add(Manifest.permission.ACCESS_FINE_LOCATION)
            }
        }
        
        permissionsGranted = permissions.all { 
            ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
        }
        
        if (!permissionsGranted) {
            permissionLauncher.launch(permissions.toTypedArray())
        } else if (!isBound) {
            // Start and bind service
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
    }

    DisposableEffect(Unit) {
        onDispose {
            if (isBound) {
                context.unbindService(serviceConnection)
            }
        }
    }

    // UI
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Text(
            text = "PolliNet Diagnostics",
            style = MaterialTheme.typography.headlineMedium
        )
        
        Text(
            text = "SDK Version: $sdkVersion",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        HorizontalDivider()

        // Connection Status
        StatusCard(
            title = "Connection Status",
            content = {
                ConnectionStatusContent(
                    connectionState = connectionState,
                    permissionsGranted = permissionsGranted
                )
            }
        )

        // Controls
        StatusCard(
            title = "Controls",
            content = {
                ControlButtons(
                    bleService = bleService,
                    permissionsGranted = permissionsGranted,
                    connectionState = connectionState
                )
            }
        )

        StatusCard(
            title = "BLE Mesh Manual Test",
            content = {
                BleMeshManualTestContent(
                    bleService = bleService,
                    connectionState = connectionState,
                    isAdvertising = isAdvertising,
                    isScanning = isScanning,
                    logs = bleLogs
                )
            }
        )

        // FFI Tests
        StatusCard(
            title = "FFI Tests",
            content = {
                FFITestButtons(
                    context = context,
                    bleService = bleService,
                    isTestingSdk = isTestingSdk,
                    onTestStart = { isTestingSdk = true },
                    onTestComplete = { isTestingSdk = false },
                    onLog = { addLog(it) },
                    scope = scope
                )
            }
        )

        // MWA (Mobile Wallet Adapter) Demo
        StatusCard(
            title = "🔐 MWA Transaction Demo",
            content = {
                MwaTransactionDemo(
                    sdk = mainSdk,  // Use SDK with RPC for nonce creation
                    activityResultSender = mwaActivityResultSender
                )
            }
        )

        // SPL Token, Vote & Nonce Account Tests
        StatusCard(
            title = "🧪 SPL Token, Vote & Nonce Tests",
            content = {
                SplVoteNonceTestContent(
                    sdk = mainSdk,
                    bleService = bleService,
                    mwaActivityResultSender = mwaActivityResultSender,
                    onLog = { addLog(it) },
                    scope = scope
                )
            }
        )

        // Metrics
        StatusCard(
            title = "Metrics",
            content = {
                MetricsContent(metrics = metrics)
            }
        )
        
        // Test Logs
        StatusCard(
            title = "Test Logs",
            content = {
                TestLogsContent(logs = testLogs)
            }
        )
    }
}

@Composable
private fun BleMeshManualTestContent(
    bleService: BleService?,
    connectionState: BleService.ConnectionState,
    isAdvertising: Boolean,
    isScanning: Boolean,
    logs: List<String>
) {
    val scope = rememberCoroutineScope()
    var customTransaction by rememberSaveable { mutableStateOf("") }
    val logScrollState = rememberScrollState()

    LaunchedEffect(logs) {
        if (logs.isNotEmpty()) {
            logScrollState.scrollTo(logScrollState.maxValue)
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            text = "Run this on two devices: one advertises, the other scans. Once connected, queue a transaction to push fragments over GATT.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        StatusRow(
            label = "Connection",
            value = connectionState.name,
            isGood = connectionState == BleService.ConnectionState.CONNECTED
        )
        StatusRow(
            label = "Advertising",
            value = if (isAdvertising) "ON" else "OFF",
            isGood = isAdvertising
        )
        StatusRow(
            label = "Scanning",
            value = if (isScanning) "ON" else "OFF",
            isGood = isScanning
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { bleService?.startAdvertising() },
                enabled = bleService != null && !isAdvertising,
                modifier = Modifier.weight(1f)
            ) {
                Text("Start Advertising")
            }
            Button(
                onClick = { bleService?.stopAdvertising() },
                enabled = bleService != null && isAdvertising,
                modifier = Modifier.weight(1f)
            ) {
                Text("Stop Advertising")
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { bleService?.startScanning() },
                enabled = bleService != null && !isScanning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Start Scanning")
            }
            Button(
                onClick = { bleService?.stopScanning() },
                enabled = bleService != null && isScanning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Stop Scanning")
            }
        }

        OutlinedTextField(
            value = customTransaction,
            onValueChange = { customTransaction = it },
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 100.dp),
            label = { Text("Base64 Transaction (optional)") },
            placeholder = { Text("Paste a signed transaction in base64") },
            minLines = 3,
            maxLines = 6
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { bleService?.queueSampleTransaction() },
                enabled = bleService != null,
                modifier = Modifier.weight(1f)
            ) {
                Text("Queue Sample (1 KB)")
            }
            Button(
                onClick = { bleService?.queueSampleTransaction(byteSize = 2048) },
                enabled = bleService != null,
                modifier = Modifier.weight(1f)
            ) {
                Text("Queue Sample (2 KB)")
            }
        }

        Button(
            onClick = { bleService?.queueTransactionFromBase64(customTransaction) },
            enabled = bleService != null && customTransaction.isNotBlank(),
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("Queue Base64 Transaction")
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { bleService?.debugQueueStatus() },
                enabled = bleService != null,
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.secondary
                )
            ) {
                Text("Debug Queue")
            }
            OutlinedButton(
                onClick = { bleService?.clearLogs() },
                enabled = bleService != null && logs.isNotEmpty(),
                modifier = Modifier.weight(1f)
            ) {
                Text("Clear Logs")
            }
        }
        
        // Test Controls for Autonomous Relay
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { 
                    scope.launch {
                        bleService?.sdk?.getReceivedQueueSize()?.onSuccess { size ->
                            println("📊 Received Queue Size: $size")
                        }
                    }
                },
                enabled = bleService?.sdk != null,
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.tertiary
                )
            ) {
                Text("Check RX Queue", style = MaterialTheme.typography.labelSmall)
            }
            
            OutlinedButton(
                onClick = { 
                    scope.launch {
                        bleService?.sdk?.cleanupOldSubmissions()
                    }
                },
                enabled = bleService?.sdk != null,
                modifier = Modifier.weight(1f)
            ) {
                Text("Cleanup", style = MaterialTheme.typography.labelSmall)
            }
        }

        HorizontalDivider()

        Text(
            text = "Mesh Logs",
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary
        )

        if (logs.isEmpty()) {
            Text(
                text = "Logs will appear here once fragments flow through the connection.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        } else {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 220.dp)
                    .verticalScroll(logScrollState),
                verticalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                logs.forEach { log ->
                    Text(
                        text = log,
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
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

@Composable
private fun StatusCard(
    title: String,
    content: @Composable () -> Unit
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant
        )
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )
            content()
        }
    }
}

@Composable
private fun ConnectionStatusContent(
    connectionState: BleService.ConnectionState,
    permissionsGranted: Boolean
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        StatusRow(
            label = "Permissions",
            value = if (permissionsGranted) "✓ Granted" else "✗ Not granted",
            isGood = permissionsGranted
        )
        
        StatusRow(
            label = "BLE State",
            value = connectionState.name,
            isGood = connectionState == BleService.ConnectionState.CONNECTED
        )
    }
}

@Composable
private fun ControlButtons(
    bleService: BleService?,
    permissionsGranted: Boolean,
    connectionState: BleService.ConnectionState
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { bleService?.startScanning() },
                enabled = permissionsGranted && connectionState != BleService.ConnectionState.CONNECTED,
                modifier = Modifier.weight(1f)
            ) {
                Text("Start Scan")
            }
            
            Button(
                onClick = { bleService?.stopScanning() },
                enabled = permissionsGranted,
                modifier = Modifier.weight(1f)
            ) {
                Text("Stop Scan")
            }
        }
        
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { bleService?.startAdvertising() },
                enabled = permissionsGranted,
                modifier = Modifier.weight(1f)
            ) {
                Text("Start Advertise")
            }
            
            Button(
                onClick = { bleService?.stopAdvertising() },
                enabled = permissionsGranted,
                modifier = Modifier.weight(1f)
            ) {
                Text("Stop Advertise")
            }
        }
    }
}

@Composable
private fun MetricsContent(metrics: MetricsSnapshot?) {
    if (metrics == null) {
        Text(
            text = "No metrics available",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    } else {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            StatusRow("Fragments Buffered", metrics.fragmentsBuffered.toString())
            StatusRow("Transactions Complete", metrics.transactionsComplete.toString())
            StatusRow("Reassembly Failures", metrics.reassemblyFailures.toString())
            if (metrics.lastError.isNotEmpty()) {
                StatusRow(
                    "Last Error", 
                    metrics.lastError,
                    isGood = false
                )
            }
            StatusRow(
                "Updated",
                java.text.SimpleDateFormat.getDateTimeInstance().format(
                    java.util.Date(metrics.updatedAt * 1000)
                )
            )
        }
    }
}

@Composable
private fun FFITestButtons(
    context: android.content.Context,
    bleService: BleService?,
    isTestingSdk: Boolean,
    onTestStart: () -> Unit,
    onTestComplete: () -> Unit,
    onLog: (String) -> Unit,
    scope: kotlinx.coroutines.CoroutineScope
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(
            onClick = {
                onTestStart()
                scope.launch {
                    try {
                        onLog("Testing SDK initialization...")
                        val config = SdkConfig(
                            rpcUrl = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6",
                            enableLogging = true,
                            logLevel = "debug",
                            storageDirectory = context.filesDir.absolutePath
                        )
                        
                        val result = PolliNetSDK.initialize(config)
                        result.onSuccess { sdk ->
                            onLog("✓ SDK initialized successfully")
                            
                            // Test metrics
                            onLog("Testing metrics...")
                            sdk.metrics().onSuccess { metrics ->
                                onLog("✓ Metrics retrieved:")
                                onLog("  Fragments: ${metrics.fragmentsBuffered}")
                                onLog("  Completed: ${metrics.transactionsComplete}")
                            }.onFailure {
                                onLog("✗ Metrics failed: ${it.message}")
                            }
                            
                            sdk.shutdown()
                        }.onFailure {
                            onLog("✗ SDK init failed: ${it.message}")
                        }
                    } catch (e: Exception) {
                        onLog("✗ Test exception: ${e.message}")
                    } finally {
                        onTestComplete()
                    }
                }
            },
            enabled = !isTestingSdk,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(if (isTestingSdk) "Testing..." else "Test SDK Init")
        }
        
        Button(
            onClick = {
                onTestStart()
                scope.launch {
                    try {
                        onLog("Testing transaction builder...")
                        val config = SdkConfig(
                            rpcUrl = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6",
                            storageDirectory = context.filesDir.absolutePath
                        )
                        val result = PolliNetSDK.initialize(config)
                        
                        result.onSuccess { sdk ->
                            // Example: Using nonceAccount (fetches from blockchain)
                            // For better performance, you can use nonceData instead (no RPC call)
                            val txResult = sdk.createUnsignedTransaction(
                                    sender = "2JnzJqwqLvrLZBAsu58jJtrMn1mT38Be3tcJBigmkTZq",
                                    recipient = "AtHGwWe2cZQ1WbsPVHFsCm4FqUDW8pcPLYXWsA89iuDE",
                                    feePayer = "2JnzJqwqLvrLZBAsu58jJtrMn1mT38Be3tcJBigmkTZq",
                                    amount = 1000000,
                                    nonceAccount = "2JnzJqwqLvrLZBAsu58jJtrMn1mT38Be3tcJBigmkTZq"
                                // Alternative: nonceData = cachedNonceData (no RPC call needed)
                            )
                            
                            txResult.onSuccess { txBase64 ->
                                onLog("✓ Transaction created:")
                                onLog("  ${txBase64.take(60)}...")
                                onLog("  Length: ${txBase64.length} chars")
                            }.onFailure {
                                onLog("✗ Transaction failed: ${it.message}")
                            }
                            
                            sdk.shutdown()
                        }
                    } catch (e: Exception) {
                        onLog("✗ Test exception: ${e.message}")
                    } finally {
                        onTestComplete()
                    }
                }
            },
            enabled = !isTestingSdk,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("Test Transaction Builder")
        }
        
        Button(
            onClick = {
                onLog("Testing BLE transport...")
                bleService?.let { service ->
                    scope.launch {
                        try {
                            // Test pushing inbound data
                            val testData = byteArrayOf(0x01, 0x02, 0x03, 0x04)
                            service.pushInboundData(testData)
                            onLog("✓ Pushed test data to transport")
                            
                            // Check metrics
                            kotlinx.coroutines.delay(100)
                            service.sdk?.metrics()?.onSuccess { metrics ->
                                onLog("  Fragments buffered: ${metrics.fragmentsBuffered}")
                            }
                        } catch (e: Exception) {
                            onLog("✗ BLE transport test failed: ${e.message}")
                        }
                    }
                } ?: onLog("✗ BLE service not available")
            },
            enabled = bleService != null,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("Test BLE Transport")
        }
    }
}

@Composable
private fun TestLogsContent(logs: List<String>) {
    if (logs.isEmpty()) {
        Text(
            text = "No test logs yet. Run tests above.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    } else {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = 200.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(2.dp)
        ) {
            logs.forEach { log ->
                Text(
                    text = log,
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    color = when {
                        log.contains("✓") -> MaterialTheme.colorScheme.primary
                        log.contains("✗") -> MaterialTheme.colorScheme.error
                        else -> MaterialTheme.colorScheme.onSurfaceVariant
                    }
                )
            }
        }
    }
}

@Composable
private fun StatusRow(
    label: String,
    value: String,
    isGood: Boolean? = null
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween
    ) {
        Text(
            text = "$label:",
            style = MaterialTheme.typography.bodyMedium
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            color = when (isGood) {
                true -> MaterialTheme.colorScheme.primary
                false -> MaterialTheme.colorScheme.error
                null -> MaterialTheme.colorScheme.onSurface
            }
        )
    }
}

@Composable
private fun SplVoteNonceTestContent(
    sdk: PolliNetSDK?,
    bleService: BleService?,
    mwaActivityResultSender: ActivityResultSender,
    onLog: (String) -> Unit,
    scope: kotlinx.coroutines.CoroutineScope
) {
    val context = LocalContext.current
    var mwaClient by remember { mutableStateOf<PolliNetMwaClient?>(null) }
    var authorizedPubkey by remember { mutableStateOf<String?>(null) }
    
    // Initialize MWA client
    LaunchedEffect(Unit) {
        try {
            mwaClient = PolliNetMwaClient.create(
                context = context,
                identityUri = "https://pollinet.xyz",
                iconUri = "favicon.ico",
                identityName = "PolliNet Diagnostics"
            )
        } catch (e: Exception) {
            onLog("⚠️ MWA client init failed: ${e.message}")
        }
    }
    var splSenderWallet by rememberSaveable { mutableStateOf("") }
    var splRecipientWallet by rememberSaveable { mutableStateOf("") }
    var splMintAddress by rememberSaveable { mutableStateOf("So11111111111111111111111111111111111111112") } // SOL mint for devnet
    var splAmount by rememberSaveable { mutableStateOf("1000000") } // 0.001 tokens (6 decimals)
    var splFeePayer by rememberSaveable { mutableStateOf("") }
    var createdSplTransaction by remember { mutableStateOf<String?>(null) }
    var isSigningSpl by remember { mutableStateOf(false) }
    
    var voteVoter by rememberSaveable { mutableStateOf("") }
    var voteProposalId by rememberSaveable { mutableStateOf("") }
    var voteAccount by rememberSaveable { mutableStateOf("") }
    var voteChoice by rememberSaveable { mutableStateOf("0") }
    var voteFeePayer by rememberSaveable { mutableStateOf("") }
    var voteNonceAccount by rememberSaveable { mutableStateOf("") }
    
    var nonceCount by rememberSaveable { mutableStateOf("3") }
    var noncePayerPubkey by rememberSaveable { mutableStateOf("") }
    var createdNonceTransactions by remember { mutableStateOf<List<UnsignedNonceTransaction>?>(null) }
    
    var isTesting by remember { mutableStateOf(false) }
    var isAuthorizing by remember { mutableStateOf(false) }
    
    // Nonce picking state (declared early so it can be used in transaction creation)
    var pickedNonce by remember { mutableStateOf<xyz.pollinet.sdk.CachedNonceData?>(null) }
    var isPickingNonce by remember { mutableStateOf(false) }

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            text = "Test SPL Token Transfer (Offline)",
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary
        )
        
        Text(
            text = "⚠️ Requires offline bundle. Prepare bundle first using MWA Demo above.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 4.dp)
        )
        
        OutlinedTextField(
            value = splSenderWallet,
            onValueChange = { splSenderWallet = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Sender Wallet (base58)") },
            placeholder = { Text("Enter sender wallet pubkey") },
            enabled = !isTesting && sdk != null
        )
        
        OutlinedTextField(
            value = splRecipientWallet,
            onValueChange = { splRecipientWallet = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Recipient Wallet (base58)") },
            placeholder = { Text("Enter recipient wallet pubkey") },
            enabled = !isTesting && sdk != null
        )
        
        OutlinedTextField(
            value = splMintAddress,
            onValueChange = { splMintAddress = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Mint Address (base58)") },
            placeholder = { Text("Token mint address") },
            enabled = !isTesting && sdk != null
        )
        
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            OutlinedTextField(
                value = splAmount,
                onValueChange = { splAmount = it },
                modifier = Modifier.weight(1f),
                label = { Text("Amount") },
                placeholder = { Text("1000000") },
                enabled = !isTesting && sdk != null
            )
            
            OutlinedTextField(
                value = splFeePayer,
                onValueChange = { splFeePayer = it },
                modifier = Modifier.weight(1f),
                label = { Text("Fee Payer") },
                placeholder = { Text("Fee payer pubkey") },
                enabled = !isTesting && sdk != null
            )
        }
        
        Button(
            onClick = {
                if (sdk == null) {
                    onLog("❌ SDK not initialized")
                    return@Button
                }
                isTesting = true
                scope.launch {
                    try {
                        onLog("🪙 Testing offline SPL token transfer...")
                        val amountLong = splAmount.toLongOrNull() ?: 0L
                        if (amountLong <= 0) {
                            onLog("❌ Invalid amount")
                            return@launch
                        }
                        
                        // Optionally use picked nonce data if available
                        val nonceData = pickedNonce?.let { nonce ->
                            xyz.pollinet.sdk.CachedNonceData(
                                version = 1,
                                nonceAccount = nonce.nonceAccount,
                                authority = nonce.authority,
                                blockhash = nonce.blockhash,
                                lamportsPerSignature = nonce.lamportsPerSignature,
                                cachedAt = nonce.cachedAt,
                                used = nonce.used
                            )
                        }
                        
                        val result = sdk.createUnsignedOfflineSplTransaction(
                            senderWallet = splSenderWallet,
                            recipientWallet = splRecipientWallet,
                            mintAddress = splMintAddress,
                            amount = amountLong,
                            feePayer = splFeePayer.ifEmpty { splSenderWallet },
                            nonceData = nonceData
                        )
                        
                        if (nonceData != null) {
                            onLog("📌 Using picked nonce account: ${nonceData.nonceAccount.take(16)}...")
                        } else {
                            onLog("📌 Auto-picking nonce from bundle...")
                        }
                        
                        result.onSuccess { txBase64 ->
                            createdSplTransaction = txBase64
                            onLog("✅ SPL token transaction created!")
                            onLog("   Length: ${txBase64.length} chars")
                            onLog("   Preview: ${txBase64.take(80)}...")
                            onLog("   Ready for MWA signing")
                        }.onFailure { e ->
                            onLog("❌ SPL transaction failed: ${e.message}")
                            createdSplTransaction = null
                        }
                    } catch (e: Exception) {
                        onLog("❌ Exception: ${e.message}")
                        createdSplTransaction = null
                    } finally {
                        isTesting = false
                    }
                }
            },
            enabled = !isTesting && sdk != null && splSenderWallet.isNotBlank() && splRecipientWallet.isNotBlank() && createdSplTransaction == null,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(if (isTesting) "Creating..." else "Create Offline SPL Transaction")
        }
        
        // Sign with MWA button (shown after transaction is created)
        if (createdSplTransaction != null && mwaClient != null) {
            Button(
                onClick = {
                    if (mwaClient == null) {
                        onLog("❌ MWA client not initialized")
                        return@Button
                    }
                    if (createdSplTransaction == null) {
                        onLog("❌ No transaction to sign")
                        return@Button
                    }
                    isSigningSpl = true
                    scope.launch {
                        try {
                            onLog("✍️ Signing SPL transaction with MWA...")
                            val signedTxBytes = mwaClient!!.signTransaction(
                                mwaActivityResultSender,
                                createdSplTransaction!!
                            )
                            val signedTxBase64 = android.util.Base64.encodeToString(
                                signedTxBytes,
                                android.util.Base64.NO_WRAP
                            )
                            onLog("✅ Transaction signed successfully!")
                            onLog("   Signature preview: ${signedTxBase64.take(80)}...")
                            
                            // Automatically add to queue using the better queueSignedTransaction method
                            if (bleService != null) {
                                onLog("📤 Adding signed transaction to BLE queue...")
                                scope.launch {
                                    try {
                                        val txBytes = android.util.Base64.decode(signedTxBase64, android.util.Base64.NO_WRAP)
                                        val queueResult = bleService.queueSignedTransaction(txBytes)
                                        queueResult.fold(
                                            onSuccess = { fragmentCount ->
                                                onLog("✅ Transaction added to queue!")
                                                onLog("   Fragmented into $fragmentCount fragments")
                                                onLog("   Ready for mesh transmission")
                                                onLog("   Sending loop will start when BLE connection is established")
                                                createdSplTransaction = null // Reset after queuing
                                            },
                                            onFailure = { error ->
                                                onLog("❌ Failed to queue transaction: ${error.message}")
                                                onLog("   Falling back to queueTransactionFromBase64...")
                                                // Fallback to older method
                                                bleService.queueTransactionFromBase64(signedTxBase64)
                                                onLog("✅ Transaction queued (fallback method)")
                                                createdSplTransaction = null
                                            }
                                        )
                                    } catch (e: Exception) {
                                        onLog("❌ Exception while queuing: ${e.message}")
                                        onLog("   Falling back to queueTransactionFromBase64...")
                                        bleService.queueTransactionFromBase64(signedTxBase64)
                                        onLog("✅ Transaction queued (fallback method)")
                                        createdSplTransaction = null
                                    }
                                }
                            } else {
                                onLog("⚠️ BLE service not available - cannot queue transaction")
                                onLog("   Signed transaction: ${signedTxBase64.take(100)}...")
                            }
                        } catch (e: MwaException) {
                            onLog("❌ MWA signing failed: ${e.message}")
                        } catch (e: Exception) {
                            onLog("❌ Exception during signing: ${e.message}")
                        } finally {
                            isSigningSpl = false
                        }
                    }
                },
                enabled = !isSigningSpl && mwaClient != null && createdSplTransaction != null,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.secondary
                )
            ) {
                Text(if (isSigningSpl) "Signing..." else "✍️ Sign with MWA & Queue")
            }
        }
        
        HorizontalDivider()
        
        Text(
            text = "Test Vote Transaction",
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary
        )
        
        OutlinedTextField(
            value = voteVoter,
            onValueChange = { voteVoter = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Voter Pubkey (base58)") },
            placeholder = { Text("Enter voter pubkey") },
            enabled = !isTesting && sdk != null
        )
        
        OutlinedTextField(
            value = voteProposalId,
            onValueChange = { voteProposalId = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Proposal ID (base58)") },
            placeholder = { Text("Enter proposal pubkey") },
            enabled = !isTesting && sdk != null
        )
        
        OutlinedTextField(
            value = voteAccount,
            onValueChange = { voteAccount = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Vote Account (base58)") },
            placeholder = { Text("Enter vote account pubkey") },
            enabled = !isTesting && sdk != null
        )
        
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            OutlinedTextField(
                value = voteChoice,
                onValueChange = { voteChoice = it },
                modifier = Modifier.weight(1f),
                label = { Text("Choice (0-255)") },
                placeholder = { Text("0") },
                enabled = !isTesting && sdk != null
            )
            
            OutlinedTextField(
                value = voteFeePayer,
                onValueChange = { voteFeePayer = it },
                modifier = Modifier.weight(1f),
                label = { Text("Fee Payer") },
                placeholder = { Text("Fee payer pubkey") },
                enabled = !isTesting && sdk != null
            )
        }
        
        OutlinedTextField(
            value = voteNonceAccount,
            onValueChange = { voteNonceAccount = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Nonce Account (base58, optional)") },
            placeholder = { Text("Enter nonce account pubkey or use picked nonce") },
            enabled = !isTesting && sdk != null
        )
        
        if (pickedNonce != null) {
            Text(
                text = "💡 Tip: Picked nonce available - will be used if nonce account field is empty",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(vertical = 4.dp)
            )
        }
        
        Button(
            onClick = {
                if (sdk == null) {
                    onLog("❌ SDK not initialized")
                    return@Button
                }
                isTesting = true
                scope.launch {
                    try {
                        onLog("🗳️ Testing vote transaction...")
                        val choice = voteChoice.toIntOrNull()?.toUByte() ?: 0u
                        
                        // Optionally use picked nonce data if available and nonce account field is empty
                        val nonceData = if (voteNonceAccount.isBlank() && pickedNonce != null) {
                            onLog("📌 Using picked nonce data (no RPC call)")
                            xyz.pollinet.sdk.CachedNonceData(
                                version = 1,
                                nonceAccount = pickedNonce!!.nonceAccount,
                                authority = pickedNonce!!.authority,
                                blockhash = pickedNonce!!.blockhash,
                                lamportsPerSignature = pickedNonce!!.lamportsPerSignature,
                                cachedAt = pickedNonce!!.cachedAt,
                                used = pickedNonce!!.used
                            )
                        } else null
                        
                        val nonceAccountStr = if (voteNonceAccount.isNotBlank()) voteNonceAccount else null
                        
                        // Verify that if using picked nonce, the authority matches the voter
                        if (nonceData != null && nonceData.authority != voteVoter) {
                            onLog("❌ Vote transaction failed: Nonce authority (${nonceData.authority.take(16)}...) does not match voter (${voteVoter.take(16)}...)")
                            onLog("   💡 The nonce authority must match the voter for vote transactions")
                            return@launch
                        }
                        
                        val result = sdk.createUnsignedVote(
                            voter = voteVoter,
                            proposalId = voteProposalId,
                            voteAccount = voteAccount,
                            choice = choice.toInt(),
                            feePayer = voteFeePayer.ifEmpty { voteVoter },
                            nonceAccount = nonceAccountStr,
                            nonceData = nonceData
                        )
                        
                        result.onSuccess { txBase64 ->
                            onLog("✅ Vote transaction created!")
                            if (nonceData != null) {
                                onLog("   ✅ Used cached nonce data (no RPC call)")
                            } else {
                                onLog("   ✅ Fetched nonce data from blockchain")
                            }
                            onLog("   Length: ${txBase64.length} chars")
                            onLog("   Preview: ${txBase64.take(80)}...")
                            onLog("   Ready for signing")
                        }.onFailure { e ->
                            onLog("❌ Vote transaction failed: ${e.message}")
                        }
                    } catch (e: Exception) {
                        onLog("❌ Exception: ${e.message}")
                    } finally {
                        isTesting = false
                    }
                }
            },
            enabled = !isTesting && sdk != null && voteVoter.isNotBlank() && voteProposalId.isNotBlank() && voteAccount.isNotBlank() && (voteNonceAccount.isNotBlank() || pickedNonce != null),
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(if (isTesting) "Creating..." else "Create Vote Transaction")
        }
        
        HorizontalDivider()
        
        Text(
            text = "Test Nonce Account Creation & Caching",
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary
        )
        
        Text(
            text = "⚠️ Flow: 1) Create unsigned TXs → 2) Authorize MWA → 3) Sign & Submit → 4) Cache",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 4.dp)
        )
        
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            OutlinedTextField(
                value = nonceCount,
                onValueChange = { nonceCount = it },
                modifier = Modifier.weight(1f),
                label = { Text("Count") },
                placeholder = { Text("3") },
                enabled = !isTesting && sdk != null
            )
            
            OutlinedTextField(
                value = noncePayerPubkey,
                onValueChange = { noncePayerPubkey = it },
                modifier = Modifier.weight(2f),
                label = { Text("Payer Pubkey") },
                placeholder = { Text("Enter payer pubkey") },
                enabled = !isTesting && sdk != null && authorizedPubkey == null
            )
        }
        
        if (authorizedPubkey == null) {
            Button(
                onClick = {
                    if (mwaClient == null) {
                        onLog("❌ MWA client not initialized")
                        return@Button
                    }
                    isAuthorizing = true
                    scope.launch {
                        try {
                            onLog("🔐 Authorizing with wallet...")
                            val pubkey = mwaClient!!.authorize(mwaActivityResultSender)
                            authorizedPubkey = pubkey
                            noncePayerPubkey = pubkey
                            onLog("✅ Authorized: ${pubkey.take(16)}...")
                        } catch (e: MwaException) {
                            onLog("❌ Authorization failed: ${e.message}")
                        } catch (e: Exception) {
                            onLog("❌ Exception: ${e.message}")
                        } finally {
                            isAuthorizing = false
                        }
                    }
                },
                enabled = !isAuthorizing && mwaClient != null,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text(if (isAuthorizing) "Authorizing..." else "1️⃣ Authorize MWA")
            }
        } else {
            Text(
                text = "✅ Authorized: ${authorizedPubkey!!.take(16)}...",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(bottom = 4.dp)
            )
        }
        
        Button(
            onClick = {
                if (sdk == null) {
                    onLog("❌ SDK not initialized")
                    return@Button
                }
                if (authorizedPubkey == null) {
                    onLog("❌ Please authorize MWA first")
                    return@Button
                }
                isTesting = true
                scope.launch {
                    try {
                        onLog("🔑 Step 1: Creating unsigned nonce account transactions...")
                        val count = nonceCount.toIntOrNull() ?: 3
                        if (count <= 0 || count > 10) {
                            onLog("❌ Count must be 1-10")
                            return@launch
                        }
                        
                        val payer = noncePayerPubkey.ifEmpty { authorizedPubkey!! }
                        
                        // Create unsigned transactions (without caching - accounts don't exist yet!)
                        // Use callback to store nonce pubkeys for later caching
                        var noncePubkeysForCaching: List<String>? = null
                        
                        val result = sdk.createUnsignedNonceAccountsAndCache(
                            count = count,
                            payerPubkey = payer,
                            onCreated = { pubkeys ->
                                noncePubkeysForCaching = pubkeys
                                onLog("📝 Stored ${pubkeys.size} nonce pubkeys for automatic caching after submission")
                            }
                        )
                        
                        result.onSuccess { transactions ->
                            createdNonceTransactions = transactions
                            val totalNonceAccounts = transactions.sumOf { it.noncePubkey.size }
                            onLog("✅ Created ${transactions.size} batched transaction(s)!")
                            onLog("   Total nonce accounts: $totalNonceAccounts (max 5 per transaction)")
                            transactions.forEachIndexed { index, tx ->
                                onLog("   Transaction ${index + 1}: ${tx.noncePubkey.size} nonce account(s)")
                                tx.noncePubkey.forEachIndexed { nonceIdx, pubkey ->
                                    onLog("     - Nonce ${nonceIdx + 1}: ${pubkey.take(16)}...")
                                }
                            }
                            onLog("   ⚠️ Accounts don't exist yet - need to sign & submit first!")
                            onLog("   Ready for Step 2: Sign & Submit")
                            onLog("   💡 Nonce pubkeys stored - will auto-cache after successful submission")
                        }.onFailure { e ->
                            onLog("❌ Nonce creation failed: ${e.message}")
                        }
                    } catch (e: Exception) {
                        onLog("❌ Exception: ${e.message}")
                    } finally {
                        isTesting = false
                    }
                }
            },
            enabled = !isTesting && sdk != null && authorizedPubkey != null && createdNonceTransactions == null,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(if (isTesting) "Creating..." else "2️⃣ Create Unsigned Nonce TXs")
        }
        
        if (createdNonceTransactions != null && mwaClient != null && authorizedPubkey != null) {
            Button(
                onClick = {
                    isTesting = true
                    scope.launch {
                        try {
                            val transactions = createdNonceTransactions!!
                            val totalNonceAccounts = transactions.sumOf { it.noncePubkey.size }
                            onLog("✍️ Step 2: Signing ${transactions.size} batched transaction(s) with MWA...")
                            onLog("   Total nonce accounts to create: $totalNonceAccounts")
                            
                            var successCount = 0
                            var totalNonceAccountsCreated = 0
                            val successfulSignatures = mutableListOf<String>()
                            
                            for ((index, unsignedTx) in transactions.withIndex()) {
                                try {
                                    val nonceCountInTx = unsignedTx.noncePubkey.size
                                    onLog("   Signing transaction ${index + 1}/${transactions.size} (${nonceCountInTx} nonce accounts)...")
                                    
                                    // Refresh blockhash right before signing to ensure it's fresh
                                    val refreshedTxBase64 = sdk!!.refreshBlockhashInUnsignedTransaction(
                                        unsignedTx.unsignedTransactionBase64
                                    ).getOrElse { 
                                        onLog("   ⚠️ Failed to refresh blockhash, using original: ${it.message}")
                                        unsignedTx.unsignedTransactionBase64
                                    }
                                    
                                    // Sign with MWA (payer signature)
                                    val signedTxBytes = mwaClient!!.signTransaction(
                                        mwaActivityResultSender,
                                        refreshedTxBase64
                                    )
                                    
                                    // Add nonce signatures (co-sign all nonce accounts in this batch)
                                    val finalTxResult = sdk!!.addNonceSignature(
                                        payerSignedTransactionBase64 = android.util.Base64.encodeToString(
                                            signedTxBytes,
                                            android.util.Base64.NO_WRAP
                                        ),
                                        nonceKeypairBase64 = unsignedTx.nonceKeypairBase64
                                    )
                                    
                                    finalTxResult.fold(
                                        onSuccess = { finalTxBase64 ->
                                            // Submit to blockchain and automatically cache
                                            onLog("   Submitting transaction ${index + 1}...")
                                            val submitResult = sdk.submitNonceAccountCreationAndCache(
                                                unsignedTransaction = unsignedTx,
                                                finalSignedTransactionBase64 = finalTxBase64
                                            )
                                            submitResult.fold(
                                                onSuccess = { signature ->
                                                    successCount++
                                                    totalNonceAccountsCreated += nonceCountInTx
                                                    successfulSignatures.add(signature)
                                                    onLog("   ✅ Transaction ${index + 1} submitted: ${nonceCountInTx} nonce account(s) created & cached")
                                                    onLog("      Signature: ${signature.take(16)}...")
                                                },
                                                onFailure = { e ->
                                                    onLog("   ❌ Submission failed: ${e.message}")
                                                }
                                            )
                                        },
                                        onFailure = { e ->
                                            onLog("   ❌ Co-sign failed: ${e.message}")
                                        }
                                    )
                                } catch (e: MwaException) {
                                    onLog("   ❌ MWA error: ${e.message}")
                                } catch (e: Exception) {
                                    onLog("   ❌ Error: ${e.message}")
                                }
                            }
                            
                            if (successCount == 0) {
                                onLog("❌ Failed to create any nonce accounts")
                                return@launch
                            }
                            
                            onLog("✅ Created & cached $totalNonceAccountsCreated/$totalNonceAccounts nonce accounts!")
                            onLog("   Successfully submitted $successCount/${transactions.size} transaction(s)")
                            onLog("   All nonce accounts are now in the offline bundle")
                            onLog("   Ready for offline transactions!")
                            createdNonceTransactions = null // Reset
                        } catch (e: Exception) {
                            onLog("❌ Exception: ${e.message}")
                        } finally {
                            isTesting = false
                        }
                    }
                },
                enabled = !isTesting && createdNonceTransactions != null && mwaClient != null && authorizedPubkey != null,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text(if (isTesting) "Signing & Submitting..." else "3️⃣ Sign, Submit & Cache")
            }
        }
        
        HorizontalDivider()
        
        Text(
            text = "Test Get Available Nonce Account",
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.typography.titleSmall.color
        )
        
        Text(
            text = "Test picking an available nonce account from cached bundle",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 4.dp)
        )
        
        if (pickedNonce != null) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer
                )
            ) {
                Column(
                    modifier = Modifier.padding(12.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Text(
                        text = "✅ Available Nonce Account Picked!",
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer
                    )
                    
                    StatusRow(
                        label = "Nonce Account",
                        value = pickedNonce!!.nonceAccount.take(32) + "...",
                        isGood = true
                    )
                    
                    StatusRow(
                        label = "Authority",
                        value = pickedNonce!!.authority.take(32) + "...",
                        isGood = true
                    )
                    
                    StatusRow(
                        label = "Blockhash",
                        value = pickedNonce!!.blockhash.take(32) + "...",
                        isGood = true
                    )
                    
                    StatusRow(
                        label = "Fee (lamports/sig)",
                        value = pickedNonce!!.lamportsPerSignature.toString(),
                        isGood = true
                    )
                    
                    StatusRow(
                        label = "Cached At",
                        value = java.text.SimpleDateFormat("yyyy-MM-dd HH:mm:ss", java.util.Locale.getDefault())
                            .format(java.util.Date(pickedNonce!!.cachedAt * 1000)),
                        isGood = true
                    )
                    
                    StatusRow(
                        label = "Used",
                        value = if (pickedNonce!!.used) "Yes" else "No",
                        isGood = !pickedNonce!!.used
                    )
                }
            }
        }
        
        Button(
            onClick = {
                if (sdk == null) {
                    onLog("❌ SDK not initialized")
                    return@Button
                }
                isPickingNonce = true
                pickedNonce = null
                scope.launch {
                    try {
                        onLog("🔍 Testing getAvailableNonce()...")
                        onLog("   Loading bundle from secure storage...")
                        
                        val result = sdk.getAvailableNonce()
                        
                        result.onSuccess { nonce ->
                            if (nonce != null) {
                                pickedNonce = nonce
                                onLog("✅ Successfully picked available nonce account!")
                                onLog("   Nonce Account: ${nonce.nonceAccount}")
                                onLog("   Authority: ${nonce.authority}")
                                onLog("   Blockhash: ${nonce.blockhash.take(32)}...")
                                onLog("   Fee: ${nonce.lamportsPerSignature} lamports/signature")
                                onLog("   Cached At: ${java.text.SimpleDateFormat("yyyy-MM-dd HH:mm:ss", java.util.Locale.getDefault()).format(java.util.Date(nonce.cachedAt * 1000))}")
                                onLog("   Used: ${if (nonce.used) "Yes" else "No"}")
                                onLog("   ✅ This nonce can be used for creating transactions!")
                            } else {
                                onLog("⚠️ No available nonce accounts found")
                                onLog("   All nonces in bundle are marked as used")
                                onLog("   💡 Try refreshing the bundle or creating new nonce accounts")
                                pickedNonce = null
                            }
                        }.onFailure { e ->
                            onLog("❌ Failed to get available nonce: ${e.message}")
                            onLog("   Possible reasons:")
                            onLog("   - Secure storage not configured")
                            onLog("   - No bundle found (create nonce accounts first)")
                            onLog("   - Bundle loading error")
                            pickedNonce = null
                        }
                    } catch (e: Exception) {
                        onLog("❌ Exception: ${e.message}")
                        pickedNonce = null
                    } finally {
                        isPickingNonce = false
                    }
                }
            },
            enabled = !isPickingNonce && sdk != null,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(if (isPickingNonce) "Picking Nonce..." else "🔍 Get Available Nonce Account")
        }
        
        if (pickedNonce != null) {
            Text(
                text = "💡 This nonce will be used when creating offline transactions above",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(vertical = 4.dp)
            )
            
            // Test buttons: Create online transactions using picked nonce data
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                // Test SOL transaction
                Button(
                    onClick = {
                        if (sdk == null) {
                            onLog("❌ SDK not initialized")
                            return@Button
                        }
                        if (pickedNonce == null) {
                            onLog("❌ No nonce picked")
                            return@Button
                        }
                        isTesting = true
                        scope.launch {
                            try {
                                onLog("🧪 Testing ONLINE SOL transaction with picked nonce data...")
                                onLog("   This demonstrates using nonceData (no RPC call needed)")
                                
                                // Convert picked nonce to SDK type
                                val nonceData = xyz.pollinet.sdk.CachedNonceData(
                                    version = 1,
                                    nonceAccount = pickedNonce!!.nonceAccount,
                                    authority = pickedNonce!!.authority,
                                    blockhash = pickedNonce!!.blockhash,
                                    lamportsPerSignature = pickedNonce!!.lamportsPerSignature,
                                    cachedAt = pickedNonce!!.cachedAt,
                                    used = pickedNonce!!.used
                                )
                                
                                // Use nonce authority as sender (required - authority must match sender)
                                val sender = pickedNonce!!.authority
                                val recipient = "AtHGwWe2cZQ1WbsPVHFsCm4FqUDW8pcPLYXWsA89iuDE"
                                
                                onLog("   Sender (nonce authority): ${sender.take(16)}...")
                                onLog("   Nonce Account: ${pickedNonce!!.nonceAccount.take(16)}...")
                                onLog("   Using cached nonce data (no RPC call)")
                                
                                val result = sdk.createUnsignedTransaction(
                                    sender = sender,
                                    recipient = recipient,
                                    feePayer = sender,
                                    amount = 1000000, // 0.001 SOL
                                    nonceAccount = null, // Not needed when using nonceData
                                    nonceData = nonceData
                                )
                                
                                result.onSuccess { txBase64 ->
                                    onLog("✅ SOL transaction created using picked nonce!")
                                    onLog("   Transaction length: ${txBase64.length} chars")
                                    onLog("   Preview: ${txBase64.take(80)}...")
                                    onLog("   ✅ No RPC call was made - used cached nonce data")
                                    onLog("   ✅ Authority validation passed")
                                }.onFailure { e ->
                                    onLog("❌ Transaction creation failed: ${e.message}")
                                    if (e.message?.contains("authority") == true) {
                                        onLog("   💡 Nonce authority must match sender")
                                    }
                                }
                            } catch (e: Exception) {
                                onLog("❌ Exception: ${e.message}")
                            } finally {
                                isTesting = false
                            }
                        }
                    },
                    enabled = !isTesting && sdk != null && pickedNonce != null,
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.secondary
                    )
                ) {
                    Text("🧪 Test: Create Online SOL TX with Picked Nonce")
                }
                
                // Test SPL transaction
                Button(
                    onClick = {
                        if (sdk == null) {
                            onLog("❌ SDK not initialized")
                            return@Button
                        }
                        if (pickedNonce == null) {
                            onLog("❌ No nonce picked")
                            return@Button
                        }
                        isTesting = true
                        scope.launch {
                            try {
                                onLog("🧪 Testing ONLINE SPL transaction with picked nonce data...")
                                onLog("   This demonstrates using nonceData for SPL tokens (no RPC call)")
                                
                                // Convert picked nonce to SDK type
                                val nonceData = xyz.pollinet.sdk.CachedNonceData(
                                    version = 1,
                                    nonceAccount = pickedNonce!!.nonceAccount,
                                    authority = pickedNonce!!.authority,
                                    blockhash = pickedNonce!!.blockhash,
                                    lamportsPerSignature = pickedNonce!!.lamportsPerSignature,
                                    cachedAt = pickedNonce!!.cachedAt,
                                    used = pickedNonce!!.used
                                )
                                
                                // Use nonce authority as sender (required - authority must match sender)
                                val sender = pickedNonce!!.authority
                                val recipient = "EufFKpRgwpdXMuXpEG6Nh2bb77tFnj9d6FgSAEnsSMQy"
                                val mintAddress = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU" // USDC on devnet
                                
                                onLog("   Sender (nonce authority): ${sender.take(16)}...")
                                onLog("   Nonce Account: ${pickedNonce!!.nonceAccount.take(16)}...")
                                onLog("   Mint: USDC on devnet")
                                onLog("   Using cached nonce data (no RPC call)")
                                
                                val result = sdk.createUnsignedSplTransaction(
                                    senderWallet = sender,
                                    recipientWallet = recipient,
                                    feePayer = sender,
                                    mintAddress = mintAddress,
                                    amount = 100_000, // 0.1 USDC (6 decimals)
                                    nonceAccount = null, // Not needed when using nonceData
                                    nonceData = nonceData
                                )
                                
                                result.onSuccess { txBase64 ->
                                    onLog("✅ SPL transaction created using picked nonce!")
                                    onLog("   Transaction length: ${txBase64.length} chars")
                                    onLog("   Preview: ${txBase64.take(80)}...")
                                    onLog("   ✅ No RPC call was made - used cached nonce data")
                                    onLog("   ✅ Authority validation passed")
                                    onLog("   ✅ Includes idempotent ATA creation")
                                }.onFailure { e ->
                                    onLog("❌ SPL transaction creation failed: ${e.message}")
                                    if (e.message?.contains("authority") == true) {
                                        onLog("   💡 Nonce authority must match sender wallet")
                                    }
                                }
                            } catch (e: Exception) {
                                onLog("❌ Exception: ${e.message}")
                            } finally {
                                isTesting = false
                            }
                        }
                    },
                    enabled = !isTesting && sdk != null && pickedNonce != null,
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.tertiary
                    )
                ) {
                    Text("🧪 Test: Create Online SPL TX with Picked Nonce")
                }
                
                // Test Vote transaction
                Button(
                    onClick = {
                        if (sdk == null) {
                            onLog("❌ SDK not initialized")
                            return@Button
                        }
                        if (pickedNonce == null) {
                            onLog("❌ No nonce picked")
                            return@Button
                        }
                        isTesting = true
                        scope.launch {
                            try {
                                onLog("🗳️ Testing ONLINE vote transaction with picked nonce data...")
                                onLog("   This demonstrates using nonceData for votes (no RPC call)")
                                
                                // Convert picked nonce to SDK type
                                val nonceData = xyz.pollinet.sdk.CachedNonceData(
                                    version = 1,
                                    nonceAccount = pickedNonce!!.nonceAccount,
                                    authority = pickedNonce!!.authority,
                                    blockhash = pickedNonce!!.blockhash,
                                    lamportsPerSignature = pickedNonce!!.lamportsPerSignature,
                                    cachedAt = pickedNonce!!.cachedAt,
                                    used = pickedNonce!!.used
                                )
                                
                                // Use nonce authority as voter (required - authority must match voter)
                                val voter = pickedNonce!!.authority
                                val proposalId = "11111111111111111111111111111111" // Example proposal ID
                                val voteAccount = "Vote111111111111111111111111111111111111111" // Example vote account
                                val feePayer = voter
                                
                                onLog("   Voter (nonce authority): ${voter.take(16)}...")
                                onLog("   Nonce Account: ${pickedNonce!!.nonceAccount.take(16)}...")
                                onLog("   Using cached nonce data (no RPC call)")
                                
                                val result = sdk.createUnsignedVote(
                                    voter = voter,
                                    proposalId = proposalId,
                                    voteAccount = voteAccount,
                                    choice = 1, // Yes vote
                                    feePayer = feePayer,
                                    nonceAccount = null, // Not needed when using nonceData
                                    nonceData = nonceData
                                )
                                
                                result.onSuccess { txBase64 ->
                                    onLog("✅ Vote transaction created using picked nonce!")
                                    onLog("   Transaction length: ${txBase64.length} chars")
                                    onLog("   Preview: ${txBase64.take(80)}...")
                                    onLog("   ✅ No RPC call was made - used cached nonce data")
                                    onLog("   ✅ Authority validation passed")
                                }.onFailure { e ->
                                    onLog("❌ Vote transaction creation failed: ${e.message}")
                                    if (e.message?.contains("authority") == true) {
                                        onLog("   💡 Nonce authority must match voter wallet")
                                    }
                                }
                            } catch (e: Exception) {
                                onLog("❌ Exception: ${e.message}")
                            } finally {
                                isTesting = false
                            }
                        }
                    },
                    enabled = !isTesting && sdk != null && pickedNonce != null,
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary
                    )
                ) {
                    Text("🧪 Test: Create Online Vote TX with Picked Nonce")
                }
            }
            
            OutlinedButton(
                onClick = {
                    pickedNonce = null
                    onLog("🔄 Cleared picked nonce display")
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Clear Display")
            }
        }
        
        if (sdk == null) {
            Text(
                text = "⚠️ SDK not initialized. Initialize SDK in MainActivity first.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error
            )
        }
    }
}


