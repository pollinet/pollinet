package xyz.pollinet.android

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountBalanceWallet
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.*
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.launch
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import xyz.pollinet.android.mwa.PolliNetMwaClient
import xyz.pollinet.android.ui.DiagnosticsScreen
import xyz.pollinet.android.ui.SendScreen
import xyz.pollinet.android.ui.WalletScreen
import xyz.pollinet.android.ui.theme.PollinetandroidTheme
import xyz.pollinet.android.viewmodel.SendViewModel
import xyz.pollinet.android.viewmodel.WalletViewModel
import xyz.pollinet.android.BuildConfig
import xyz.pollinet.sdk.BleService
import xyz.pollinet.sdk.PolliNetSDK
import xyz.pollinet.sdk.SdkConfig

class MainActivity : ComponentActivity() {
    companion object {
        private const val TAG = "PolliNet.MainActivity"
    }

    private var bleService: BleService? = null
    private var isBound = false

    private val mwaActivityResultSender = ActivityResultSender(this)

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as? BleService.LocalBinder
            bleService = binder?.getService()
            isBound = true
            lifecycleScope.launch {
                bleService?.initializeSdk(
                    SdkConfig(
                        rpcUrl = "https://devnet.helius-rpc.com/?api-key=a8d3dc32-abdb-43b6-8638-74bd01d728a4",
                        enableLogging = true,
                        logLevel = "info",
                        storageDirectory = filesDir.absolutePath,
                        encryptionKey = BuildConfig.POLLINET_ENCRYPTION_KEY,
                    )
                )?.onFailure { e -> Log.e(TAG, "BLE Service SDK init failed: ${e.message}", e) }
            }
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            isBound = false
        }
    }

    private val requestPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        if (permissions.values.all { it }) startBleService()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            PollinetandroidTheme {
                PolliNetApp(mwaActivityResultSender = mwaActivityResultSender)
            }
        }
        requestBlePermissions()
    }

    private fun requestBlePermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_ADVERTISE)
        } else {
            arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }
        if (permissions.all { ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED }) {
            startBleService()
            requestBatteryOptimizationExemption()
        } else {
            requestPermissionLauncher.launch(permissions)
        }
    }

    private fun requestBatteryOptimizationExemption() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
            if (!pm.isIgnoringBatteryOptimizations(packageName)) {
                try {
                    startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                        .apply { data = Uri.parse("package:$packageName") })
                } catch (_: Exception) {}
            }
        }
    }

    private fun startBleService() {
        val intent = Intent(this, BleService::class.java).apply { action = BleService.ACTION_START }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) startForegroundService(intent)
            else startService(intent)
            bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start BLE service", e)
        }
    }

    override fun onDestroy() {
        if (isBound) unbindService(serviceConnection)
        super.onDestroy()
    }
}

@Composable
fun PolliNetApp(mwaActivityResultSender: ActivityResultSender) {
    var selectedTab by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()
    var sdk by remember { mutableStateOf<PolliNetSDK?>(null) }

    val walletViewModel: WalletViewModel = viewModel()
    val sendViewModel: SendViewModel = viewModel()

    val context = LocalContext.current

    // Initialize SDK
    LaunchedEffect(Unit) {
        scope.launch {
            PolliNetSDK.initialize(
                SdkConfig(
                    rpcUrl = "https://devnet.helius-rpc.com/?api-key=a8d3dc32-abdb-43b6-8638-74bd01d728a4",
                    enableLogging = true,
                    logLevel = "info",
                    storageDirectory = context.filesDir.absolutePath,
                    encryptionKey = BuildConfig.POLLINET_ENCRYPTION_KEY,
                )
            ).onSuccess { sdk = it }
                .onFailure { e -> android.util.Log.e("PolliNet", "SDK init failed: ${e.message}") }
        }
    }

    // MWA client (single instance for the whole app)
    val mwaClient = remember {
        PolliNetMwaClient.create(
            context = context,
            identityUri = "https://pollinet.xyz",
            iconUri = "icon.png",   // relative to identityUri — MWA requirement
            identityName = "Pollinet",
        )
    }

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.AccountBalanceWallet, contentDescription = "Wallet") },
                    label = { Text("Wallet") },
                    selected = selectedTab == 0,
                    onClick = { selectedTab = 0 },
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Send, contentDescription = "Send") },
                    label = { Text("Send") },
                    selected = selectedTab == 1,
                    onClick = { selectedTab = 1 },
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Filled.Settings, contentDescription = "Diagnostics") },
                    label = { Text("Dev") },
                    selected = selectedTab == 2,
                    onClick = { selectedTab = 2 },
                )
            }
        }
    ) { innerPadding ->
        Box(modifier = Modifier.padding(innerPadding)) {
            when (selectedTab) {
                0 -> WalletScreen(
                    sdk = sdk,
                    viewModel = walletViewModel,
                    mwaClient = mwaClient,
                    mwaSender = mwaActivityResultSender,
                    onWalletConnected = { pubkey ->
                        sendViewModel.setWallet(pubkey)
                    },
                )
                1 -> SendScreen(
                    sdk = sdk,
                    sendViewModel = sendViewModel,
                    walletViewModel = walletViewModel,
                    mwaClient = mwaClient,
                    mwaSender = mwaActivityResultSender,
                )
                2 -> DiagnosticsScreen(
                    mwaActivityResultSender = mwaActivityResultSender,
                    mainSdk = sdk,
                )
            }
        }
    }
}
