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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import xyz.pollinet.sdk.*

@Composable
fun DiagnosticsScreen() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    
    var bleService by remember { mutableStateOf<BleService?>(null) }
    var isBound by remember { mutableStateOf(false) }
    
    val connectionState by bleService?.connectionState?.collectAsStateWithLifecycle(BleService.ConnectionState.DISCONNECTED) 
        ?: remember { mutableStateOf(BleService.ConnectionState.DISCONNECTED) }
    
    val metrics by bleService?.metrics?.collectAsStateWithLifecycle(null) 
        ?: remember { mutableStateOf(null) }
    
    var permissionsGranted by remember { mutableStateOf(false) }
    var sdkVersion by remember { mutableStateOf("Unknown") }

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
                            logLevel = "info"
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
        sdkVersion = PolliNetSDK.version()
        
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

        // Metrics
        StatusCard(
            title = "Metrics",
            content = {
                MetricsContent(metrics = metrics)
            }
        )
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

