package xyz.pollinet.sdk

import android.annotation.SuppressLint
import android.app.*
import android.bluetooth.*
import android.bluetooth.le.*
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Base64
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.sync.Mutex
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentLinkedQueue
import kotlin.random.Random
import java.util.UUID as JavaUUID

/**
 * Foreground service for BLE operations.
 * 
 * This service:
 * - Handles BLE scanning, advertising, and connections
 * - Manages GATT server and client operations
 * - Bridges GATT callbacks to Rust FFI transport layer
 * - Maintains foreground status to survive background restrictions
 */
class BleService : Service() {
    
    companion object {
        private const val NOTIFICATION_ID = 1001
        private const val CHANNEL_ID = "pollinet_ble_service"
        
        // PolliNet UUIDs
        val SERVICE_UUID: JavaUUID = JavaUUID.fromString("00001820-0000-1000-8000-00805f9b34fb")
        val TX_CHAR_UUID: JavaUUID = JavaUUID.fromString("00001821-0000-1000-8000-00805f9b34fb")
        val RX_CHAR_UUID: JavaUUID = JavaUUID.fromString("00001822-0000-1000-8000-00805f9b34fb")
        
        const val ACTION_START = "xyz.pollinet.sdk.action.START"
        const val ACTION_STOP = "xyz.pollinet.sdk.action.STOP"
    }

    private val binder = LocalBinder()
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    
    // Bluetooth components
    private var bluetoothManager: BluetoothManager? = null
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var bleScanner: BluetoothLeScanner? = null
    private var bleAdvertiser: BluetoothLeAdvertiser? = null
    private var gattServer: BluetoothGattServer? = null
    private var connectedDevice: BluetoothDevice? = null
    private var gattCharacteristicTx: BluetoothGattCharacteristic? = null
    private var gattCharacteristicRx: BluetoothGattCharacteristic? = null
    private var clientGatt: BluetoothGatt? = null
    private var remoteTxCharacteristic: BluetoothGattCharacteristic? = null
    private var remoteRxCharacteristic: BluetoothGattCharacteristic? = null
    private val cccdUuid: JavaUUID = JavaUUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    private var remoteWriteInProgress = false
    
    // Sending state management
    private var sendingJob: Job? = null
    private val sendingMutex = Mutex()
    private val operationQueue = ConcurrentLinkedQueue<ByteArray>()
    private var operationInProgress = false
    
    // Retry logic for status 133
    private var descriptorWriteRetries = 0
    private val MAX_DESCRIPTOR_RETRIES = 3
    private val mainHandler = Handler(Looper.getMainLooper())
    
    // Autonomous transaction relay system
    private var autoSubmitJob: Job? = null
    private var cleanupJob: Job? = null
    
