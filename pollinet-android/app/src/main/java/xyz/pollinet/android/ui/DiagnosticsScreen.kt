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
import xyz.pollinet.sdk.BleService
import xyz.pollinet.sdk.CreateUnsignedTransactionRequest
import xyz.pollinet.sdk.MetricsSnapshot
import xyz.pollinet.sdk.PolliNetSDK
import xyz.pollinet.sdk.SdkConfig

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
                            rpcUrl = null,
                            enableLogging = true,
                            logLevel = "info",
                            storageDirectory = context.filesDir.absolutePath
                        )
                    )
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

        // Offline Bundle Demo
        StatusCard(
            title = "🚀 Offline Bundle Demo (Core PolliNet)",
            content = {
                OfflineBundleDemo(
                    context = context,
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
        
        // BLE Mesh Broadcast Visualization
        BroadcastVisualizationCard(
            sdk = mainSdk
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
                            val txResult = sdk.createUnsignedTransaction(
                                CreateUnsignedTransactionRequest(
                                    sender = "2JnzJqwqLvrLZBAsu58jJtrMn1mT38Be3tcJBigmkTZq",
                                    recipient = "AtHGwWe2cZQ1WbsPVHFsCm4FqUDW8pcPLYXWsA89iuDE",
                                    feePayer = "2JnzJqwqLvrLZBAsu58jJtrMn1mT38Be3tcJBigmkTZq",
                                    amount = 1000000,
                                    nonceAccount = "2JnzJqwqLvrLZBAsu58jJtrMn1mT38Be3tcJBigmkTZq"
                                )
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
private fun OfflineBundleDemo(
    context: android.content.Context,
    isTestingSdk: Boolean,
    onTestStart: () -> Unit,
    onTestComplete: () -> Unit,
    onLog: (String) -> Unit,
    scope: kotlinx.coroutines.CoroutineScope
) {
    var bundle by remember { mutableStateOf<xyz.pollinet.sdk.OfflineTransactionBundle?>(null) }
    var offlineTransaction by remember { mutableStateOf<String?>(null) }
    
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            text = "Core PolliNet Feature: Create transactions completely offline!",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.primary
        )
        
        // Step 1: Prepare Bundle
        Button(
            onClick = {
                onTestStart()
                scope.launch {
                    try {
                        onLog("📦 Step 1: Preparing Offline Bundle...")
                        onLog("   Creating 3 nonce accounts for offline use")
                        onLog("   Cost: 3 × $0.20 = $0.60 (first time)")
                        
                        val config = xyz.pollinet.sdk.SdkConfig(
                            rpcUrl = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6",
                            enableLogging = true,
                            storageDirectory = context.filesDir.absolutePath
                        )
                        
                        xyz.pollinet.sdk.PolliNetSDK.initialize(config).onSuccess { sdk ->
                            // Use real funded wallet for testing
                            val privateKeyBase58 = "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu"
                            val keypairBytes = decodeBase58(privateKeyBase58)
                            
                            sdk.prepareOfflineBundle(
                                count = 3,
                                senderKeypair = keypairBytes,
                                bundleFile = null // Managed by Rust secure storage
                            ).onSuccess { preparedBundle ->
                                bundle = preparedBundle
                                onLog("✓ Bundle prepared!")
                                onLog("  Total nonces: ${preparedBundle.totalNonces()}")
                                onLog("  Available: ${preparedBundle.availableNonces()}")
                                onLog("  Ready for offline transaction creation!")
                            }.onFailure {
                                onLog("✗ Bundle failed: ${it.message}")
                            }
                            
                            sdk.shutdown()
                        }.onFailure {
                            onLog("✗ SDK init failed: ${it.message}")
                        }
                    } catch (e: Exception) {
                        onLog("✗ Exception: ${e.message}")
                    } finally {
                        onTestComplete()
                    }
                }
            },
            enabled = !isTestingSdk && bundle == null,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("1️⃣ Prepare Offline Bundle (3 nonces)")
        }
        
        if (bundle != null) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer
                )
            ) {
                Column(modifier = Modifier.padding(12.dp)) {
                    Text(
                        "📊 Bundle Status",
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        "Available: ${bundle!!.availableNonces()} | Used: ${bundle!!.usedNonces()}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer
                    )
                }
            }
        }
        
        // Step 2: Create Offline Transaction
        Button(
            onClick = {
                onTestStart()
                scope.launch {
                    try {
                        onLog("📴 Step 2: Creating Transaction OFFLINE...")
                        onLog("   NO INTERNET REQUIRED!")
                        onLog("   Using cached nonce data")
                        
                        val config = xyz.pollinet.sdk.SdkConfig(
                            rpcUrl = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6",
                            enableLogging = true,
                            storageDirectory = context.filesDir.absolutePath
                        )
                        
                        xyz.pollinet.sdk.PolliNetSDK.initialize(config).onSuccess { sdk ->
                            // No need to pass nonce - Rust picks it from storage automatically!
                            val privateKeyBase58 = "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu"
                            val keypairBytes = decodeBase58(privateKeyBase58)
                            
                            sdk.createOfflineTransaction(
                                senderKeypair = keypairBytes,
                                nonceAuthorityKeypair = keypairBytes,
                                recipient = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX",
                                amount = 1_000_000 // 0.001 SOL
                                // Nonce automatically picked from stored bundle!
                            ).onSuccess { tx ->
                                offlineTransaction = tx
                                onLog("✓ Transaction created OFFLINE!")
                                onLog("  Size: ${tx.length} chars (base64)")
                                onLog("  Nonce marked as used in storage")
                                onLog("  Ready for BLE transmission")
                                onLog("  Can submit when back online")
                            }.onFailure {
                                onLog("✗ Transaction failed: ${it.message}")
                            }

                            sdk.shutdown()
                        }
                    } catch (e: Exception) {
                        onLog("✗ Exception: ${e.message}")
                    } finally {
                        onTestComplete()
                    }
                }
            },
            enabled = !isTestingSdk && bundle != null && offlineTransaction == null,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("2️⃣ Create Transaction (Offline)")
        }
        
        if (offlineTransaction != null) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.tertiaryContainer
                )
            ) {
                Column(modifier = Modifier.padding(12.dp)) {
                    Text(
                        "📡 Offline Transaction Ready",
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        "Created without internet! Ready for BLE mesh propagation.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                }
            }
        }
        
        // Step 3: Submit (when online)
        Button(
            onClick = {
                onTestStart()
                scope.launch {
                    try {
                        onLog("🌐 Step 3: Submitting Transaction...")
                        onLog("   Back online - submitting to blockchain")
                        
                        val config = xyz.pollinet.sdk.SdkConfig(
                            rpcUrl = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6",
                            enableLogging = true,
                            storageDirectory = context.filesDir.absolutePath
                        )
                        
                        xyz.pollinet.sdk.PolliNetSDK.initialize(config).onSuccess { sdk ->
                            offlineTransaction?.let { tx ->
                                sdk.submitOfflineTransaction(
                                    transactionBase64 = tx,
                                    verifyNonce = true
                                ).onSuccess { signature ->
                                    onLog("✓ Transaction submitted!")
                                    onLog("  Signature: ${signature.take(20)}...")
                                    onLog("  🎉 Complete offline → online flow!")
                                    // Reset for next demo
                                    bundle = null
                                    offlineTransaction = null
                                }.onFailure {
                                    onLog("✗ Submit failed: ${it.message}")
                                    onLog("  (Demo keypair has no balance)")
                                }
                            }
                            
                            sdk.shutdown()
                        }
                    } catch (e: Exception) {
                        onLog("✗ Exception: ${e.message}")
                    } finally {
                        onTestComplete()
                    }
                }
            },
            enabled = !isTestingSdk && offlineTransaction != null,
            modifier = Modifier.fillMaxWidth(),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.tertiary
            )
        ) {
            Text("3️⃣ Submit Transaction (Online)")
        }
        
        // Reset button
        if (bundle != null || offlineTransaction != null) {
            OutlinedButton(
                onClick = {
                    bundle = null
                    offlineTransaction = null
                    onLog("🔄 Demo reset")
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Reset Demo")
            }
        }
        
        HorizontalDivider()
        
        Text(
            text = "💡 This demonstrates PolliNet's core feature: True offline transaction creation with smart cost optimization!",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}

/**
 * Decode a base58-encoded Solana private key to bytes
 * Base58 alphabet: 123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz
 */
private fun decodeBase58(input: String): ByteArray {
    val ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    val decoded = java.math.BigInteger.ZERO
    var result = decoded
    
    for (char in input) {
        val digit = ALPHABET.indexOf(char)
        if (digit < 0) {
            throw IllegalArgumentException("Invalid base58 character: $char")
        }
        result = result.multiply(java.math.BigInteger.valueOf(58))
            .add(java.math.BigInteger.valueOf(digit.toLong()))
    }
    
    // Convert to byte array
    val bytes = result.toByteArray()
    
    // Count leading zeros in input
    val leadingZeros = input.takeWhile { it == '1' }.length
    
    // Remove sign byte if present and add leading zeros
    val stripped = if (bytes[0].toInt() == 0) bytes.drop(1).toByteArray() else bytes
    return ByteArray(leadingZeros) + stripped
}

