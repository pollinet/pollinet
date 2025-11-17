package xyz.pollinet.android

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import kotlinx.coroutines.launch
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import xyz.pollinet.android.ui.DiagnosticsScreen
import xyz.pollinet.android.ui.SigningScreen
import xyz.pollinet.android.ui.TransactionBuilderScreen
import xyz.pollinet.android.ui.theme.PollinetandroidTheme
import xyz.pollinet.sdk.BleService
import xyz.pollinet.sdk.PolliNetSDK
import xyz.pollinet.sdk.SdkConfig

class MainActivity : ComponentActivity() {
    companion object {
        private const val TAG = "PolliNet.MainActivity"
    }
    
    private var bleService: BleService? = null
    private var isBound = false
    private var sdk: PolliNetSDK? = null
    private var permissionsGranted = false
    
    // MWA ActivityResultSender - MUST be initialized as a field (not lazy) before onCreate
    private val mwaActivityResultSender = ActivityResultSender(this)
    
    private val serviceConnection = object : ServiceConnection {
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
    
    // Permission launcher
    private val requestPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        permissionsGranted = permissions.values.all { it }
        if (permissionsGranted) {
            startBleService()
        }
    }
    
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        
        Log.d(TAG, "onCreate: Starting PolliNet Android")
        
        enableEdgeToEdge()
        setContent {
            PollinetandroidTheme {
                PolliNetApp(mwaActivityResultSender = mwaActivityResultSender)
            }
        }
        
        // Request BLE permissions
        requestBlePermissions()
        Log.d(TAG, "onCreate: Completed")
    }
    
    private fun requestBlePermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // Android 12+
            arrayOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_ADVERTISE
            )
        } else {
            // Android 10-11
            arrayOf(
                Manifest.permission.ACCESS_FINE_LOCATION
            )
        }
        
        // Check if permissions are already granted
        val allGranted = permissions.all {
            ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED
        }
        
        if (allGranted) {
            permissionsGranted = true
            startBleService()
        } else {
            requestPermissionLauncher.launch(permissions)
        }
    }
    
    private fun startBleService() {
        Log.d(TAG, "startBleService: Starting BLE service")
        val intent = Intent(this, BleService::class.java).apply {
            action = BleService.ACTION_START
        }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(intent)
            } else {
                startService(intent)
            }
            bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
            Log.d(TAG, "startBleService: Service started successfully")
        } catch (e: Exception) {
            Log.e(TAG, "startBleService: Failed to start service", e)
        }
    }
    
    override fun onDestroy() {
        if (isBound) {
            unbindService(serviceConnection)
        }
        sdk?.shutdown()
        super.onDestroy()
    }
}

@Composable
fun PolliNetApp(mwaActivityResultSender: ActivityResultSender) {
    var selectedTab by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()
    var sdk by remember { mutableStateOf<PolliNetSDK?>(null) }
    
    // Initialize SDK
    val context = LocalContext.current
    LaunchedEffect(Unit) {
        scope.launch {
            PolliNetSDK.initialize(
                SdkConfig(
                    rpcUrl = "https://api.devnet.solana.com", // Use devnet for testing
                    enableLogging = true,
                    logLevel = "info",
                    storageDirectory = context.filesDir.absolutePath
                )
            ).onSuccess {
                sdk = it
            }
        }
    }
    
    Scaffold(
        modifier = Modifier.fillMaxSize(),
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Settings, contentDescription = "Diagnostics") },
                    label = { Text("Diagnostics") },
                    selected = selectedTab == 0,
                    onClick = { selectedTab = 0 }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Build, contentDescription = "Build Tx") },
                    label = { Text("Build Tx") },
                    selected = selectedTab == 1,
                    onClick = { selectedTab = 1 }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Edit, contentDescription = "Sign Tx") },
                    label = { Text("Sign Tx") },
                    selected = selectedTab == 2,
                    onClick = { selectedTab = 2 }
                )
            }
        }
    ) { innerPadding ->
        Box(modifier = Modifier.padding(innerPadding)) {
            when (selectedTab) {
                0 -> DiagnosticsScreen(
                    mwaActivityResultSender = mwaActivityResultSender,
                    mainSdk = sdk
                )
                1 -> TransactionBuilderScreen(sdk = sdk)
                2 -> SigningScreen(sdk = sdk)
            }
        }
    }
}