    // Bonding state receiver
    private val bondStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == BluetoothDevice.ACTION_BOND_STATE_CHANGED) {
                val device: BluetoothDevice? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE, BluetoothDevice::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE)
                }
                val bondState = intent.getIntExtra(BluetoothDevice.EXTRA_BOND_STATE, BluetoothDevice.ERROR)
                
                appendLog("🔐 Bond state changed for ${device?.address}: ${bondState.toBondStateString()}")
                
                // If bonding completed, retry connection
                if (bondState == BluetoothDevice.BOND_BONDED && device != null && device == clientGatt?.device) {
                    appendLog("✅ Bonding completed, retrying connection...")
                    mainHandler.postDelayed({
                        clientGatt?.connect()
                    }, 500)
                }
            }
        }
    }
    
    // SDK instance (exposed for testing)
    var sdk: PolliNetSDK? = null
        private set
    
    // State
    private val _connectionState = MutableStateFlow(ConnectionState.DISCONNECTED)
    val connectionState: StateFlow<ConnectionState> = _connectionState
    
    private val _metrics = MutableStateFlow<MetricsSnapshot?>(null)
    val metrics: StateFlow<MetricsSnapshot?> = _metrics

    private val _logs = MutableStateFlow<List<String>>(emptyList())
    val logs: StateFlow<List<String>> = _logs

    private val _isAdvertising = MutableStateFlow(false)
    val isAdvertising: StateFlow<Boolean> = _isAdvertising

    private val _isScanning = MutableStateFlow(false)
    val isScanning: StateFlow<Boolean> = _isScanning
    
    enum class ConnectionState {
        DISCONNECTED,
        SCANNING,
        CONNECTING,
        CONNECTED,
        ERROR
    }

    inner class LocalBinder : Binder() {
        fun getService(): BleService = this@BleService
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onCreate() {
        super.onCreate()
        android.util.Log.d("BleService", "onCreate: Starting BLE service initialization")
        
        createNotificationChannel()
        
        // Register bond state receiver for authentication/encryption support
        val bondFilter = IntentFilter(BluetoothDevice.ACTION_BOND_STATE_CHANGED)
        registerReceiver(bondStateReceiver, bondFilter)
        
        // Only start foreground if we have required permissions
        if (hasRequiredPermissions()) {
            android.util.Log.d("BleService", "onCreate: Permissions granted, starting foreground")
            startForeground()
            
            // Initialize Bluetooth asynchronously to avoid blocking onCreate
            serviceScope.launch {
                try {
                    android.util.Log.d("BleService", "onCreate: Initializing Bluetooth")
                    initializeBluetooth()
                    android.util.Log.d("BleService", "onCreate: Bluetooth initialized successfully")
                } catch (e: Exception) {
                    android.util.Log.e("BleService", "onCreate: Failed to initialize Bluetooth", e)
                    _connectionState.value = ConnectionState.ERROR
                }
            }
            
            // Start metrics collection
            serviceScope.launch {
                while (isActive) {
                    sdk?.metrics()?.getOrNull()?.let { _metrics.value = it }
                    delay(1000) // Update every second
                }
            }
        } else {
            android.util.Log.w("BleService", "onCreate: Missing required permissions, stopping service")
            // Stop the service if permissions aren't granted
            stopSelf()
        }
        
        android.util.Log.d("BleService", "onCreate: Completed")
    }

    fun clearLogs() {
        _logs.value = emptyList()
    }

    fun queueSampleTransaction(byteSize: Int = 1024) {
        val sdkInstance = sdk ?: run {
            appendLog("⚠️ SDK not initialized; cannot queue sample transaction")
            return
        }

        serviceScope.launch {
            appendLog("🧪 Queueing sample transaction (${byteSize} bytes)")
            val payload = ByteArray(byteSize) { Random.nextInt(0, 256).toByte() }
            sdkInstance.fragment(payload).onSuccess { fragments ->
                val count = fragments.fragments.size
                val txId = fragments.fragments.firstOrNull()?.id ?: "unknown"
                val firstFragmentData = fragments.fragments.firstOrNull()?.data;
                val firstFragmentType = fragments.fragments.firstOrNull()?.fragmentType
                appendLog("📤 Queued $count fragments for tx ${txId.take(12)}…")
                appendLog(" Fragment Data: ${firstFragmentData?.take(12)}…")
                appendLog(" Fragment Type: $firstFragmentType")

                // Start sending loop if not already running
                ensureSendingLoopStarted()
            }.onFailure {
                appendLog("❌ Failed to queue sample transaction: ${it.message}")
            }
        }
    }

    fun queueTransactionFromBase64(base64: String) {
        val trimmed = base64.trim()
        if (trimmed.isEmpty()) {
            appendLog("⚠️ Provided transaction is empty")
            return
        }

        val sdkInstance = sdk ?: run {
            appendLog("⚠️ SDK not initialized; cannot queue transaction")
            return
        }

        serviceScope.launch {
            try {
                val bytes = Base64.decode(trimmed, Base64.DEFAULT)
                appendLog("🧾 Queueing provided transaction (${bytes.size} bytes)")
                sdkInstance.fragment(bytes).onSuccess { fragments ->
                    val count = fragments.fragments.size
                    val txId = fragments.fragments.firstOrNull()?.id ?: "unknown"
                    appendLog("📤 Queued $count fragments for tx ${txId.take(12)}…")
                    
                    // Start sending loop if not already running
                    ensureSendingLoopStarted()
                }.onFailure {
                    appendLog("❌ Failed to queue provided transaction: ${it.message}")
                }
            } catch (e: IllegalArgumentException) {
                appendLog("❌ Invalid base64 input: ${e.message}")
            }
        }
    }

    fun debugQueueStatus() {
        serviceScope.launch {
            val sdkInstance = sdk ?: run {
                appendLog("⚠️ SDK not initialized")
                return@launch
            }
            
            appendLog("🔍 === DIAGNOSTIC STATUS ===")
            appendLog("🔍 Connection: ${_connectionState.value}")
            appendLog("🔍 Sending job active: ${sendingJob?.isActive}")
            appendLog("🔍 Write in progress: $remoteWriteInProgress")
            appendLog("🔍 Client GATT: ${clientGatt != null}")
            appendLog("🔍 Remote RX char: ${remoteRxCharacteristic != null}")
            appendLog("🔍 GATT server: ${gattServer != null}")
            appendLog("🔍 GATT server TX char: ${gattCharacteristicTx != null}")
            appendLog("🔍 Connected device: ${connectedDevice?.address}")
            
            // Non-destructive queue peek
            sdkInstance.getOutboundQueueSize().onSuccess { size ->
                appendLog("📊 Outbound queue: $size fragments")
                
                if (size > 0) {
                    sdkInstance.debugOutboundQueue().onSuccess { queueDebug ->
                        appendLog("📦 Queue details:")
                        queueDebug.fragments.forEach { frag ->
                            appendLog("  [${frag.index}] ${frag.size} bytes")
                        }
                    }
                } else {
                    appendLog("📭 Queue is empty")
                }
            }.onFailure { e ->
                appendLog("❌ Failed to get queue size: ${e.message}")
            }
            
            appendLog("🔍 === END DIAGNOSTIC ===")
        }
    }
    
    private fun hasRequiredPermissions(): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // Android 12+
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_SCAN) == PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_ADVERTISE) == PackageManager.PERMISSION_GRANTED
        } else {
            // Android 10-11
            checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                // Service already started in onCreate
            }
            ACTION_STOP -> {
                stopSelf()
            }
        }
        return START_STICKY
    }

    // =========================================================================
    // Autonomous Transaction Relay System
    // =========================================================================

    /**
     * Start the autonomous transaction auto-submission loop
     * This loop continuously monitors for received transactions and auto-submits them
     */
    private fun startAutoSubmitLoop() {
        if (autoSubmitJob?.isActive == true) {
            appendLog("🔄 Auto-submit loop already running")
            return
        }
        
        appendLog("🚀 Starting autonomous transaction relay system")
        
        autoSubmitJob = serviceScope.launch {
            while (isActive) {
                try {
                    val sdkInstance = sdk ?: continue
                    
                    // Get next received transaction
                    val result = sdkInstance.nextReceivedTransaction()
                    val receivedTx = result.getOrNull()
                    
                    if (receivedTx != null) {
                        appendLog("📥 Processing received transaction: ${receivedTx.txId}")
                        
                        // Decode transaction bytes
                        val txBytes = Base64.decode(receivedTx.transactionBase64, Base64.DEFAULT)
                        
                        // Check if we have internet
                        if (hasInternetConnection()) {
                            // Submit transaction
                            appendLog("🌐 Internet available, submitting transaction: ${receivedTx.txId}")
                            
                            try {
                                val submitResult = sdkInstance.submitOfflineTransaction(
                                    transactionBase64 = receivedTx.transactionBase64,
                                    verifyNonce = false  // Don't verify for received transactions
                                )
                                
                                submitResult.onSuccess { signature ->
                                    appendLog("✅ Auto-submitted transaction: ${receivedTx.txId}")
                                    appendLog("   Signature: $signature")
                                    
                                    // Mark as submitted for deduplication
                                    sdkInstance.markTransactionSubmitted(txBytes)
                                }.onFailure { e ->
                                    appendLog("⚠️ Failed to submit transaction ${receivedTx.txId}: ${e.message}")
                                    // Requeue for retry by pushing it back
                                    sdkInstance.pushReceivedTransaction(txBytes)
                                }
                            } catch (e: Exception) {
                                appendLog("❌ Error submitting transaction ${receivedTx.txId}: ${e.message}")
                                // Requeue for retry
                                sdkInstance.pushReceivedTransaction(txBytes)
                            }
                        } else {
                            // No internet, relay to mesh
                            appendLog("📡 No internet, relaying transaction ${receivedTx.txId} to mesh")
                            
                            // Queue for BLE transmission to other peers (re-fragment)
                            try {
                                val fragmentResult = sdkInstance.fragmentTransaction(txBytes)
                                fragmentResult.onSuccess { fragmentDataList ->
                                    appendLog("📤 Queued ${fragmentDataList.size} fragments for mesh relay")
                                    // The fragments are already in the outbound queue
                                    ensureSendingLoopStarted()
                                }.onFailure { e ->
                                    appendLog("⚠️ Failed to queue for relay: ${e.message}")
                                    // Requeue for later
                                    sdkInstance.pushReceivedTransaction(txBytes)
                                }
                            } catch (e: Exception) {
                                appendLog("⚠️ Exception while queueing relay: ${e.message}")
                                // Requeue for later
                                sdkInstance.pushReceivedTransaction(txBytes)
                            }
                        }
                    }
                    
                    // Check every 2 seconds
                    delay(2000)
                    
                } catch (e: Exception) {
                    appendLog("❌ Auto-submit loop error: ${e.message}")
                    delay(5000) // Wait longer on error
                }
            }
        }
        
        // Also start cleanup job
        startCleanupJob()
    }

    /**
     * Start periodic cleanup of old submission hashes
     */
    private fun startCleanupJob() {
        if (cleanupJob?.isActive == true) {
            return
        }
        
        cleanupJob = serviceScope.launch {
            while (isActive) {
                try {
                    sdk?.cleanupOldSubmissions()
                    delay(3600_000) // Run every hour
                } catch (e: Exception) {
                    appendLog("⚠️ Cleanup job error: ${e.message}")
                }
            }
        }
    }

    /**
     * Check if device has internet connectivity
     */
    @SuppressLint("MissingPermission")
    private fun hasInternetConnection(): Boolean {
        return try {
            val connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) 
                as? ConnectivityManager ?: return false
            
            val network = connectivityManager.activeNetwork ?: return false
            val capabilities = connectivityManager.getNetworkCapabilities(network) 
                ?: return false
            
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
        } catch (e: Exception) {
            appendLog("⚠️ Error checking internet: ${e.message}")
            false
        }
    }

    /**
     * Handle incoming packet - reassemble and queue for auto-submission
     */
    private suspend fun handleReceivedData(data: ByteArray) {
        try {
            appendLog("📥 Received data: ${data.size} bytes")
            
            // Push to SDK for reassembly
            // The pushInbound call will handle fragmentation internally
            // Completed transactions will be picked up by the auto-submit loop
            val result = sdk?.pushInbound(data)
            result?.onSuccess {
                appendLog("✅ Fragment processed and added to reassembly buffer")
                
                // Check if we have completed transactions
                val queueSize = sdk?.getReceivedQueueSize()?.getOrNull() ?: 0
                if (queueSize > 0) {
                    appendLog("🎉 Transaction reassembly complete! Queue size: $queueSize")
                }
            }?.onFailure { e ->
                appendLog("⚠️ Error processing fragment: ${e.message}")
            }
        } catch (e: Exception) {
            appendLog("❌ Error in handleReceivedData: ${e.message}")
        }
    }

    @SuppressLint("MissingPermission")
    override fun onUnbind(intent: Intent?): Boolean {
        // Close GATT connection when activity unbinds
        // This is critical to avoid battery drain per Android docs
        closeGattConnection()
        return super.onUnbind(intent)
    }
    
    override fun onDestroy() {
        // Cancel all coroutine jobs
        autoSubmitJob?.cancel()
        cleanupJob?.cancel()
        sendingJob?.cancel()
        serviceScope.cancel()
        
        // Unregister bond state receiver
        try {
            unregisterReceiver(bondStateReceiver)
        } catch (e: IllegalArgumentException) {
            // Receiver was not registered
        }
        
        // Stop BLE operations
        stopScanning()
        stopAdvertising()
        closeGattConnection()
        gattServer?.close()
        sdk?.shutdown()
        super.onDestroy()
    }
    
    /**
     * Close GATT connection properly to avoid battery drain
     * Per Android documentation: https://developer.android.com/develop/connectivity/bluetooth/ble/connect-gatt-server#close-gatt-connection
     * 
     * CRITICAL: Must call disconnect() before close() per Android best practices
     */
    @SuppressLint("MissingPermission")
    private fun closeGattConnection() {
        clientGatt?.let { gatt ->
            appendLog("🔌 Disconnecting and closing GATT connection to ${gatt.device.address}")
            // Official Android sample shows: disconnect() -> close()
            // This ensures proper cleanup and prevents battery drain
            gatt.disconnect()
            gatt.close()
            clientGatt = null
        }
    }

    /**
     * Initialize the PolliNet SDK
     */
    suspend fun initializeSdk(config: SdkConfig): Result<Unit> {
        return PolliNetSDK.initialize(config).map { 
            sdk = it
            // Start autonomous transaction relay system
            startAutoSubmitLoop()
            appendLog("🚀 Autonomous relay system started")
        }
    }

    /**
     * Start BLE scanning for PolliNet devices
     */
    @SuppressLint("MissingPermission")
    fun startScanning() {
        bleScanner?.let { scanner ->
            appendLog("🔍 Starting BLE scan")
            val scanFilter = ScanFilter.Builder()
                .setServiceUuid(android.os.ParcelUuid(SERVICE_UUID))
                .build()
            
            val scanSettings = ScanSettings.Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
                .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
                .build()
            
            scanner.startScan(listOf(scanFilter), scanSettings, scanCallback)
            _connectionState.value = ConnectionState.SCANNING
            _isScanning.value = true
        }
            ?: appendLog("⚠️ BLE scanner unavailable")
    }

    /**
     * Stop BLE scanning
     */
    @SuppressLint("MissingPermission")
    fun stopScanning() {
        bleScanner?.stopScan(scanCallback)
        if (_isScanning.value) {
            appendLog("🛑 Stopped BLE scan")
        }
        _isScanning.value = false
    }

    /**
     * Start BLE advertising
     */
    @SuppressLint("MissingPermission")
    fun startAdvertising() {
        bleAdvertiser?.let { advertiser ->
            appendLog("📣 Starting advertising")
            val settings = AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
                .setConnectable(true)
                .setTimeout(0)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
                .build()
            
            val data = AdvertiseData.Builder()
                .setIncludeDeviceName(false)
                .addServiceUuid(android.os.ParcelUuid(SERVICE_UUID))
                .build()
            
            advertiser.startAdvertising(settings, data, advertiseCallback)
            _isAdvertising.value = true
        } ?: appendLog("⚠️ BLE advertiser unavailable")
    }

    /**
     * Stop BLE advertising
     */
    @SuppressLint("MissingPermission")
    fun stopAdvertising() {
        bleAdvertiser?.stopAdvertising(advertiseCallback)
        if (_isAdvertising.value) {
            appendLog("🛑 Stopped advertising")
        }
        _isAdvertising.value = false
    }

    /**
     * Push inbound data to the transport layer (for testing)
     */
    suspend fun pushInboundData(data: ByteArray) {
        val sdkInstance = sdk ?: run {
            appendLog("⚠️ SDK not initialized; inbound test data dropped")
            return
        }
        sdkInstance.pushInbound(data).onSuccess {
            appendLog("⬅️ Inbound test data (${previewFragment(data)})")
        }.onFailure {
            appendLog("❌ Failed to process inbound test data: ${it.message}")
        }
    }

    /**
     * Connect to a discovered device
     */
    @SuppressLint("MissingPermission")
    fun connectToDevice(device: BluetoothDevice) {
        _connectionState.value = ConnectionState.CONNECTING
        appendLog("🔗 Connecting to ${device.address}")
        device.connectGatt(this, false, gattCallback)
    }

    /**
     * Ensure the sending loop is started
     */
    private fun ensureSendingLoopStarted() {
        if (sendingJob?.isActive == true) {
            appendLog("🔄 Sending loop already active")
            return
        }
        
        if (_connectionState.value != ConnectionState.CONNECTED) {
            appendLog("⚠️ Not connected - fragments will be sent when connection established")
            return
        }
        
        appendLog("🚀 Starting sending loop")
        sendingJob = serviceScope.launch {
            while (_connectionState.value == ConnectionState.CONNECTED) {
                sendNextOutbound()
                delay(500) // Increased delay for BLE stability (was 500ms)
            }
            appendLog("🛑 Sending loop stopped")
        }
    }

    /**
     * Attempt to send the next outbound fragment
     */
    private suspend fun sendNextOutbound() {
        sendingMutex.lock()
        try {
            if (operationInProgress) {
                // Operation already in progress, skip
                return
            }

            val sdkInstance = sdk ?: run {
                appendLog("⚠️ sendNextOutbound: SDK is null")
                return
            }
            
            // BLE safe fragment size:
            // Target max 150 bytes to ensure reliable transmission
            // This matches the Rust fragmentation (52 bytes data + headers + bincode overhead)
            val data = sdkInstance.nextOutbound(maxLen = 150)
            
            if (data == null) {
                // No more data to send - this is normal
                return
            }

            appendLog("➡️ Sending fragment (${data.size}B)")
            
            // Send directly - no queue needed with proper GATT callbacks
            sendToGatt(data)
        } catch (e: Exception){
            appendLog("❌ Exception in sendNextOutbound: ${e.message}")
        } finally {
            sendingMutex.unlock()
        }
    }

    @SuppressLint("MissingPermission")
    private fun sendToGatt(data: ByteArray) {
        // Based on official Android sample (lines 184-202)
        // Try server/peripheral path first
        val server = gattServer
        val txChar = gattCharacteristicTx
        val device = connectedDevice
        
        if (server != null && txChar != null && device != null) {
            txChar.value = data
            val success = server.notifyCharacteristicChanged(device, txChar, false)
            appendLog(if (success) "✅ Sent ${data.size}B via notify" else "❌ Notify failed")
            return
        }

        // Try client/central path
        val gatt = clientGatt
        val remoteRx = remoteRxCharacteristic
        
        if (gatt == null || remoteRx == null) {
            appendLog("⚠️ GATT or RX characteristic not available")
            return
        }

        // Mark operation in progress for client writes
        operationInProgress = true
        
        // Use official sample's write pattern (Android 13+ vs older)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val result = gatt.writeCharacteristic(
                remoteRx,
                data,
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            )
            appendLog("✅ Wrote ${data.size}B (result=$result)")
            if (result != BluetoothGatt.GATT_SUCCESS) {
                operationInProgress = false
            }
        } else {
            remoteRx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            @Suppress("DEPRECATION")
            remoteRx.value = data
            @Suppress("DEPRECATION")
            val success = gatt.writeCharacteristic(remoteRx)
            appendLog(if (success) "✅ Wrote ${data.size}B" else "❌ Write failed")
            if (!success) {
                operationInProgress = false
            }
        }
    }

    private fun completeRemoteWrite() {
        if (remoteWriteInProgress) {
            remoteWriteInProgress = false
            appendLog("✅ Write complete, ready for next")
            // Don't manually trigger send here - the loop will handle it
        }
    }

    // =========================================================================
    // Bluetooth initialization
    // =========================================================================

    private fun initializeBluetooth() {
        android.util.Log.d("BleService", "initializeBluetooth: Getting Bluetooth manager")
        bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
        
        if (bluetoothManager == null) {
            android.util.Log.e("BleService", "initializeBluetooth: Failed to get BluetoothManager")
            throw IllegalStateException("BluetoothManager not available")
        }
        
        bluetoothAdapter = bluetoothManager?.adapter
        if (bluetoothAdapter == null) {
            android.util.Log.e("BleService", "initializeBluetooth: BluetoothAdapter is null")
            throw IllegalStateException("BluetoothAdapter not available")
        }
        
        if (!bluetoothAdapter!!.isEnabled) {
            android.util.Log.w("BleService", "initializeBluetooth: Bluetooth is not enabled")
        }
        
        bleScanner = bluetoothAdapter?.bluetoothLeScanner
        bleAdvertiser = bluetoothAdapter?.bluetoothLeAdvertiser
        
        android.util.Log.d("BleService", "initializeBluetooth: Setting up GATT server")
        setupGattServer()
        android.util.Log.d("BleService", "initializeBluetooth: GATT server setup complete")
        appendLog("✅ Bluetooth initialized")
    }

    @SuppressLint("MissingPermission")
    private fun setupGattServer() {
        try {
            android.util.Log.d("BleService", "setupGattServer: Creating GATT service")
            val service = BluetoothGattService(
                SERVICE_UUID,
                BluetoothGattService.SERVICE_TYPE_PRIMARY
            )

            // TX characteristic (server -> client)
            gattCharacteristicTx = BluetoothGattCharacteristic(
                TX_CHAR_UUID,
                BluetoothGattCharacteristic.PROPERTY_NOTIFY,
                BluetoothGattCharacteristic.PERMISSION_READ
            ).apply {
                addDescriptor(
                    BluetoothGattDescriptor(
                        JavaUUID.fromString("00002902-0000-1000-8000-00805f9b34fb"),
                        BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE
                    )
                )
            }

            // RX characteristic (client -> server)
            gattCharacteristicRx = BluetoothGattCharacteristic(
                RX_CHAR_UUID,
                BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
                BluetoothGattCharacteristic.PERMISSION_WRITE
            )

            service.addCharacteristic(gattCharacteristicTx)
            service.addCharacteristic(gattCharacteristicRx)

            android.util.Log.d("BleService", "setupGattServer: Opening GATT server")
            gattServer = bluetoothManager?.openGattServer(this, gattServerCallback)
            
            if (gattServer == null) {
                android.util.Log.e("BleService", "setupGattServer: Failed to open GATT server")
                throw IllegalStateException("Failed to open GATT server")
            }
            
            android.util.Log.d("BleService", "setupGattServer: Adding service to GATT server")
            val result = gattServer?.addService(service)
            android.util.Log.d("BleService", "setupGattServer: Service added, result=$result")
        } catch (e: Exception) {
            android.util.Log.e("BleService", "setupGattServer: Exception occurred", e)
            throw e
        }
    }

    // =========================================================================
    // BLE Callbacks
    // =========================================================================

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            // Auto-connect to first discovered PolliNet device
            stopScanning()
            appendLog("📡 Discovered device ${result.device.address} (${result.rssi})")
            connectToDevice(result.device)
        }

        override fun onScanFailed(errorCode: Int) {
            _connectionState.value = ConnectionState.ERROR
            appendLog("❌ Scan failed (code $errorCode)")
        }
    }

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            // Advertising started successfully
            appendLog("✅ Advertising started (mode=${settingsInEffect.mode})")
        }

        override fun onStartFailure(errorCode: Int) {
            _connectionState.value = ConnectionState.ERROR
            _isAdvertising.value = false
            appendLog("❌ Advertising failed (code $errorCode)")
        }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        @SuppressLint("MissingPermission")
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            // Based on official Android ConnectGATTSample
            // https://github.com/android/platform-samples/blob/main/samples/connectivity/bluetooth/ble/ConnectGATTSample.kt
            
            appendLog("🔄 Connection state change: status=$status, newState=$newState")
            
            // Handle error statuses - per official sample (lines 254-261)
            if (status != BluetoothGatt.GATT_SUCCESS) {
                appendLog("❌ Connection error: status=$status")
                when (status) {
                    5, 15 -> {
                        // GATT_INSUFFICIENT_AUTHENTICATION or GATT_INSUFFICIENT_ENCRYPTION
                        appendLog("🔐 Authentication/Encryption required - creating bond...")
                        gatt.device.createBond()
                    }
                    133 -> {
                        // GATT_ERROR - Try cache refresh
                        appendLog("⚠️ GATT_ERROR - refreshing cache and retrying...")
                        refreshDeviceCache(gatt)
                        gatt.close()
                        clientGatt = null
                    }
                    else -> {
                        appendLog("❌ Error: See https://developer.android.com/reference/android/bluetooth/BluetoothGatt")
                    }
                }
                _connectionState.value = ConnectionState.DISCONNECTED
                return
            }
            
            // Handle connection states
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    _connectionState.value = ConnectionState.CONNECTED
                    connectedDevice = gatt.device
                    clientGatt = gatt
                    appendLog("✅ Connected to ${gatt.device.address}")
                    
                    // Request MTU for better throughput (official sample line 137)
                    // Note: Android 14+ sets default MTU automatically
                    appendLog("📏 Requesting MTU (247 bytes)...")
                    gatt.requestMtu(247)
                    // Service discovery happens in onMtuChanged
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    _connectionState.value = ConnectionState.DISCONNECTED
                    appendLog("🔌 Disconnected from ${gatt.device.address}")
                    
                    // Clean up
                    connectedDevice = null
                    clientGatt = null
                    remoteTxCharacteristic = null
                    remoteRxCharacteristic = null
                    remoteWriteInProgress = false
                    operationInProgress = false
                    operationQueue.clear()
                    descriptorWriteRetries = 0
                    sendingJob?.cancel()
                }
            }
        }
        
        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            appendLog("📏 MTU changed to $mtu (status=$status)")
            
            // Now discover services (official sample pattern)
            appendLog("🔍 Discovering services...")
            gatt.discoverServices()
        }

        @SuppressLint("MissingPermission")
        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            // Based on official Android ConnectGATTSample (line 270-274)
            appendLog("📋 Services discovered: status=$status")
            
            if (status != BluetoothGatt.GATT_SUCCESS) {
                appendLog("❌ Service discovery failed")
                return
            }
            
            // Find our service
            val service = gatt.getService(SERVICE_UUID)
            if (service == null) {
                appendLog("⚠️ PolliNet service not found")
                appendLog("   Available: ${gatt.services.map { it.uuid }}")
                return
            }
            
            appendLog("✅ Service found: $SERVICE_UUID")
            
            // Get characteristics
            remoteTxCharacteristic = service.getCharacteristic(TX_CHAR_UUID)
            remoteRxCharacteristic = service.getCharacteristic(RX_CHAR_UUID)
            
            if (remoteTxCharacteristic == null || remoteRxCharacteristic == null) {
                appendLog("❌ Missing characteristics")
                return
            }
            
            appendLog("✅ Characteristics ready")
            appendLog("   TX: $TX_CHAR_UUID")
            appendLog("   RX: $RX_CHAR_UUID")
            
            // Enable notifications on TX characteristic
            gatt.setCharacteristicNotification(remoteTxCharacteristic, true)
            
            // Write CCCD to enable remote notifications
            val descriptor = remoteTxCharacteristic?.getDescriptor(cccdUuid)
            if (descriptor != null) {
                descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                gatt.writeDescriptor(descriptor)
                appendLog("📬 Enabling notifications...")
            } else {
                appendLog("⚠️ CCCD descriptor not found")
            }
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray
        ) {
            // Forward to Rust FFI
            serviceScope.launch {
                if (sdk == null) {
                    appendLog("⚠️ SDK not initialized; inbound dropped")
                    return@launch
                }
                appendLog("⬅️ Received: ${previewFragment(value)}")
                handleReceivedData(value)
            }
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int
        ) {
            if (characteristic.uuid == RX_CHAR_UUID) {
                operationInProgress = false
                
                if (status == BluetoothGatt.GATT_SUCCESS) {
                    completeRemoteWrite()
                    // Process next operation in queue
                    processOperationQueue()
                } else {
                    remoteWriteInProgress = false
                    appendLog("❌ Write failed with status $status")
                    
                    if (status == 133) {
                        handleStatus133(gatt)
                    } else {
                        // Process next operation anyway
                        processOperationQueue()
                    }
                }
            }
        }

        @SuppressLint("MissingPermission")
        override fun onDescriptorWrite(
            gatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
            status: Int
        ) {
            // Simple handling like official sample
            appendLog("📝 Descriptor write: status=$status")
            
            if (status == BluetoothGatt.GATT_SUCCESS) {
                appendLog("✅ Notifications enabled - ready to transfer data!")
                // Start sending loop
                ensureSendingLoopStarted()
            } else {
                appendLog("❌ Failed to enable notifications: status=$status")
            }
        }
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        @SuppressLint("MissingPermission")
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    _connectionState.value = ConnectionState.CONNECTED
                    connectedDevice = device
                    appendLog("🤝 (Server) connected ${device.address}")
                    // Start sending loop for server mode
                    ensureSendingLoopStarted()
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    _connectionState.value = ConnectionState.DISCONNECTED
                    connectedDevice = null
                    sendingJob?.cancel()
                    appendLog("🔌 (Server) disconnected ${device.address}")
                }
            }
        }

        @SuppressLint("MissingPermission")
        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray
        ) {
            if (characteristic.uuid == RX_CHAR_UUID) {
                // Forward to Rust FFI
                serviceScope.launch {
                    if (sdk == null) {
                        appendLog("⚠️ SDK not initialized; write dropped")
                        return@launch
                    }
                    appendLog("⬅️ RX from ${device.address}: ${previewFragment(value)}")
                    handleReceivedData(value)
                }
                
                if (responseNeeded) {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                }
            }
        }
    }

    // =========================================================================
    // Foreground service notification
    // =========================================================================

    private fun startForeground() {
        val notification = createNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "PolliNet BLE Service",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Manages PolliNet Bluetooth connections"
            }
            
            val notificationManager = getSystemService(NotificationManager::class.java)
            notificationManager.createNotificationChannel(channel)
        }
    }

    private fun createNotification(): Notification {
        val stopIntent = Intent(this, BleService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this,
            0,
            stopIntent,
            PendingIntent.FLAG_IMMUTABLE
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("PolliNet Active")
            .setContentText("Managing BLE connections")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setOngoing(true)
            .addAction(android.R.drawable.ic_delete, "Stop", stopPendingIntent)
            .build()
    }

    /**
     * Refresh GATT cache - critical for recovering from status 133
     */
    private fun refreshDeviceCache(gatt: BluetoothGatt): Boolean {
        return try {
            val refresh = gatt.javaClass.getMethod("refresh")
            val result = refresh.invoke(gatt) as Boolean
            appendLog("🔄 GATT cache refresh: $result")
            result
        } catch (e: Exception) {
            appendLog("❌ Failed to refresh cache: ${e.message}")
            false
        }
    }
    
    /**
     * Handle status 133 error - disconnect, clear cache, and retry
     */
    @SuppressLint("MissingPermission")
    private fun handleStatus133(gatt: BluetoothGatt) {
        appendLog("⚠️ Handling status 133 - clearing cache and reconnecting")
        refreshDeviceCache(gatt)
        gatt.close()
        clientGatt = null
        _connectionState.value = ConnectionState.DISCONNECTED
        
        // Retry connection after delay
        val device = gatt.device
        mainHandler.postDelayed({
            appendLog("🔄 Retrying connection after status 133...")
            try {
                device.connectGatt(this, false, gattCallback)
            } catch (e: Exception) {
                appendLog("❌ Retry connection failed: ${e.message}")
            }
        }, 1000)
    }
    
    /**
     * Process the operation queue - ensures only one BLE operation at a time
     */
    @SuppressLint("MissingPermission")
    private fun processOperationQueue() {
        if (operationInProgress || operationQueue.isEmpty()) return
        
        val data = operationQueue.poll() ?: return
        operationInProgress = true
        
        appendLog("📤 Processing queued operation (${data.size} bytes)")
        sendToGatt(data)
    }

    private fun appendLog(message: String) {
        val timestamp = SimpleDateFormat("HH:mm:ss", Locale.getDefault()).format(Date())
        val entry = "[$timestamp] $message"
        val current = _logs.value
        _logs.value = (current + entry).takeLast(50)
    }

    private fun previewFragment(data: ByteArray): String {
        return try {
            val text = String(data, Charsets.UTF_8)
            when {
                text.isBlank() -> "empty JSON"
                text.length > 160 -> text.take(160) + "…"
                else -> text
            }
        } catch (e: Exception) {
            "${data.size} bytes (binary)"
        }
    }
    
    /**
     * Convert bond state integer to human-readable string
     */
    private fun Int.toBondStateString() = when (this) {
        BluetoothDevice.BOND_NONE -> "BOND_NONE"
        BluetoothDevice.BOND_BONDING -> "BOND_BONDING"
        BluetoothDevice.BOND_BONDED -> "BOND_BONDED"
        else -> "UNKNOWN ($this)"
    }
}