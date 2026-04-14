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
import android.os.PowerManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.BatteryManager
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Base64
import androidx.core.app.NotificationCompat
import androidx.work.WorkManager
import xyz.pollinet.sdk.workers.RetryWorker
import xyz.pollinet.sdk.workers.CleanupWorker
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.sync.Mutex
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.random.Random
import java.util.UUID as JavaUUID
import kotlinx.serialization.json.Json
import kotlinx.serialization.encodeToString

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
        
        // Queue size limits (Edge Case Fix #3)
        // Prevents OutOfMemoryError from unbounded queue growth
        private const val MAX_OPERATION_QUEUE_SIZE = 100
        private const val MAX_FRAGMENT_SIZE = 512 // bytes (documentation)
        
        // Transaction size limits (Edge Case Fix #5)
        // Prevents OutOfMemoryError and DOS attacks from oversized transactions
        // Reasonable limit: ~10 fragments at 512 bytes each = 5120 bytes (~5KB)
        // Solana transaction max is 1232 bytes, so 5KB provides comfortable headroom
        private const val MAX_TRANSACTION_SIZE = 5120 // bytes (~5KB)

        // Maximum number of times this device will relay the same transaction over BLE.
        // Prevents dead/orphaned transactions from circulating indefinitely when all
        // devices in the mesh lack internet.  Mirrors the confirmation relay cap below.
        private const val MAX_TX_RELAY_HOPS = 5
    }

    private val binder = LocalBinder()
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    
    // JSON serializer for confirmations
    private val json = Json { 
        ignoreUnknownKeys = true
        encodeDefaults = true
    }
    
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
    
    // MTU tracking for dynamic payload sizing
    // Start with a safe assumption of 185 bytes (common for Android BLE)
    // This will be updated when MTU negotiation completes
    // Using 23 (minimum) causes tiny 13-byte fragments before negotiation
    private var currentMtu: Int = 185 // Reasonable default, updated on negotiation
    
    // Sending state management
    private var sendingJob: Job? = null
    private val sendingMutex = Mutex()
    private val operationQueue = ConcurrentLinkedQueue<ByteArray>()
    
    // Connection management - decoupled from discovery
    private var pendingConnectionDevice: BluetoothDevice? = null
    private var connectionAttemptTime: Long = 0
    private val CONNECTION_TIMEOUT_MS = 10_000L // 10 seconds timeout for connection attempts
    // Edge Case Fix #8: Use AtomicBoolean to prevent race conditions
    // Prevents concurrent BLE operations that cause status 133 errors
    private val operationInProgress = AtomicBoolean(false)
    
    /**
     * Record a discovered or updated peer in the in-memory map and in the Rust health monitor.
     * Safe to call from any thread (uses serviceScope for the FFI suspend calls).
     */
    private fun recordPeer(address: String, rssi: Int, connected: Boolean) {
        val now = System.currentTimeMillis()
        _peers.value = _peers.value.toMutableMap().apply {
            val existing = get(address)
            put(address, DiscoveredPeer(
                address      = address,
                rssi         = rssi,
                discoveredAt = existing?.discoveredAt ?: now,
                isConnected  = connected,
                lastSeenAt   = now
            ))
        }
        val sdkHandle = sdk ?: return
        serviceScope.launch {
            if (connected) sdkHandle.recordPeerHeartbeat(address)
            sdkHandle.recordPeerRssi(address, rssi)
        }
    }

    /**
     * Safely add fragment to operation queue with overflow protection.
     * When the queue is full we cancel the active sending loop and clear the queue
     * so the in-progress transaction can be retried cleanly — silent mid-stream
     * drops would corrupt fragment reassembly on the receiver side.
     */
    private fun safelyQueueFragment(data: ByteArray, context: String = "") {
        // Synchronize the check-cancel-clear-offer sequence so no concurrent caller
        // can slip a fragment in between our overflow check and the queue.clear().
        synchronized(operationQueue) {
            if (operationQueue.size >= MAX_OPERATION_QUEUE_SIZE) {
                appendLog("🚨 Operation queue full ($MAX_OPERATION_QUEUE_SIZE) — cancelling send loop and clearing queue to avoid mid-stream corruption")
                appendLog("   Context: $context")
                appendLog("   This indicates the BLE stack is too slow or the connection is degraded")
                sendingJob?.cancel()
                operationQueue.clear()
                // Reset in-progress flag so the loop can restart cleanly
                operationInProgress.set(false)
            }
            operationQueue.offer(data)
        }
        appendLog("📦 Queued fragment (${data.size}B), queue size: ${operationQueue.size}/$MAX_OPERATION_QUEUE_SIZE")
    }
    
    // Track original transaction bytes for re-fragmentation when MTU changes
    private var pendingTransactionBytes: ByteArray? = null
    private var fragmentsQueuedWithMtu: Int = 0
    
    // Track if we're ready to send (descriptor write completed)
    private var descriptorWriteComplete = false
    
    // Retry logic for status 133
    private var descriptorWriteRetries = 0
    private val MAX_DESCRIPTOR_RETRIES = 3
    private val mainHandler = Handler(Looper.getMainLooper())
    private var pendingDescriptorWrite: BluetoothGattDescriptor? = null
    private var pendingGatt: BluetoothGatt? = null
    private var pendingDescriptorRetry: Runnable? = null // token for cancellation
    
    // Autonomous transaction relay system
    private var autoSubmitJob: Job? = null
    private var cleanupJob: Job? = null
    
    // =========================================================================
    // Phase 4: Event-Driven Architecture (Battery Optimization)
    // =========================================================================
    
    /**
     * Work event types for event-driven processing
     * Replaces multiple polling loops with single event-driven worker
     */
    sealed class WorkEvent {
        object OutboundReady : WorkEvent()      // Transaction queued for transmission
        object ReceivedReady : WorkEvent()      // Transaction received and reassembled
        object RetryReady : WorkEvent()         // Retry item ready to process
        object ConfirmationReady : WorkEvent()  // Confirmation ready to relay
        object CleanupNeeded : WorkEvent()      // Periodic cleanup trigger
    }
    
    // Event channel for unified worker (replaces 4-5 polling loops!)
    private val workChannel = Channel<WorkEvent>(Channel.UNLIMITED)
    
    // Unified event-driven worker
    private var unifiedWorker: Job? = null
    
    // Battery metrics
    private var lastEventTime = System.currentTimeMillis()
    private var eventCount = 0
    private var wakeUpCount = 0
    
    // Network state monitoring
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    
    // Phase 5: Auto-save job for queue persistence
    private var autoSaveJob: Job? = null
    
    // Mesh watchdog: Ensures scanning/advertising stay active
    private var meshWatchdogJob: Job? = null
    
    // Alternating mesh mode for dancing mesh
    private var alternatingMeshJob: Job? = null
    private val ALTERNATING_INTERVAL_MS = 8_000L // 8 seconds per mode

    // Fix: Idle-disconnect window — once our outbound queue empties, keep the connection open
    // for this long so the remote peer has a chance to push data back to us.
    @Volatile private var lastInboundDataMs = 0L
    @Volatile private var queueEmptySinceMs = 0L   // timestamp when our queue first went empty
    private val IDLE_DISCONNECT_WINDOW_MS = 4_000L // 4 s of silence on both sides → disconnect

    // Fix: Peer cooldown — after disconnecting from a device, suppress reconnection for this
    // long so the alternating loop has a chance to find a different peer.
    private val recentlyConnectedPeers = LinkedHashMap<String, Long>() // address → disconnect timestamp
    private val PEER_COOLDOWN_MS = 45_000L // 45 seconds

    // TTL: track how many times THIS device has relayed each transaction so we can drop
    // transactions that have already been forwarded MAX_TX_RELAY_HOPS times.
    // Key = txId (hex string), Value = relay count.
    private val txRelayHops = HashMap<String, Int>()
    
    // Edge Case Fix #1: Bluetooth state tracking
    // Saves operation state when BT disabled, restores when BT re-enabled
    private var wasAdvertisingBeforeDisable = false
    private var wasScanningBeforeDisable = false
    
    // Permission monitoring
    private var permissionMonitoringJob: Job? = null
    private var lastKnownPermissionState = false
    
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
                
                this@BleService.appendLog("🔐 Bond state changed for ${device?.address}: ${bondState.toBondStateString()}")
                
                // If bonding completed, retry connection or descriptor write
                if (bondState == BluetoothDevice.BOND_BONDED && device != null) {
                    if (device == clientGatt?.device) {
                        this@BleService.appendLog("✅ Bonding completed, retrying connection...")
                        mainHandler.postDelayed({
                            clientGatt?.connect()
                        }, 500)
                    }
                    
                    // If we have a pending descriptor write, retry it
                    if (pendingDescriptorWrite != null && pendingGatt != null && device == pendingGatt?.device) {
                        this@BleService.appendLog("✅ Bonding completed, retrying descriptor write...")
                        mainHandler.postDelayed({
                            try {
                                pendingGatt?.let { gatt ->
                                    gatt.setCharacteristicNotification(remoteTxCharacteristic, true)
                                    val descriptor = pendingDescriptorWrite
                                    if (descriptor != null) {
                                        descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                                        gatt.writeDescriptor(descriptor)
                                        this@BleService.appendLog("🔄 Retrying descriptor write after bonding...")
                                    }
                                }
                            } catch (e: Exception) {
                                this@BleService.appendLog("❌ Failed to retry descriptor write after bonding: ${e.message}")
                            }
                        }, 500)
                    }
                }
            }
        }
    }
    
    // Edge Case Fix #1: Bluetooth state receiver
    // Monitors Bluetooth on/off state to prevent battery drain and manage operations
    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == BluetoothAdapter.ACTION_STATE_CHANGED) {
                val state = intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)
                
                when (state) {
                    BluetoothAdapter.STATE_OFF -> {
                        appendLog("📴 Bluetooth disabled - pausing all BLE operations")
                        appendLog("   This prevents battery drain from scanning/advertising on disabled BT")
                        
                        // Save current operation state before stopping
                        wasAdvertisingBeforeDisable = _isAdvertising.value
                        wasScanningBeforeDisable = _isScanning.value
                        
                        appendLog("   State saved: advertising=$wasAdvertisingBeforeDisable, scanning=$wasScanningBeforeDisable")
                        
                        // Stop all BLE operations immediately
                        stopScanning()
                        stopAdvertising()
                        closeGattConnection()
                        
                        // Update connection state
                        _connectionState.value = ConnectionState.DISCONNECTED
                        
                        appendLog("✅ All BLE operations stopped - safe for Bluetooth OFF state")
                    }
                    
                    BluetoothAdapter.STATE_ON -> {
                        appendLog("📶 Bluetooth enabled - recovering operations")
                        
                        // Check if we have permissions before attempting recovery
                        if (!hasRequiredPermissions()) {
                            appendLog("⚠️ Permissions not granted - cannot recover operations")
                            appendLog("   Please grant Bluetooth permissions in Settings")
                            return@onReceive
                        }
                        
                        // Re-initialize Bluetooth components after re-enable
                        appendLog("   Re-initializing Bluetooth components...")
                        serviceScope.launch {
                            try {
                                // Give BT stack a moment to fully initialize
                                delay(1500)
                                
                                // Re-initialize all Bluetooth components
                                initializeBluetooth()
                                appendLog("✅ Bluetooth components re-initialized successfully")
                                
                                // Resume alternating mesh mode after Bluetooth is re-enabled
                                appendLog("   Resuming alternating mesh mode...")
                                if (_connectionState.value == ConnectionState.DISCONNECTED && 
                                    connectedDevice == null && clientGatt == null) {
                                    startAlternatingMeshMode()
                                    appendLog("✅ Alternating mesh mode resumed after BT re-enable")
                                }
                                
                                // Reset saved state flags
                                wasAdvertisingBeforeDisable = false
                                wasScanningBeforeDisable = false
                            } catch (e: Exception) {
                                android.util.Log.e("BleService", "Failed to recover after BT re-enable", e)
                                appendLog("❌ Failed to recover operations: ${e.message}")
                                appendLog("   Will retry on next state change")
                                _connectionState.value = ConnectionState.ERROR
                            }
                        }
                    }
                    
                    BluetoothAdapter.STATE_TURNING_OFF -> {
                        appendLog("⚠️ Bluetooth turning off - preparing to stop operations")
                        // Preemptively save state before it fully turns off
                        wasAdvertisingBeforeDisable = _isAdvertising.value
                        wasScanningBeforeDisable = _isScanning.value
                        appendLog("   State pre-saved: advertising=$wasAdvertisingBeforeDisable, scanning=$wasScanningBeforeDisable")
                    }
                    
                    BluetoothAdapter.STATE_TURNING_ON -> {
                        appendLog("⚠️ Bluetooth turning on - preparing to resume operations")
                        appendLog("   BLE stack initializing... operations will resume when STATE_ON received")
                    }
                    
                    else -> {
                        appendLog("⚠️ Unknown Bluetooth state: $state")
                    }
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

    /**
     * All BLE peers discovered or connected during this session.
     * Key = MAC address. Updated on scan results and connection events.
     * Backed by the Rust health monitor — call [getHealthSnapshot] for full metrics.
     */
    private val _peers = MutableStateFlow<Map<String, DiscoveredPeer>>(emptyMap())
    val peers: StateFlow<Map<String, DiscoveredPeer>> = _peers

    /** Snapshot of a discovered/connected BLE peer. */
    data class DiscoveredPeer(
        val address: String,
        val rssi: Int,
        val discoveredAt: Long,
        val isConnected: Boolean,
        val lastSeenAt: Long = discoveredAt
    )

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
        
        // Edge Case Fix #1: Register Bluetooth state receiver
        // Monitors BT on/off to prevent battery drain and manage operations
        val btStateFilter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        registerReceiver(bluetoothStateReceiver, btStateFilter)
        appendLog("✅ Bluetooth state monitor registered - will handle BT on/off gracefully")
        
        // Start permission monitoring to detect when permissions are granted
        startPermissionMonitoring()
        
        // CRITICAL: Always call startForeground() when started via startForegroundService()
        // Android requires this within 5 seconds, even if we don't have permissions yet
        android.util.Log.d("BleService", "onCreate: Starting foreground service (required by Android)")
        startForeground()
        
        // Check permissions and initialize accordingly
        if (hasRequiredPermissions()) {
            android.util.Log.d("BleService", "onCreate: Permissions granted, initializing service")
            
            // Request battery optimization exemption for persistent operation
            requestBatteryOptimizationExemption()
            
            // Initialize Bluetooth asynchronously to avoid blocking onCreate
            serviceScope.launch {
                try {
                    android.util.Log.d("BleService", "onCreate: Initializing Bluetooth")
                    initializeBluetooth()
                    android.util.Log.d("BleService", "onCreate: Bluetooth initialized successfully")
                    
                    // Auto-start alternating mesh mode for automatic peer discovery
                    appendLog("🚀 Auto-starting alternating mesh mode")
                    startAlternatingMeshMode()
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
            
            // Note: Mesh watchdog disabled - alternating mode handles discovery automatically
            // startMeshWatchdog()
        } else {
            android.util.Log.w("BleService", "onCreate: Missing required permissions - service will wait for permissions")
            appendLog("⚠️ Permissions not granted - monitoring for permission grant")
            appendLog("   Service will automatically recover when permissions are granted")
            // Don't stop the service - keep it running so permission monitoring can recover
            // The service will automatically initialize when permissions are granted via handlePermissionGranted()
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
        
        // Edge Case Fix #5: Validate transaction size to prevent OOM and DOS attacks
        if (byteSize > MAX_TRANSACTION_SIZE) {
            appendLog("❌ Transaction too large: $byteSize bytes (max: $MAX_TRANSACTION_SIZE)")
            appendLog("   This prevents OutOfMemoryError and DOS attacks")
            return
        }
        
        if (byteSize <= 0) {
            appendLog("❌ Invalid transaction size: $byteSize bytes (must be > 0)")
            return
        }

        serviceScope.launch {
            appendLog("🧪 Queueing sample transaction (${byteSize} bytes)")
            val payload = ByteArray(byteSize) { Random.nextInt(0, 256).toByte() }
            val maxPayload = (currentMtu - 10).coerceAtLeast(20)
            appendLog("📏 Using MTU=$currentMtu, maxPayload=$maxPayload bytes per fragment")
            sdkInstance.fragment(payload, maxPayload).onSuccess { fragments ->
                val count = fragments.fragments.size
                val txId = fragments.fragments.firstOrNull()?.id ?: "unknown"
                val firstFragmentData = fragments.fragments.firstOrNull()?.data;
                val firstFragmentType = fragments.fragments.firstOrNull()?.fragmentType
                appendLog("📤 Queued $count fragments for tx ${txId}…")
                appendLog("   Fragment size calculation: ${byteSize} bytes ÷ $maxPayload = ~$count fragments")
                appendLog(" Fragment Data: ${firstFragmentData}…")
                appendLog(" Fragment Type: $firstFragmentType")

                // Store original bytes for potential re-fragmentation if MTU increases
                pendingTransactionBytes = payload
                fragmentsQueuedWithMtu = currentMtu
                
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
                
                // Edge Case Fix #5: Validate transaction size to prevent OOM and DOS attacks
                if (bytes.size > MAX_TRANSACTION_SIZE) {
                    appendLog("❌ Transaction too large: ${bytes.size} bytes (max: $MAX_TRANSACTION_SIZE)")
                    appendLog("   This prevents OutOfMemoryError and DOS attacks")
                    return@launch
                }
                
                appendLog("🧾 Queueing provided transaction (${bytes.size} bytes)")
                val maxPayload = (currentMtu - 10).coerceAtLeast(20)
                appendLog("📏 Using MTU=$currentMtu, maxPayload=$maxPayload bytes per fragment")
                sdkInstance.fragment(bytes, maxPayload).onSuccess { fragments ->
                    val count = fragments.fragments.size
                    val txId = fragments.fragments.firstOrNull()?.id ?: "unknown"
                    appendLog("📤 Queued $count fragments for tx ${txId}…")
                    appendLog("   Fragment size calculation: ${bytes.size} bytes ÷ $maxPayload = ~$count fragments")
                    
                    // Store original bytes for potential re-fragmentation if MTU increases
                    pendingTransactionBytes = bytes
                    fragmentsQueuedWithMtu = currentMtu
                    
                    // Start sending loop if not already running
                    ensureSendingLoopStarted()
                    
                    // If not connected, ensure scanning/advertising is active to establish connection
                    if (_connectionState.value != ConnectionState.CONNECTED) {
                        appendLog("⚠️ Not connected - ensuring BLE discovery is active...")
                        // Start scanning if not already scanning and not advertising
                        if (!_isScanning.value && !_isAdvertising.value) {
                            appendLog("   Starting scan to find peers...")
                            startScanning()
                        } else if (_isAdvertising.value) {
                            appendLog("   Already advertising - waiting for peer to connect...")
                        } else {
                            appendLog("   Already scanning - waiting to find peer...")
                        }
                    }
                }.onFailure {
                    appendLog("❌ Failed to queue provided transaction: ${it.message}")
                }
            } catch (e: IllegalArgumentException) {
                appendLog("❌ Invalid base64 input: ${e.message}")
            }
        }
    }

    /**
     * Queue signed transaction bytes for BLE transmission (for MWA integration)
     * Uses MTU-aware fragmentation and new priority-based outbound queue
     * 
     * @param txBytes Fully signed transaction bytes (from MWA)
     * @param priority Transaction priority (default: NORMAL)
     * @return Result with fragment count
     */
    suspend fun queueSignedTransaction(
        txBytes: ByteArray,
        priority: Priority = Priority.NORMAL
    ): Result<Int> = withContext(Dispatchers.Default) {
        val sdkInstance = sdk ?: run {
            appendLog("⚠️ SDK not initialized; cannot queue transaction")
            return@withContext Result.failure(Exception("SDK not initialized"))
        }
        
        // Edge Case Fix #5: Validate transaction size to prevent OOM and DOS attacks
        if (txBytes.size > MAX_TRANSACTION_SIZE) {
            appendLog("❌ Transaction too large: ${txBytes.size} bytes (max: $MAX_TRANSACTION_SIZE)")
            appendLog("   This prevents OutOfMemoryError and DOS attacks")
            return@withContext Result.failure(
                Exception("Transaction too large: ${txBytes.size} bytes (max: $MAX_TRANSACTION_SIZE)")
            )
        }

        try {
            appendLog("🧾 Queueing signed transaction (${txBytes.size} bytes, priority: $priority) [MWA]")
            val maxPayload = (currentMtu - 10).coerceAtLeast(20)
            appendLog("📏 Using MTU=$currentMtu, maxPayload=$maxPayload bytes per fragment")
            
            // Fragment transaction
            val fragmentResult = sdkInstance.fragment(txBytes, maxPayload)
            
            fragmentResult.fold(
                onSuccess = { fragmentList ->
                    val count = fragmentList.fragments.size
                    val firstFragment = fragmentList.fragments.firstOrNull()
                    
                    if (firstFragment == null) {
                        appendLog("❌ Fragment list is empty")
                        return@withContext Result.failure(Exception("Fragment list is empty"))
                    }
                    
                    // Use checksum as transaction ID (SHA-256 hash, hex-encoded)
                    // The checksum is base64-encoded in the Fragment, so decode and hex-encode it
                    appendLog("🔍 Decoding fragment checksum to get transaction ID...")
                    android.util.Log.d("PolliNet.BLE", "🔍 Decoding fragment checksum to get transaction ID...")
                    appendLog("   Fragment checksum (base64): ${firstFragment.checksum}...")
                    android.util.Log.d("PolliNet.BLE", "   Fragment checksum (base64): ${firstFragment.checksum}...")
                    appendLog("   Fragment ID (from Rust): ${firstFragment.id}")
                    android.util.Log.d("PolliNet.BLE", "   Fragment ID (from Rust): ${firstFragment.id}")
                    
                    val txId = try {
                        val checksumBytes = android.util.Base64.decode(firstFragment.checksum, android.util.Base64.DEFAULT)
                        appendLog("   ✅ Checksum decoded: ${checksumBytes.size} bytes")
                        android.util.Log.d("PolliNet.BLE", "   ✅ Checksum decoded: ${checksumBytes.size} bytes")
                        
                        if (checksumBytes.size != 32) {
                            val errorMsg = "❌ Invalid checksum size: ${checksumBytes.size} bytes (expected 32)"
                            appendLog(errorMsg)
                            android.util.Log.e("PolliNet.BLE", errorMsg)
                            return@withContext Result.failure(Exception("Invalid checksum size"))
                        }
                        
                        val hexTxId = checksumBytes.joinToString("") { "%02x".format(it) }
                        appendLog("   ✅ Transaction ID (hex): $hexTxId")
                        android.util.Log.d("PolliNet.BLE", "   ✅ Transaction ID (hex): $hexTxId")
                        appendLog("   ✅ Transaction ID length: ${hexTxId.length} characters (expected 64)")
                        android.util.Log.d("PolliNet.BLE", "   ✅ Transaction ID length: ${hexTxId.length} characters (expected 64)")
                        appendLog("   ✅ First 16 chars: ${hexTxId}...")
                        android.util.Log.d("PolliNet.BLE", "   ✅ First 16 chars: ${hexTxId}...")
                        hexTxId
                    } catch (e: Exception) {
                        val errorMsg = "❌ Failed to decode checksum: ${e.message}"
                        appendLog(errorMsg)
                        android.util.Log.e("PolliNet.BLE", errorMsg, e)
                        appendLog("   Error type: ${e.javaClass.simpleName}")
                        android.util.Log.e("PolliNet.BLE", "   Error type: ${e.javaClass.simpleName}", e)
                        return@withContext Result.failure(Exception("Failed to decode checksum: ${e.message}"))
                    }
                    
                    // Add fragment progress indicator (e.g., ~1/2, ~2/2)
                    val totalFragments = firstFragment.total
                    val fragmentProgress = "~$count/$totalFragments"
                    
                    appendLog("📤 Fragmenting into $count fragments for tx ${txId}… $fragmentProgress")
                    android.util.Log.d("PolliNet.BLE", "📤 Fragmenting into $count fragments for tx ${txId}… $fragmentProgress")
                    appendLog("   Using transaction ID: ${txId}... $fragmentProgress (full: $txId)")
                    android.util.Log.d("PolliNet.BLE", "   Using transaction ID: ${txId}... $fragmentProgress (full: $txId)")
                    
                    // Convert to FragmentFFI format - use the hex-encoded checksum as transaction ID
                    val fragmentsFFI = fragmentList.fragments.map { frag ->
                        FragmentFFI(
                            transactionId = txId, // Use the hex-encoded checksum for all fragments
                            fragmentIndex = frag.index,
                            totalFragments = frag.total,
                            dataBase64 = frag.data
                        )
                    }
                    
                    // Push to new outbound queue (Phase 2)
                    appendLog("📥 Pushing to outbound queue...")
                    val pushResult = sdkInstance.pushOutboundTransaction(
                        txBytes = txBytes,
                        txId = txId,
                        fragments = fragmentsFFI,
                        priority = priority
                    )
                    
                    pushResult.fold(
                        onSuccess = {
                            appendLog("✅ Added to outbound queue ($count fragments, priority: $priority)")
                            
                            // Phase 4: Trigger event for immediate processing (no polling delay!)
                            workChannel.trySend(WorkEvent.OutboundReady)
                            appendLog("📡 Event triggered - unified worker will process")
                            
                            // Store for potential MTU re-fragmentation
                            pendingTransactionBytes = txBytes
                            fragmentsQueuedWithMtu = currentMtu
                            
                            // Start sending loop if not already running
                            ensureSendingLoopStarted()
                            
                            // If not connected, ensure scanning/advertising is active to establish connection
                            if (_connectionState.value != ConnectionState.CONNECTED) {
                                appendLog("⚠️ Not connected - ensuring BLE discovery is active...")
                                // Start scanning if not already scanning and not advertising
                                if (!_isScanning.value && !_isAdvertising.value) {
                                    appendLog("   Starting scan to find peers...")
                                    startScanning()
                                } else if (_isAdvertising.value) {
                                    appendLog("   Already advertising - waiting for peer to connect...")
                                } else {
                                    appendLog("   Already scanning - waiting to find peer...")
                                }
                            }
                            
                            Result.success(count)
                        },
                        onFailure = { error ->
                            appendLog("❌ Failed to push to queue: ${error.message}")
                            Result.failure(error)
                        }
                    )
                },
                onFailure = { error ->
                    appendLog("❌ Failed to fragment transaction: ${error.message}")
                    Result.failure(error)
                }
            )
        } catch (e: Exception) {
            appendLog("❌ Failed to queue signed transaction: ${e.message}")
            Result.failure(e)
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
            appendLog("🔍 Operation in progress: ${operationInProgress.get()}")
            appendLog("🔍 Operation queue size: ${operationQueue.size}/$MAX_OPERATION_QUEUE_SIZE")
            if (operationQueue.size > MAX_OPERATION_QUEUE_SIZE * 0.8) {
                appendLog("   ⚠️ WARNING: Queue is ${(operationQueue.size.toFloat() / MAX_OPERATION_QUEUE_SIZE * 100).toInt()}% full!")
            }
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

    // =========================================================================
    // Phase 4: Unified Event-Driven Worker
    // =========================================================================
    
    /**
     * Start unified event-driven worker
     * Replaces multiple polling loops with single event-driven architecture
     * Battery savings: 85%+ (150 wake-ups/min → 5 wake-ups/min)
     */
    private fun startUnifiedEventWorker() {
        if (unifiedWorker?.isActive == true) {
            appendLog("🔄 Unified worker already running")
            return
        }
        
        appendLog("🚀 Starting unified event-driven worker (battery-optimized)")
        
        unifiedWorker = serviceScope.launch {
            var lastCleanup = System.currentTimeMillis()
            var lastReceivedCheck = System.currentTimeMillis()
            
            while (isActive) {
                try {
                    wakeUpCount++
                    
                    // Wait for ANY event OR 30-second timeout (fallback)
                    val event = withTimeoutOrNull(30_000) {
                        workChannel.receive()
                    }
                    
                    when (event) {
                        WorkEvent.OutboundReady -> {
                            appendLog("📤 Event: OutboundReady (sending loop handles transmission)")
                            eventCount++
                            lastEventTime = System.currentTimeMillis()
                            // DISABLED: processOutboundQueue() was redundant - sending loop handles transmission via nextOutbound()
                            // The sending loop (started by ensureSendingLoopStarted()) already reads fragments from the queue
                            // and sends them via sendNextOutbound() -> nextOutbound() -> sendToGatt()
                            // processOutboundQueue() was only logging "Would transmit" without actually sending
                            // processOutboundQueue()
                        }
                        WorkEvent.ReceivedReady -> {
                            appendLog("📥 Event: ReceivedReady")
                            eventCount++
                            lastEventTime = System.currentTimeMillis()
                            processReceivedQueue()
                        }
                        WorkEvent.RetryReady -> {
                            appendLog("🔄 Event: RetryReady")
                            eventCount++
                            lastEventTime = System.currentTimeMillis()
                            processRetryQueue()
                        }
                        WorkEvent.ConfirmationReady -> {
                            appendLog("✅ Event: ConfirmationReady")
                            eventCount++
                            lastEventTime = System.currentTimeMillis()
                            processConfirmationQueue()
                        }
                        WorkEvent.CleanupNeeded -> {
                            appendLog("🧹 Event: CleanupNeeded")
                            eventCount++
                            processCleanup()
                        }
                        null -> {
                            // Timeout - fallback check for received queue
                            val timeSinceLastCheck = System.currentTimeMillis() - lastReceivedCheck
                            val timeSinceLastCleanup = System.currentTimeMillis() - lastCleanup
                            
                            // Only log timeout if we're actually doing something
                            val shouldCheckQueue = timeSinceLastCheck > 10_000
                            val shouldCleanup = timeSinceLastCleanup > 300_000
                            
                            if (shouldCheckQueue || shouldCleanup) {
                                appendLog("⏰ Worker timeout (30s) - running fallback checks")
                                
                                // Check fragment reassembly progress using metrics
                                val metrics = sdk?.metrics()?.getOrNull()
                                if (metrics != null) {
                                    appendLog("📊 Fragment reassembly status:")
                                    appendLog("   Fragments buffered: ${metrics.fragmentsBuffered}")
                                    appendLog("   Transactions complete: ${metrics.transactionsComplete}")
                                    appendLog("   Reassembly failures: ${metrics.reassemblyFailures}")
                                    if (metrics.fragmentsBuffered > 0) {
                                        appendLog("   ⏳ ${metrics.fragmentsBuffered} fragments waiting for reassembly")
                                        appendLog("   💡 More fragments may be needed to complete transaction(s)")
                                    }
                                    if (metrics.lastError.isNotEmpty()) {
                                        appendLog("   ⚠️ Last error: ${metrics.lastError}")
                                    }
                                }
                                
                                // Get detailed fragment metadata for all incomplete transactions
                                val fragmentInfo = sdk?.getFragmentReassemblyInfo()?.getOrNull()
                                if (fragmentInfo != null && fragmentInfo.transactions.isNotEmpty()) {
                                    appendLog("📋 Fragment metadata for incomplete transactions:")
                                    fragmentInfo.transactions.forEachIndexed { idx, info ->
                                        val fragmentProgress = "~${info.receivedFragments}/${info.totalFragments}"
                                        val isComplete = info.receivedFragments == info.totalFragments
                                        val progressIndicator = if (isComplete) "✅ $fragmentProgress" else "⏳ $fragmentProgress"
                                        appendLog("   Transaction ${idx + 1}: ${info.transactionId}... $progressIndicator")
                                        android.util.Log.d("PolliNet.BLE", "   Transaction ${idx + 1}: ${info.transactionId}... $progressIndicator")
                                        appendLog("      Total fragments: ${info.totalFragments}")
                                        appendLog("      Received: ${info.receivedFragments}/${info.totalFragments}")
                                        appendLog("      Fragment indices received: ${info.receivedIndices.sorted().joinToString(", ")}")
                                        appendLog("      Fragment sizes: ${info.fragmentSizes.joinToString(", ")} bytes")
                                        appendLog("      Total bytes received: ${info.totalBytesReceived}")
                                        val missing = info.totalFragments - info.receivedFragments
                                        if (missing > 0) {
                                            appendLog("      ⏳ Waiting for $missing more fragments")
                                            val expectedIndices = (0 until info.totalFragments).toList()
                                            val missingIndices = expectedIndices.filter { it !in info.receivedIndices }
                                            appendLog("      Missing fragment indices: ${missingIndices.joinToString(", ")}")
                                        }
                                    }
                                } else if (fragmentInfo != null && fragmentInfo.transactions.isEmpty()) {
                                    appendLog("✅ No incomplete transactions - all fragments processed")
                                }
                            } else {
                                // Silent timeout - nothing to do, this is normal
                                // Only log every 5 minutes to reduce noise
                                if (timeSinceLastCheck > 300_000) {
                                    appendLog("⏰ Worker idle (no events) - this is normal when no work pending")
                                    lastReceivedCheck = System.currentTimeMillis() // Reset to avoid spam
                                }
                            }
                            
                            // Check received queue (only fallback needed)
                            if (shouldCheckQueue) {
                                appendLog("🔄 Fallback: Checking received queue...")
                                val queueSize = sdk?.getReceivedQueueSize()?.getOrNull() ?: 0
                                if (queueSize > 0) {
                                    appendLog("📦 Found $queueSize transactions in queue - processing...")
                                } else {
                                    appendLog("📭 Received queue is empty")
                                }
                                processReceivedQueue()
                                lastReceivedCheck = System.currentTimeMillis()
                            }
                            
                            // Periodic cleanup (every 5 minutes)
                            if (shouldCleanup) {
                                appendLog("🧹 Running periodic cleanup...")
                                processCleanup()
                                lastCleanup = System.currentTimeMillis()
                            }
                        }
                    }
                    
                } catch (e: Exception) {
                    appendLog("❌ Unified worker error: ${e.message}")
                    delay(5000) // Wait on error
                }
            }
        }
        
        appendLog("✅ Unified event-driven worker started")
        logBatteryMetrics()
    }
    
    /**
     * Process received transaction queue (event-driven)
     */
    private suspend fun processReceivedQueue() {
        val sdkInstance = sdk ?: run {
            appendLog("⚠️ processReceivedQueue: SDK not initialized, skipping")
            return
        }
        
        appendLog("🔄 processReceivedQueue: Starting queue check...")
        
        // Check internet connectivity
        if (!hasInternetConnection()) {
            appendLog("⚠️ processReceivedQueue: No internet connection, skipping submission")
            return
        }
        
        // Get fragment reassembly info to show progress indicators
        val fragmentInfo = sdkInstance.getFragmentReassemblyInfo().getOrNull()
        val fragmentInfoMap = fragmentInfo?.transactions?.associateBy { it.transactionId } ?: emptyMap()
        
        // Check queue size before processing
        val queueSizeBefore = sdkInstance.getReceivedQueueSize().getOrNull() ?: 0
        appendLog("📊 Received queue size: $queueSizeBefore transactions")
        
        if (queueSizeBefore == 0) {
            appendLog("📭 Queue is empty - no transactions to submit")
            // Show incomplete transactions if any
            if (fragmentInfoMap.isNotEmpty()) {
                appendLog("   (${fragmentInfoMap.size} incomplete transaction(s) still being reassembled)")
                fragmentInfoMap.values.forEach { info ->
                    val fragmentProgress = "~${info.receivedFragments}/${info.totalFragments}"
                    val progressIndicator = if (info.receivedFragments == info.totalFragments) "✅ $fragmentProgress" else "⏳ $fragmentProgress"
                    appendLog("      ${info.transactionId} $progressIndicator")
                    android.util.Log.d("PolliNet.BLE", "      ${info.transactionId} $progressIndicator")
                }
            }
            return
        }
        
        var processedCount = 0
        var successCount = 0
        var failureCount = 0
        val batchSize = 5 // Process up to 5 received transactions per wake-up
        
        appendLog("🚀 Processing up to $batchSize transactions from queue...")
        
        repeat(batchSize) {
            val receivedTx = sdkInstance.nextReceivedTransaction().getOrNull() ?: run {
                appendLog("📭 No more transactions in queue")
                return@repeat
            }
            
            // Check if this transaction is still in reassembly buffers (shouldn't happen, but check anyway)
            val txFragmentInfo = fragmentInfoMap[receivedTx.txId]
            val progressIndicator = if (txFragmentInfo != null) {
                val fragmentProgress = "~${txFragmentInfo.receivedFragments}/${txFragmentInfo.totalFragments}"
                if (txFragmentInfo.receivedFragments == txFragmentInfo.totalFragments) {
                    "✅ $fragmentProgress" // Complete
                } else {
                    "⏳ $fragmentProgress" // Still incomplete (unusual case)
                }
            } else {
                "✅ ~complete" // Already reassembled and in queue (complete)
            }
            
            appendLog("📥 Processing received tx: ${receivedTx.txId} $progressIndicator")
            android.util.Log.d("PolliNet.BLE", "📥 Processing received tx: ${receivedTx.txId} $progressIndicator")
            appendLog("   Transaction size: ${receivedTx.transactionBase64.length} base64 chars")
            
            try {
                appendLog("🌐 Submitting transaction to Solana RPC...")
                val submitResult = sdkInstance.submitOfflineTransaction(
                    transactionBase64 = receivedTx.transactionBase64,
                    verifyNonce = false
                )
                
                submitResult.onSuccess { signature ->
                    successCount++
                    val txProgress = txFragmentInfo?.let { "~${it.totalFragments}/${it.totalFragments}" } ?: "~complete"
                    appendLog("✅ ✅ ✅ Transaction submitted SUCCESSFULLY! ✅ ✅ ✅")
                    appendLog("   Transaction ID: ${receivedTx.txId} $txProgress")
                    android.util.Log.d("PolliNet.BLE", "   Transaction ID: ${receivedTx.txId} $txProgress")
                    appendLog("   Signature: $signature")
                    appendLog("   Transaction is now on-chain")
                    
                    // Mark as submitted for deduplication
                    val txBytes = android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP)
                    sdkInstance.markTransactionSubmitted(txBytes)
                    
                    // Calculate transaction hash (SHA-256) for confirmation
                    // The confirmation queue expects a hex-encoded 32-byte hash, not the UUID txId
                    val txHash = try {
                        val digest = java.security.MessageDigest.getInstance("SHA-256")
                        digest.update(txBytes)
                        digest.digest().joinToString("") { "%02x".format(it) }
                    } catch (e: Exception) {
                        appendLog("❌ Failed to calculate transaction hash: ${e.message}")
                        // Fallback: use first 64 chars of base64 as identifier (not ideal but better than UUID)
                        receivedTx.transactionBase64.take(64)
                    }
                    
                    // Queue confirmation for relay (Phase 2)
                    sdkInstance.queueConfirmation(txHash, signature)
                        .onSuccess {
                            appendLog("📤 Queued confirmation for relay")
                            workChannel.trySend(WorkEvent.ConfirmationReady)
                        }
                        .onFailure { e ->
                            appendLog("⚠️ Failed to queue confirmation: ${e.message}")
                        }
                    
                    processedCount++
                }.onFailure { error ->
                    failureCount++
                    val txProgress = txFragmentInfo?.let { "~${it.totalFragments}/${it.totalFragments}" } ?: "~complete"
                    val errorMsg = error.message ?: "Unknown error"
                    
                    appendLog("❌ ❌ ❌ Transaction submission FAILED ❌ ❌ ❌")
                    appendLog("   Transaction ID: ${receivedTx.txId} $txProgress")
                    android.util.Log.e("PolliNet.BLE", "   Transaction ID: ${receivedTx.txId} $txProgress")
                    appendLog("   Error: $errorMsg")
                    
                    // Check if this is a stale or permanently invalid transaction error
                    if (isStaleTransactionError(errorMsg)) {
                        appendLog("   🗑️ Invalid transaction detected - dropping (won't retry)")
                        android.util.Log.w("PolliNet.BLE", "Dropping invalid transaction ${receivedTx.txId.take(8)}... due to: $errorMsg")
                        // Don't add to retry queue - transaction is permanently invalid
                    } else {
                    appendLog("   Adding to retry queue for later...")
                    
                    // Add to retry queue (Phase 2)
                    sdkInstance.addToRetryQueue(
                        txBytes = android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP),
                        txId = receivedTx.txId,
                            error = errorMsg
                    ).onSuccess {
                        appendLog("   ✅ Added to retry queue")
                    }.onFailure { e ->
                        appendLog("   ❌ Failed to add to retry queue: ${e.message}")
                        }
                    }
                }
            } catch (e: Exception) {
                failureCount++
                val txProgress = txFragmentInfo?.let { "~${it.totalFragments}/${it.totalFragments}" } ?: "~complete"
                appendLog("❌ ❌ ❌ Exception processing received tx ❌ ❌ ❌")
                appendLog("   Transaction ID: ${receivedTx.txId} $txProgress")
                android.util.Log.e("PolliNet.BLE", "   Transaction ID: ${receivedTx.txId} $txProgress", e)
                appendLog("   Exception: ${e.message}")
                appendLog("   Stack trace: ${e.stackTraceToString()}")
        }
        }
        
        // Summary log
        if (processedCount > 0) {
            appendLog("📊 Queue processing complete:")
            appendLog("   ✅ Successful: $successCount")
            appendLog("   ❌ Failed: $failureCount")
            appendLog("   📦 Total processed: $processedCount")
            
            // Check remaining queue size
            val queueSizeAfter = sdkInstance.getReceivedQueueSize().getOrNull() ?: 0
            appendLog("   📊 Remaining in queue: $queueSizeAfter")
            
            if (queueSizeAfter > 0) {
                appendLog("   🔄 More transactions pending - will process in next cycle")
            }
        } else {
            appendLog("ℹ️ No transactions were processed")
        }
    }
    
    /**
     * Process retry queue (event-driven)
     * Note: WorkManager is preferred for retries, this is fallback
     */
    private suspend fun processRetryQueue() {
        val sdkInstance = sdk ?: return
        
        if (!hasInternetConnection()) {
            return
        }
        
        var processedCount = 0
        var skippedCount = 0
        
        repeat(5) { // Process up to 5 retries per wake-up
            val retryItem = sdkInstance.popReadyRetry().getOrNull() ?: return@repeat
            
            // CRITICAL: Check if retry is actually ready (respect backoff)
            // Rust's pop_ready() should only return items where nextRetryInSecs <= 0,
            // but we check defensively to ensure we respect backoff timing
            if (retryItem.nextRetryInSecs > 0) {
                // Item not ready yet - this indicates a potential bug in Rust's pop_ready() filtering
                // Skip it to respect backoff and prevent infinite retry loops
                appendLog("⏳ Retry not ready yet for tx: ${retryItem.txId.take(8)}... (next retry in ${retryItem.nextRetryInSecs}s, attempt ${retryItem.attemptCount})")
                android.util.Log.w("PolliNet.BLE", "⚠️ Skipping retry - not ready yet (nextRetryInSecs=${retryItem.nextRetryInSecs}). This shouldn't happen if Rust pop_ready() is working correctly.")
                skippedCount++
                
                // Re-add to queue since we popped it but it's not ready
                // Only re-add if we haven't exceeded max attempts to avoid infinite loops
                if (retryItem.attemptCount < 5) {
                    try {
                        val txBytes = android.util.Base64.decode(retryItem.txBytes, android.util.Base64.NO_WRAP)
                        sdkInstance.addToRetryQueue(
                            txBytes = txBytes,
                            txId = retryItem.txId,
                            error = retryItem.lastError
                        ).onFailure { e ->
                            appendLog("❌ Failed to re-add skipped retry: ${e.message}")
                            android.util.Log.e("PolliNet.BLE", "Failed to re-add skipped retry", e)
                        }
                    } catch (e: Exception) {
                        appendLog("❌ Exception re-adding skipped retry: ${e.message}")
                        android.util.Log.e("PolliNet.BLE", "Exception re-adding skipped retry", e)
                    }
                } else {
                    appendLog("❌ Not re-adding skipped retry - max attempts (${retryItem.attemptCount}) exceeded")
                }
                return@repeat
            }
            
            appendLog("🔄 Retrying tx: ${retryItem.txId.take(8)}... (attempt ${retryItem.attemptCount}, nextRetryInSecs=${retryItem.nextRetryInSecs})")
            
            try {
                val txBytes = android.util.Base64.decode(retryItem.txBytes, android.util.Base64.NO_WRAP)
                val submitResult = sdkInstance.submitOfflineTransaction(
                    transactionBase64 = retryItem.txBytes,
                    verifyNonce = false
                )
                
                submitResult.onSuccess { signature ->
                    appendLog("✅ Retry successful: $signature")
                    sdkInstance.markTransactionSubmitted(txBytes)
                    
                    // Calculate transaction hash (SHA-256) for confirmation
                    // The confirmation queue expects a hex-encoded 32-byte hash, not the UUID txId
                    val txHash = try {
                        val digest = java.security.MessageDigest.getInstance("SHA-256")
                        digest.update(txBytes)
                        digest.digest().joinToString("") { "%02x".format(it) }
                    } catch (e: Exception) {
                        appendLog("❌ Failed to calculate transaction hash: ${e.message}")
                        // Fallback: use first 64 chars of base64 as identifier (not ideal but better than UUID)
                        retryItem.txBytes.take(64)
                    }
                    
                    // Queue confirmation
                    sdkInstance.queueConfirmation(txHash, signature)
                        .onSuccess {
                            workChannel.trySend(WorkEvent.ConfirmationReady)
                        }
                    
                    processedCount++
                }.onFailure { error ->
                    val errorMsg = error.message ?: "Unknown error"
                    appendLog("⚠️ Retry failed (attempt ${retryItem.attemptCount}): $errorMsg")
                    
                    // Check if this is a stale or permanently invalid transaction error
                    if (isStaleTransactionError(errorMsg)) {
                        appendLog("   🗑️ Invalid transaction detected - dropping (won't retry)")
                        android.util.Log.w("PolliNet.BLE", "Dropping invalid transaction ${retryItem.txId.take(8)}... after ${retryItem.attemptCount} attempts due to: $errorMsg")
                        // Don't re-add to retry queue - transaction is permanently invalid
                    } else {
                    // Re-add to retry queue with incremented count (if not max)
                    if (retryItem.attemptCount < 5) {
                        sdkInstance.addToRetryQueue(
                            txBytes = txBytes,
                            txId = retryItem.txId,
                                error = errorMsg
                        )
                    } else {
                        appendLog("❌ Giving up on tx ${retryItem.txId.take(8)}... after ${retryItem.attemptCount} attempts")
                        }
                    }
                }
            } catch (e: Exception) {
                appendLog("❌ Exception processing retry: ${e.message}")
            }
        }
        
        if (processedCount > 0) {
            appendLog("✅ Processed $processedCount retry items")
        }
        if (skippedCount > 0) {
            appendLog("⏳ Skipped $skippedCount retry items (not ready yet - respecting backoff)")
        }
    }
    
    /**
     * Process confirmation queue (event-driven)
     */
    private suspend fun processConfirmationQueue() {
        val sdkInstance = sdk ?: return
        
        // Check connection state
        if (_connectionState.value != ConnectionState.CONNECTED) {
            appendLog("⚠️ Not connected - confirmation relay skipped")
            return
        }
        
        var processedCount = 0
        
        repeat(10) { // Process up to 10 confirmations per wake-up
            val confirmation = sdkInstance.popConfirmation().getOrNull() ?: return@repeat
            
            appendLog("✅ Relaying confirmation for tx: ${confirmation.txId.take(8)}...")
            
            // Serialize confirmation to JSON bytes for BLE transmission
            try {
                val jsonBytes = json.encodeToString(Confirmation.serializer(), confirmation).toByteArray(Charsets.UTF_8)
                
                appendLog("   📤 Serialized confirmation: ${jsonBytes.size} bytes")
            when (confirmation.status) {
                is ConfirmationStatus.Success -> {
                    val sig = (confirmation.status as ConfirmationStatus.Success).signature
                        appendLog("   SUCCESS: ${sig.take(16)}... (relay count: ${confirmation.relayCount})")
                }
                is ConfirmationStatus.Failed -> {
                    val err = (confirmation.status as ConfirmationStatus.Failed).error
                        appendLog("   FAILED: $err (relay count: ${confirmation.relayCount})")
                }
            }
            
                // Send confirmation over BLE using the same mechanism as fragments
                // Since confirmations are small, we can send them as a single packet
                if (jsonBytes.size <= currentMtu - 10) {
                    // Send directly if it fits in one packet
                    sendConfirmationToGatt(jsonBytes)
            processedCount++
                } else {
                    // If confirmation is too large (unlikely), log error
                    appendLog("❌ Confirmation too large (${jsonBytes.size} bytes) for MTU ($currentMtu)")
                }
            } catch (e: Exception) {
                appendLog("❌ Failed to serialize/send confirmation: ${e.message}")
            }
        }
        
        if (processedCount > 0) {
            appendLog("✅ Relayed $processedCount confirmations")
        }
    }
    
    /**
     * Handle received confirmation from BLE
     */
    private suspend fun handleReceivedConfirmation(data: ByteArray) {
        try {
            appendLog("📨 ===== PROCESSING RECEIVED CONFIRMATION =====")
            appendLog("📨 Confirmation size: ${data.size} bytes")
            
            // Deserialize confirmation from JSON
            val confirmationStr = String(data, Charsets.UTF_8)
            val confirmation = json.decodeFromString<Confirmation>(confirmationStr)
            
            appendLog("✅ Confirmation deserialized for tx: ${confirmation.txId.take(8)}...")
            appendLog("   Relay count: ${confirmation.relayCount}")
            
            when (confirmation.status) {
                is ConfirmationStatus.Success -> {
                    val sig = (confirmation.status as ConfirmationStatus.Success).signature
                    appendLog("   ✅ SUCCESS: ${sig.take(16)}...")
                    appendLog("   📝 Transaction ${confirmation.txId.take(8)}... was successfully submitted!")
                }
                is ConfirmationStatus.Failed -> {
                    val err = (confirmation.status as ConfirmationStatus.Failed).error
                    appendLog("   ❌ FAILED: $err")
                    appendLog("   📝 Transaction ${confirmation.txId.take(8)}... submission failed")
                }
            }
            
            // Relay confirmation back through the mesh if hop count hasn't exceeded max
            // In a full mesh, we'd check if this confirmation is for us (we're the origin)
            // For now, we always relay if hop count allows (mesh will eventually reach origin)
            if (confirmation.relayCount < MAX_TX_RELAY_HOPS) {
                appendLog("🔄 Relaying confirmation (hops: ${confirmation.relayCount}/$MAX_TX_RELAY_HOPS)")
                sdk?.relayConfirmation(confirmation)?.onSuccess {
                    appendLog("✅ Confirmation re-queued for relay")
                    workChannel.trySend(WorkEvent.ConfirmationReady)
                }?.onFailure { e ->
                    appendLog("⚠️ Failed to relay confirmation: ${e.message}")
                }
            } else {
                appendLog("⚠️ Confirmation reached relay TTL (${confirmation.relayCount}/$MAX_TX_RELAY_HOPS) — dropping")
            }
            
            appendLog("✅ Confirmation processed")
            
        } catch (e: Exception) {
            appendLog("❌ Failed to process confirmation: ${e.message}")
            android.util.Log.e("PolliNet.BLE", "Failed to process confirmation", e)
        }
    }
    
    /**
     * Send confirmation over BLE GATT
     */
    @SuppressLint("MissingPermission")
    private fun sendConfirmationToGatt(data: ByteArray) {
        appendLog("📤 sendConfirmationToGatt: Sending ${data.size} bytes")
        
        // Use the same GATT transmission path as fragments
        val gatt = clientGatt
        val remoteRx = remoteRxCharacteristic
        
        // If we have a client connection, use client path (write to remote RX)
        if (gatt != null && remoteRx != null) {
            appendLog("   → Using CLIENT path (write confirmation to remote RX)")
            
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                val result = gatt.writeCharacteristic(
                    remoteRx,
                    data,
                    BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                )
                appendLog(if (result == BluetoothGatt.GATT_SUCCESS) {
                    "✅ Confirmation sent successfully (${data.size}B)"
                } else {
                    "❌ Failed to send confirmation (result: $result)"
                })
            } else {
                remoteRx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                @Suppress("DEPRECATION")
                remoteRx.value = data
                @Suppress("DEPRECATION")
                val success = gatt.writeCharacteristic(remoteRx)
                appendLog(if (success) {
                    "✅ Confirmation sent successfully (${data.size}B)"
                } else {
                    "❌ Failed to send confirmation"
                })
            }
            return
        }
        
        // Fallback: Use server path if no client connection
        val server = gattServer
        val txChar = gattCharacteristicTx
        val device = connectedDevice
        
        if (server != null && txChar != null && device != null) {
            appendLog("   → Using SERVER path (notify confirmation)")
            @Suppress("DEPRECATION")
            txChar.value = data
            val success = server.notifyCharacteristicChanged(device, txChar, false)
            appendLog(if (success) {
                "✅ Confirmation notification sent (${data.size}B)"
            } else {
                "❌ Failed to notify confirmation"
            })
        } else {
            appendLog("❌ No active connection for sending confirmation")
        }
    }
    
    /**
     * Process cleanup (remove stale data)
     */
    private suspend fun processCleanup() {
        val sdkInstance = sdk ?: return
        
        appendLog("🧹 Running cleanup...")
        
        // Cleanup stale fragments
        val fragmentsCleaned = sdkInstance.cleanupStaleFragments().getOrNull() ?: 0
        
        // Cleanup expired confirmations and retries
        val (confirmationsCleaned, retriesCleaned) = sdkInstance.cleanupExpired().getOrNull() ?: Pair(0, 0)
        
        if (fragmentsCleaned > 0 || confirmationsCleaned > 0 || retriesCleaned > 0) {
            appendLog("✅ Cleanup complete:")
            appendLog("   Fragments: $fragmentsCleaned")
            appendLog("   Confirmations: $confirmationsCleaned")
            appendLog("   Retries: $retriesCleaned")
        } else {
            appendLog("✅ Cleanup complete (nothing to clean)")
        }
    }
    
    /**
     * Log battery metrics
     */
    private fun logBatteryMetrics() {
        try {
            val batteryManager = getSystemService(Context.BATTERY_SERVICE) as? BatteryManager
            val batteryPct = batteryManager?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY) ?: -1
            val currentNow = batteryManager?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW) ?: 0
            
            val wakeUpsPerMin = if (System.currentTimeMillis() - lastEventTime > 60_000) {
                0
            } else {
                wakeUpCount
            }
            
            appendLog("🔋 Battery: $batteryPct%, Current: ${currentNow/1000}mA, Wake-ups/min: $wakeUpsPerMin")
            
            // Reset counter every minute
            if (System.currentTimeMillis() - lastEventTime > 60_000) {
                wakeUpCount = 0
            }
        } catch (e: Exception) {
            // Ignore battery metrics errors
        }
    }
    
    /**
     * Register network state callback for immediate response to connectivity changes
     */
    private fun registerNetworkCallback() {
        try {
            val connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
            
            networkCallback = object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    appendLog("📡 Network available - triggering pending work")
                    // Internet restored - process pending transactions and retries
                    workChannel.trySend(WorkEvent.ReceivedReady)
                    workChannel.trySend(WorkEvent.RetryReady)

                    // Quietly refresh offline nonce bundle in the background
                    serviceScope.launch {
                        try {
                            val refreshed = sdk?.refreshOfflineBundle()?.getOrNull() ?: 0
                            if (refreshed > 0) {
                                appendLog("♻️ Refreshed $refreshed cached nonces after network recovery")
                            }
                        } catch (_: Exception) {
                            // Best-effort, ignore failures
                        }
                    }
                }
                
                override fun onLost(network: Network) {
                    appendLog("📡 Network lost - queuing mode activated")
                }
                
                override fun onCapabilitiesChanged(
                    network: Network,
                    networkCapabilities: NetworkCapabilities
                ) {
                    val hasInternet = networkCapabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                    val validated = networkCapabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
                    
                    if (hasInternet && validated) {
                        appendLog("📡 Internet validated - triggering work")
                        workChannel.trySend(WorkEvent.ReceivedReady)
                        
                        // Also attempt a quiet nonce bundle refresh here (in case onAvailable was missed)
                        serviceScope.launch {
                            try {
                                val refreshed = sdk?.refreshOfflineBundle()?.getOrNull() ?: 0
                                if (refreshed > 0) {
                                    appendLog("♻️ Refreshed $refreshed cached nonces after validation")
                                }
                            } catch (_: Exception) {
                                // Ignore, best-effort
                            }
                        }
                    }
                }
            }
            
            val networkRequest = NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .build()
            
            connectivityManager.registerNetworkCallback(networkRequest, networkCallback!!)
            appendLog("✅ Network callback registered")
            
        } catch (e: Exception) {
            appendLog("⚠️ Failed to register network callback: ${e.message}")
        }
    }
    
    /**
     * Unregister network callback
     */
    private fun unregisterNetworkCallback() {
        try {
            networkCallback?.let {
                val connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
                connectivityManager.unregisterNetworkCallback(it)
                appendLog("✅ Network callback unregistered")
            }
        } catch (e: Exception) {
            appendLog("⚠️ Failed to unregister network callback: ${e.message}")
        }
    }
    
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

                                    // Clean up relay-hop tracking — no longer needed
                                    txRelayHops.remove(receivedTx.txId)

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
                            // No internet — relay to next BLE peer, subject to hop limit.
                            val hops = txRelayHops.getOrDefault(receivedTx.txId, 0)
                            if (hops >= MAX_TX_RELAY_HOPS) {
                                appendLog("⚠️ TX ${receivedTx.txId.take(8)} hit relay TTL ($hops/$MAX_TX_RELAY_HOPS) — dropping to prevent infinite circulation")
                                txRelayHops.remove(receivedTx.txId) // free memory
                            } else {
                                txRelayHops[receivedTx.txId] = hops + 1
                                appendLog("📡 No internet, relaying transaction ${receivedTx.txId.take(8)} (hop ${hops + 1}/$MAX_TX_RELAY_HOPS)")

                                // Evict oldest entry if map grows too large (safety valve)
                                if (txRelayHops.size > 200) {
                                    txRelayHops.remove(txRelayHops.keys.first())
                                }

                                // Queue for BLE transmission to other peers (re-fragment)
                                try {
                                    val fragmentResult = sdkInstance.fragmentTransaction(txBytes)
                                    fragmentResult.onSuccess { fragmentDataList ->
                                        appendLog("📤 Queued ${fragmentDataList.size} fragments for mesh relay")
                                        ensureSendingLoopStarted()
                                    }.onFailure { e ->
                                        appendLog("⚠️ Failed to queue for relay: ${e.message}")
                                        sdkInstance.pushReceivedTransaction(txBytes)
                                    }
                                } catch (e: Exception) {
                                    appendLog("⚠️ Exception while queueing relay: ${e.message}")
                                    sdkInstance.pushReceivedTransaction(txBytes)
                                }
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
     * Check if error indicates a stale transaction that should not be retried
     * Detects nonce errors and other permanent failures
     */
    private fun isStaleTransactionError(errorMessage: String?): Boolean {
        if (errorMessage == null) return false
        
        val errorLower = errorMessage.lowercase()
        
        // Common Solana nonce error patterns (stale/expired transactions)
        val staleErrorPatterns = listOf(
            "nonce has been advanced",
            "nonce account",
            "blockhash not found",
            "this transaction has already been processed",
            "transaction has already been processed",
            "duplicate signature",
            "signature verification failed", // Can indicate stale transaction
            "invalid blockhash",
            "blockhash expired"
        )
        
        // Permanently invalid transaction errors (won't succeed on retry)
        val permanentlyInvalidPatterns = listOf(
            "invalid account data for instruction",
            "invalidaccountdata",
            "invalid account data",
            "account not found",
            "insufficient funds",
            "program account not found",
            "invalid program id",
            "wrong number of accounts",
            "invalid instruction data"
        )
        
        return staleErrorPatterns.any { pattern ->
            errorLower.contains(pattern, ignoreCase = true)
        } || permanentlyInvalidPatterns.any { pattern ->
            errorLower.contains(pattern, ignoreCase = true)
        }
    }

    /**
     * Handle incoming packet - reassemble and queue for auto-submission
     * Phase 4: Triggers event when transaction complete (no polling!)
     */
    private suspend fun handleReceivedData(data: ByteArray) {
        lastInboundDataMs = System.currentTimeMillis()
        try {
            appendLog("📥 ===== PROCESSING RECEIVED DATA =====")
            appendLog("📥 Data size: ${data.size} bytes")
            appendLog("📥 Data preview: ${data.take(32).joinToString(" ") { "%02X".format(it) }}...")
            
            // Check if this is a confirmation (JSON) or a fragment (binary)
            // Confirmations start with '{' (JSON), fragments are binary (bincode)
            val isConfirmation = data.isNotEmpty() && data[0] == '{'.code.toByte()
            
            if (isConfirmation) {
                // Handle confirmation
                handleReceivedConfirmation(data)
                return
            }
            
            // Push to SDK for reassembly (fragment)
            val result = sdk?.pushInbound(data)
            result?.onSuccess {
                appendLog("✅ ✅ ✅ Fragment processed successfully ✅ ✅ ✅")
                android.util.Log.d("PolliNet.BLE", "✅ ✅ ✅ Fragment processed successfully ✅ ✅ ✅")
                appendLog("✅ Fragment added to reassembly buffer")
                
                // Check metrics to see if transaction was completed
                val metrics = sdk?.metrics()?.getOrNull()
                metrics?.let {
                    appendLog("📊 Metrics after fragment processing:")
                    android.util.Log.d("PolliNet.BLE", "📊 Metrics: fragmentsBuffered=${it.fragmentsBuffered}, transactionsComplete=${it.transactionsComplete}, reassemblyFailures=${it.reassemblyFailures}")
                    appendLog("   Fragments buffered: ${it.fragmentsBuffered}")
                    appendLog("   Transactions complete: ${it.transactionsComplete}")
                    appendLog("   Reassembly failures: ${it.reassemblyFailures}")
                    
                    if (it.transactionsComplete > 0) {
                        appendLog("   ⚠️ Transaction was marked complete but checking queue...")
                        android.util.Log.w("PolliNet.BLE", "   ⚠️ Transaction was marked complete (${it.transactionsComplete}) but checking queue...")
                    }
                }
                
                // Check if we have completed transactions in queue
                val queueSize = sdk?.getReceivedQueueSize()?.getOrNull() ?: 0
                appendLog("📊 Received queue size: $queueSize")
                android.util.Log.d("PolliNet.BLE", "📊 Received queue size: $queueSize")
                
                if (queueSize > 0) {
                    appendLog("🎉 🎉 🎉 Transaction reassembly complete! Queue size: $queueSize")
                    android.util.Log.i("PolliNet.BLE", "🎉 🎉 🎉 Transaction reassembly complete! Queue size: $queueSize")
                    
                    // Phase 4: Trigger event for immediate processing (no polling delay!)
                    workChannel.trySend(WorkEvent.ReceivedReady)
                    appendLog("📡 Event triggered - unified worker will submit transaction")
                } else {
                    // Queue is empty - check fragment reassembly info
                    val fragmentInfo = sdk?.getFragmentReassemblyInfo()?.getOrNull()
                    if (fragmentInfo != null && fragmentInfo.transactions.isEmpty()) {
                        // No incomplete transactions - transaction was completed but not queued
                        if (metrics?.transactionsComplete ?: 0 > 0) {
                            appendLog("⚠️ ⚠️ ⚠️ WARNING: Transaction was completed but NOT in received queue! ⚠️ ⚠️ ⚠️")
                            android.util.Log.w("PolliNet.BLE", "⚠️ ⚠️ ⚠️ WARNING: Transaction was completed (metrics: ${metrics?.transactionsComplete}) but NOT in received queue!")
                            appendLog("   ✅ Fragments were successfully reassembled")
                            appendLog("   ❌ BUT transaction was rejected as a DUPLICATE")
                            appendLog("   💡 This means the transaction hash already exists in submitted_tx_hashes")
                            appendLog("   💡 Possible reasons:")
                            appendLog("      • Transaction was already received/submitted before")
                            appendLog("      • You're testing with the same transaction multiple times")
                            appendLog("      • Same device is both sender and receiver (loopback)")
                            appendLog("   🔧 To reset: Clear app data or reinstall the app")
                            android.util.Log.w("PolliNet.BLE", "   💡 Transaction was rejected as duplicate - clear app data to reset")
                        } else {
                            appendLog("⏳ Waiting for more fragments... (received queue is empty)")
                            appendLog("   This is normal if not all fragments have been received yet")
                        }
                    } else {
                        appendLog("⏳ Waiting for more fragments... (received queue is empty)")
                        appendLog("   This is normal if not all fragments have been received yet")
                        fragmentInfo?.transactions?.forEach { info ->
                            val progress = "~${info.receivedFragments}/${info.totalFragments}"
                            appendLog("   Fragment progress: ${info.transactionId.take(16)}... $progress")
                        }
                    }
                }
            }?.onFailure { e ->
                appendLog("❌ ❌ ❌ Error processing fragment ❌ ❌ ❌")
                appendLog("❌ Error: ${e.message}")
                appendLog("❌ Fragment size: ${data.size} bytes")
                appendLog("❌ Data preview: ${data.take(32).joinToString(" ") { "%02X".format(it) }}...")
                if (e is PolliNetException) {
                    appendLog("❌ Code: ${e.code}")
                }
                appendLog("⚠️ This might indicate a fragment format mismatch or deserialization error")
            }
            appendLog("📥 ===== END PROCESSING =====\n")
        } catch (e: Exception) {
            appendLog("❌ ❌ ❌ Exception in handleReceivedData ❌ ❌ ❌")
            appendLog("❌ Error: ${e.message}")
            appendLog("❌ Stack trace: ${e.stackTraceToString()}")
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
        // Edge Case Fix #21: Cancel all pending handler callbacks FIRST
        // Prevents memory leaks from pending postDelayed callbacks
        // This must be done before any other cleanup to prevent callbacks
        // from executing after service is partially destroyed
        mainHandler.removeCallbacksAndMessages(null)
        appendLog("🧹 Cancelled all pending handler callbacks")
        
        // Stop permission monitoring
        permissionMonitoringJob?.cancel()
        permissionMonitoringJob = null
        
        // Phase 5: Force save queues before shutdown
        serviceScope.launch {
            try {
                sdk?.saveQueues()
                appendLog("💾 Queues saved before shutdown")
            } catch (e: Exception) {
                appendLog("⚠️ Failed to save queues on shutdown: ${e.message}")
            }
        }
        
        // Cancel all coroutine jobs
        autoSubmitJob?.cancel()
        cleanupJob?.cancel()
        sendingJob?.cancel()
        unifiedWorker?.cancel() // Phase 4: Cancel unified worker
        autoSaveJob?.cancel() // Phase 5: Cancel auto-save job
        alternatingMeshJob?.cancel() // Cancel alternating mesh mode
        serviceScope.cancel()
        
        // Phase 4: Cancel WorkManager background tasks
        cancelBackgroundTasks()
        
        // Unregister network callback (Phase 4)
        unregisterNetworkCallback()
        
        // Unregister bond state receiver
        try {
            unregisterReceiver(bondStateReceiver)
        } catch (e: IllegalArgumentException) {
            // Receiver was not registered
        }
        
        // Edge Case Fix #1: Unregister Bluetooth state receiver
        try {
            unregisterReceiver(bluetoothStateReceiver)
            appendLog("✅ Bluetooth state monitor unregistered")
        } catch (e: IllegalArgumentException) {
            // Receiver was not registered
        }
        
        // Note: Mesh watchdog not used with alternating mode
        meshWatchdogJob?.cancel() // Keep for compatibility if needed
        stopAlternatingMeshMode() // Stop alternating mesh mode
        
        // Stop BLE operations
        stopScanning()
        stopAdvertising()
        closeGattConnection()
        gattServer?.close()
        sdk?.shutdown()
        SdkHolder.clear() // Release WeakReference so workers see null and skip gracefully
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
     * 
     * @param config SDK configuration (RPC URL, storage directory, etc.)
     * @return Result<Unit> - Success if initialized, Failure if error occurred
     * 
     * Note: If SDK is already initialized, this method will return success without re-initializing
     * to prevent resource leaks and handle duplication.
     */
    suspend fun initializeSdk(config: SdkConfig): Result<Unit> {
        // Prevent double initialization - if SDK is already initialized, return success
        if (sdk != null) {
            appendLog("⚠️ SDK already initialized - skipping re-initialization")
            appendLog("   This prevents resource leaks from duplicate initialization")
            return Result.success(Unit)
        }
        
        return PolliNetSDK.initialize(config).map {
            sdk = it
            SdkHolder.set(it)
            appendLog("✅ SDK initialized successfully")
            
            // Phase 4: Start unified event-driven worker (replaces multiple polling loops)
            startUnifiedEventWorker()
            appendLog("🚀 Event-driven worker started (battery-optimized)")
            
            // Phase 4: Start network state listener
            registerNetworkCallback()
            appendLog("📡 Network state listener registered")
            
            // Phase 4: Schedule WorkManager tasks for battery-efficient background work
            scheduleBackgroundTasks()
            appendLog("⏰ WorkManager tasks scheduled (retry: 15min, cleanup: 30min)")
            
            // Phase 5: Start auto-save job for queue persistence
            startAutoSaveJob()
            appendLog("💾 Auto-save job started (debounced: 5s)")
            
            // If already connected, start the sending loop now that SDK is ready
            if (_connectionState.value == ConnectionState.CONNECTED) {
                appendLog("🔄 Connection already established - starting sending loop now that SDK is ready")
                ensureSendingLoopStarted()
            }
        }.onFailure { error ->
            // Log initialization failure for debugging
            appendLog("❌ SDK initialization failed: ${error.message}")
            appendLog("   SDK will remain null - operations requiring SDK will be skipped")
        }
    }
    
    /**
     * Start mesh watchdog to ensure scanning/advertising stay active
     * BLE scanning stops after ~30 seconds (Android timeout), so we need to restart it
     */
    private fun startMeshWatchdog() {
        if (meshWatchdogJob?.isActive == true) {
            return
        }
        
        meshWatchdogJob = serviceScope.launch {
            while (isActive) {
                try {
                    delay(35_000) // Check every 35 seconds (after typical scan timeout)
                    
                    // Skip if Bluetooth is disabled
                    if (bluetoothAdapter?.isEnabled != true) {
                        continue
                    }
                    
                    // Skip if already connected (no need to scan/advertise)
                    if (_connectionState.value == ConnectionState.CONNECTED) {
                        continue
                    }
                    
                    // Restart advertising if it stopped
                    if (!_isAdvertising.value) {
                        appendLog("🔄 Mesh watchdog: Advertising stopped - restarting...")
                        startAdvertising()
                    }
                    
                    // Restart scanning if it stopped (and not connected, and no pending connection)
                    if (!_isScanning.value && connectedDevice == null && clientGatt == null && pendingConnectionDevice == null) {
                        appendLog("🔄 Mesh watchdog: Scanning stopped - restarting...")
                        startScanning()
                    }
                    
                    // Check for connection timeout
                    if (pendingConnectionDevice != null) {
                        val elapsed = System.currentTimeMillis() - connectionAttemptTime
                        if (elapsed > CONNECTION_TIMEOUT_MS) {
                            appendLog("⏱️ Connection attempt to ${pendingConnectionDevice?.address} timed out after ${elapsed}ms")
                            pendingConnectionDevice = null
                            // Restart scanning if it stopped
                            if (!_isScanning.value) {
                                appendLog("🔄 Restarting scan after connection timeout...")
                                startScanning()
                            }
                        }
                    }
                    
                } catch (e: Exception) {
                    appendLog("❌ Mesh watchdog error: ${e.message}")
                    delay(60_000) // Wait longer on error
                }
            }
        }
        
        appendLog("✅ Mesh watchdog started - will keep scanning/advertising active")
    }
    
    /**
     * Start alternating mesh mode - switches between scanning and advertising
     * This ensures at least some devices are scanning while others advertise
     * Prevents deadlock where all devices advertise with nobody scanning
     */
    private fun startAlternatingMeshMode() {
        if (alternatingMeshJob?.isActive == true) {
            appendLog("🔄 Alternating mesh already running")
            return
        }
        
        appendLog("🚀 Starting alternating mesh mode (8s scan ↔ 8s advertise)")
        
        // Randomize starting mode to ensure ~50% devices scan first, ~50% advertise first
        val startWithScan = Random.nextBoolean()
        appendLog("   Starting with: ${if (startWithScan) "SCAN" else "ADVERTISE"} mode")
        
        alternatingMeshJob = serviceScope.launch {
            var scanMode = startWithScan
            
            while (isActive) {
                // Skip alternating if connected (transfer in progress)
                if (_connectionState.value == ConnectionState.CONNECTED) {
                    delay(1000)
                    continue
                }
                
                // Skip if Bluetooth disabled
                if (bluetoothAdapter?.isEnabled != true) {
                    delay(1000)
                    continue
                }
                
                // Skip if already have a pending connection attempt
                if (pendingConnectionDevice != null) {
                    delay(1000)
                    continue
                }
                
                if (scanMode) {
                    // Scan mode - actively look for peers
                    appendLog("🔄 Alternating mesh: → SCAN mode (${ALTERNATING_INTERVAL_MS / 1000}s)")
                    stopAdvertising()
                    delay(500) // Small gap to avoid BLE conflicts
                    startScanning()
                    delay(ALTERNATING_INTERVAL_MS)
                } else {
                    // Advertise mode - wait for peers to find us
                    appendLog("🔄 Alternating mesh: → ADVERTISE mode (${ALTERNATING_INTERVAL_MS / 1000}s)")
                    stopScanning()
                    delay(500) // Small gap to avoid BLE conflicts
                    startAdvertising()
                    delay(ALTERNATING_INTERVAL_MS)
                }
                
                // Toggle mode for next iteration
                scanMode = !scanMode
            }
        }
        
        appendLog("✅ Alternating mesh mode started - automatic peer discovery active")
    }
    
    /**
     * Stop alternating mesh mode
     */
    private fun stopAlternatingMeshMode() {
        alternatingMeshJob?.cancel()
        alternatingMeshJob = null
        stopScanning()
        stopAdvertising()
        appendLog("🛑 Alternating mesh mode stopped")
    }
    
    /**
     * Start auto-save job for queue persistence
     * Phase 5.2: Auto-save on changes with debouncing
     */
    private fun startAutoSaveJob() {
        if (autoSaveJob?.isActive == true) {
            return
        }
        
        autoSaveJob = serviceScope.launch {
            while (isActive) {
                try {
                    delay(10_000) // Check every 10 seconds
                    
                    // Auto-save queues (debounced internally to 5s)
                    sdk?.autoSaveQueues()?.onFailure { error ->
                        appendLog("⚠️ Auto-save failed: ${error.message}")
                    }
                    
                } catch (e: Exception) {
                    appendLog("❌ Auto-save job error: ${e.message}")
                    delay(30_000) // Wait longer on error
                }
            }
        }
        
        appendLog("✅ Auto-save job started")
    }
    
    /**
     * Schedule WorkManager background tasks
     * Phase 4.6 & 4.8: Battery-optimized scheduled work
     */
    private fun scheduleBackgroundTasks() {
        try {
            // Schedule retry worker (every 15 minutes, network required)
            RetryWorker.schedule(this)
            
            // Schedule cleanup worker (every 30 minutes)
            CleanupWorker.schedule(this)
            
            appendLog("✅ WorkManager tasks scheduled successfully")
        } catch (e: Exception) {
            appendLog("⚠️ Failed to schedule WorkManager tasks: ${e.message}")
        }
    }
    
    /**
     * Cancel all WorkManager background tasks
     */
    private fun cancelBackgroundTasks() {
        try {
            RetryWorker.cancel(this)
            CleanupWorker.cancel(this)
            appendLog("✅ WorkManager tasks cancelled")
        } catch (e: Exception) {
            appendLog("⚠️ Failed to cancel WorkManager tasks: ${e.message}")
        }
    }

    /**
     * Start BLE scanning for PolliNet devices
     */
    @SuppressLint("MissingPermission")
    fun startScanning() {
        // Check if Bluetooth is enabled
        if (bluetoothAdapter?.isEnabled != true) {
            appendLog("❌ Cannot start scanning: Bluetooth is disabled")
            appendLog("📱 Please enable Bluetooth in Settings")
            return
        }
        
        // Don't scan if already connected (prevents conflicts)
        if (connectedDevice != null || clientGatt != null) {
            appendLog("⚠️ Already connected - scan cancelled to avoid conflicts")
            appendLog("   Disconnect first before scanning for new peers")
            return
        }
        
        bleScanner?.let { scanner ->
            appendLog("🔍 Starting BLE scan for PolliNet peers")
            val scanFilter = ScanFilter.Builder()
                .setServiceUuid(android.os.ParcelUuid(SERVICE_UUID))
                .build()
            
            val scanSettings = ScanSettings.Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
                .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
                .setReportDelay(0) // Report results immediately (no batching)
                .build()
            
            scanner.startScan(listOf(scanFilter), scanSettings, scanCallback)
            _connectionState.value = ConnectionState.SCANNING
            _isScanning.value = true
        } ?: run {
            appendLog("❌ BLE scanner unavailable")
            appendLog("Possible reasons:")
            appendLog("  • Bluetooth is disabled - check Settings")
            appendLog("  • Device doesn't support BLE")
            appendLog("  • Required permissions not granted")
        }
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
     * For mesh networking, devices should both advertise AND scan (dual role)
     */
    @SuppressLint("MissingPermission")
    fun startAdvertising() {
        // Check if Bluetooth is enabled
        if (bluetoothAdapter?.isEnabled != true) {
            appendLog("❌ Cannot start advertising: Bluetooth is disabled")
            appendLog("📱 Please enable Bluetooth in Settings")
            return
        }
        
        bleAdvertiser?.let { advertiser ->
            appendLog("📣 Starting advertising (for mesh peer discovery)")
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
            
            // For mesh networking: Optionally start scanning after advertising is stable
            // This is disabled by default to avoid connection conflicts
            // Enable if you need automatic peer discovery
            // if (!_isScanning.value && connectedDevice == null) {
            //     appendLog("📡 Will auto-scan for peers after advertising stabilizes...")
            //     mainHandler.postDelayed({
            //         if (connectedDevice == null) {
            //             startScanning()
            //         }
            //     }, 5000) // Long delay to ensure stability
            // }
            appendLog("ℹ️ Auto-scanning disabled to prevent connection conflicts")
            appendLog("   Manually call startScanning() if needed for peer discovery")
        } ?: run {
            appendLog("❌ BLE advertiser unavailable")
            appendLog("Possible reasons:")
            appendLog("  • Bluetooth is disabled - check Settings")
            appendLog("  • Device doesn't support BLE advertising")
            appendLog("  • Required permissions not granted")
        }
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
        
        // CRITICAL: Don't start sending until SDK is initialized
        if (sdk == null) {
            appendLog("⚠️ SDK not initialized - sending loop will start after initialization")
            return
        }
        
        if (_connectionState.value != ConnectionState.CONNECTED) {
            appendLog("⚠️ Not connected - fragments will be sent when connection established")
            return
        }
        
        // CRITICAL: Don't start sending until descriptor write completes (client mode)
        // In server mode, we can send immediately
        if (clientGatt != null && !descriptorWriteComplete) {
            appendLog("⚠️ Waiting for descriptor write to complete before sending...")
            appendLog("   This ensures receiver is ready to receive notifications")
            return
        }
        
        appendLog("🚀 Starting sending loop")
        sendingJob = serviceScope.launch {
            while (_connectionState.value == ConnectionState.CONNECTED) {
                sendNextOutbound()
                // Increased delay per Android BLE best practices
                // 500ms was too aggressive, causing connection degradation
                // 800ms provides better stability for notification-based transfers
                delay(800) // Increased from 500ms for better stability
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
            // Check connection state first (Android best practice)
            if (_connectionState.value != ConnectionState.CONNECTED) {
                appendLog("⚠️ Not connected, dropping fragment")
                return
            }
            
            if (operationInProgress.get()) {
                // Operation already in progress, skip
                return
            }

            val sdkInstance = sdk ?: run {
                appendLog("⚠️ sendNextOutbound: SDK is null")
                return
            }
            
            // BLE safe fragment size: dynamically tied to negotiated MTU
            // Use currentMtu - 10 to ensure reliable transmission (10 bytes safety margin)
            val safeMaxLen = (currentMtu - 10).coerceAtLeast(20) // guard against too small values
            val data = sdkInstance.nextOutbound(maxLen = safeMaxLen)
            
            if (data == null) {
                // Outbound queue is empty.  Before disconnecting, give the remote peer a window
                // to push data back to us (the "server-send window").  We only disconnect once
                // BOTH sides have been silent for IDLE_DISCONNECT_WINDOW_MS.

                // Record the moment the queue first went empty this session.
                if (queueEmptySinceMs == 0L) {
                    queueEmptySinceMs = System.currentTimeMillis()
                    appendLog("📭 Queue empty — opening ${IDLE_DISCONNECT_WINDOW_MS / 1000}s idle window for peer to push data")
                }

                // The effective idle start is the LATER of: when our queue emptied, and when
                // we last received any data — so a burst of inbound data resets the clock.
                val idleStart = maxOf(queueEmptySinceMs, lastInboundDataMs)
                val elapsed = System.currentTimeMillis() - idleStart
                if (elapsed < IDLE_DISCONNECT_WINDOW_MS) {
                    // Still inside the window — yield and let the loop poll again after its 800ms sleep
                    return
                }

                // Window expired — proceed to finalise and disconnect
                if (pendingTransactionBytes != null) {
                    appendLog("📭 Idle window expired (${elapsed}ms) — confirming delivery and disconnecting")
                    delay(500) // Small buffer to flush final fragments

                    if (_connectionState.value == ConnectionState.CONNECTED) {
                        appendLog("✅ All fragments delivered successfully, clearing pending transaction")
                        pendingTransactionBytes = null
                        fragmentsQueuedWithMtu = 0
                        queueEmptySinceMs = 0L

                        // Dancing mesh: disconnect and move to next peer
                        appendLog("🔄 Dancing mesh: Transfer complete - disconnecting to find next peer...")
                        mainHandler.postDelayed({
                            if (_connectionState.value == ConnectionState.CONNECTED) {
                                appendLog("🔌 Dancing mesh: Disconnecting from current peer...")
                                closeGattConnection()
                            }
                        }, 200)
                    } else {
                        appendLog("⚠️ Connection lost, keeping transaction for potential retry")
                        queueEmptySinceMs = 0L
                    }
                }
                return
            }

            // We have data to send — reset the idle-window tracker
            queueEmptySinceMs = 0L

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
        appendLog("📤 sendToGatt: Attempting to send ${data.size} bytes")
        appendLog("   Server path: server=${gattServer != null}, txChar=${gattCharacteristicTx != null}, device=${connectedDevice?.address}")
        appendLog("   Client path: gatt=${clientGatt != null}, remoteRx=${remoteRxCharacteristic != null}")
        
        // CRITICAL FIX: Prioritize client path when we have an active client connection
        // This prevents dual-role confusion where device tries to notify AND write
        val gatt = clientGatt
        val remoteRx = remoteRxCharacteristic
        
        // If we have a client connection, ALWAYS use client path (write to remote RX)
        if (gatt != null && remoteRx != null) {
            appendLog("   → Using CLIENT path (write to remote RX)")
            appendLog("   Writing to device: ${gatt.device.address}")
            appendLog("   RX characteristic UUID: ${remoteRx.uuid}")
            appendLog("   Data preview: ${data.take(20).joinToString(" ") { "%02X".format(it) }}...")

            // Mark operation in progress for client writes
            // Edge Case Fix #8: Atomic check-and-set prevents race conditions
            if (!operationInProgress.compareAndSet(false, true)) {
                appendLog("⚠️ Operation in progress, queuing fragment")
                safelyQueueFragment(data, "Client write path - operation in progress")
                return
            }
            
            // Use official sample's write pattern (Android 13+ vs older)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                val result = gatt.writeCharacteristic(
                    remoteRx,
                    data,
                    BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                )
                appendLog("✅ Wrote ${data.size}B (result=$result) to ${gatt.device.address}")
                if (result != BluetoothGatt.GATT_SUCCESS) {
                    appendLog("   ⚠️ Write result indicates failure: $result")
                    operationInProgress.set(false)
                } else {
                    // Watchdog: if onCharacteristicWrite callback never fires (BLE stack stall),
                    // release the flag after 5s so the send loop doesn't deadlock permanently.
                    mainHandler.postDelayed({
                        if (operationInProgress.get()) {
                            appendLog("⚠️ Write callback timeout (5s) — force-releasing operationInProgress")
                            operationInProgress.set(false)
                        }
                    }, 5_000L)
                }
            } else {
                remoteRx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                @Suppress("DEPRECATION")
                remoteRx.value = data
                @Suppress("DEPRECATION")
                val success = gatt.writeCharacteristic(remoteRx)
                appendLog(if (success) "✅ Wrote ${data.size}B to ${gatt.device.address}" else "❌ Write failed to ${gatt.device.address}")
                if (!success) {
                    operationInProgress.set(false)
                } else {
                    // Same watchdog for legacy path
                    mainHandler.postDelayed({
                        if (operationInProgress.get()) {
                            appendLog("⚠️ Write callback timeout (5s) — force-releasing operationInProgress")
                            operationInProgress.set(false)
                        }
                    }, 5_000L)
                }
            }
            return
        }
        
        // Fallback: Use server path only if no client connection exists
        // This is for when we're purely acting as a server (peripheral)
        val server = gattServer
        val txChar = gattCharacteristicTx
        val device = connectedDevice
        
        if (server != null && txChar != null && device != null) {
            appendLog("   → Using SERVER path (notify) - no client connection")
            // Add flow control for server path (critical fix)
            // Android docs: notifyCharacteristicChanged() returns when queued, not when delivered
            // Edge Case Fix #8: Atomic check-and-set prevents race conditions
            if (!operationInProgress.compareAndSet(false, true)) {
                appendLog("⚠️ Operation in progress, queuing fragment")
                safelyQueueFragment(data, "Server notify path - operation in progress")
                return
            }
            txChar.value = data
            val success = server.notifyCharacteristicChanged(device, txChar, false)
            
            if (success) {
                appendLog("✅ Sent ${data.size}B via notify (queued) to ${device.address}")
                appendLog("   Data preview: ${data.take(20).joinToString(" ") { "%02X".format(it) }}...")
                // Clear flag after delay to allow notification queue processing
                // Android BLE best practice: space out notifications to avoid overwhelming connection
                // Increased from 150ms to 300ms for better reliability
                mainHandler.postDelayed({
                    operationInProgress.set(false)
                    processOperationQueue()
                }, 300) // 300ms delay ensures notification is actually delivered
            } else {
                appendLog("❌ Notify failed")
                operationInProgress.set(false)
                safelyQueueFragment(data, "Server notify failure - retry needed")
            }
            return
        }

        // No valid path available
        appendLog("⚠️ No valid GATT path available for sending")
        appendLog("   clientGatt: ${clientGatt != null}, remoteRxCharacteristic: ${remoteRx != null}")
        appendLog("   gattServer: ${server != null}, gattCharacteristicTx: ${txChar != null}, connectedDevice: ${device != null}")
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
        
        // Re-acquire Bluetooth manager (may have changed)
        bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
        
        if (bluetoothManager == null) {
            android.util.Log.e("BleService", "initializeBluetooth: Failed to get BluetoothManager")
            throw IllegalStateException("BluetoothManager not available")
        }
        
        // Re-acquire Bluetooth adapter (may have changed)
        bluetoothAdapter = bluetoothManager?.adapter
        if (bluetoothAdapter == null) {
            android.util.Log.e("BleService", "initializeBluetooth: BluetoothAdapter is null")
            throw IllegalStateException("BluetoothAdapter not available")
        }
        
        if (!bluetoothAdapter!!.isEnabled) {
            android.util.Log.w("BleService", "initializeBluetooth: Bluetooth is not enabled")
            appendLog("⚠️ Bluetooth is disabled. Please enable Bluetooth to use PolliNet.")
            appendLog("📱 Go to Settings → Bluetooth and turn it on")
            throw IllegalStateException("Bluetooth is not enabled. Please enable Bluetooth in device settings.")
        }
        
        // Re-acquire BLE components (may have changed after BT off/on)
        bleScanner = bluetoothAdapter?.bluetoothLeScanner
        bleAdvertiser = bluetoothAdapter?.bluetoothLeAdvertiser
        
        // Verify BLE components are available
        if (bleScanner == null) {
            android.util.Log.e("BleService", "initializeBluetooth: BLE scanner is null")
            appendLog("❌ BLE scanner unavailable - device may not support BLE")
        }
        
        if (bleAdvertiser == null) {
            android.util.Log.e("BleService", "initializeBluetooth: BLE advertiser is null")
            appendLog("❌ BLE advertiser unavailable - device may not support BLE advertising")
            appendLog("Note: Some devices or Android versions may not support BLE advertising")
        }
        
        android.util.Log.d("BleService", "initializeBluetooth: Setting up GATT server")
        setupGattServer()
        android.util.Log.d("BleService", "initializeBluetooth: GATT server setup complete")
        appendLog("✅ Bluetooth initialized")
        appendLog("   Scanner: ${if (bleScanner != null) "✅" else "❌"}")
        appendLog("   Advertiser: ${if (bleAdvertiser != null) "✅" else "❌"}")
    }
    
    /**
     * Start monitoring for permission changes
     * Detects when permissions are newly granted and recovers service operations
     */
    private fun startPermissionMonitoring() {
        // Cancel existing monitoring if any
        permissionMonitoringJob?.cancel()
        
        // Check initial permission state
        lastKnownPermissionState = hasRequiredPermissions()
        
        if (!lastKnownPermissionState) {
            appendLog("⚠️ Permissions not granted - monitoring for permission grant")
            appendLog("   Service will automatically recover when permissions are granted")
        }
        
        // Monitor permissions periodically (every 5 seconds)
        permissionMonitoringJob = serviceScope.launch {
            while (isActive) {
                delay(5000) // Check every 5 seconds
                
                val currentPermissionState = hasRequiredPermissions()
                
                // Detect permission grant transition (false -> true)
                if (!lastKnownPermissionState && currentPermissionState) {
                    android.util.Log.d("BleService", "Permission monitoring: Permissions granted!")
                    appendLog("✅ Permissions granted - recovering service operations")
                    
                    // Restart service operations
                    handlePermissionGranted()
                }
                
                // Detect permission revocation (true -> false)
                if (lastKnownPermissionState && !currentPermissionState) {
                    android.util.Log.w("BleService", "Permission monitoring: Permissions revoked!")
                    appendLog("⚠️ Permissions revoked - pausing service operations")
                    
                    // Stop operations
                    handlePermissionRevoked()
                }
                
                lastKnownPermissionState = currentPermissionState
            }
        }
    }
    
    /**
     * Handle recovery when permissions are newly granted
     */
    private fun handlePermissionGranted() {
        serviceScope.launch {
            try {
                // Start foreground service if not already started
                if (!isForegroundServiceRunning()) {
                    startForeground()
                }
                
                // Request battery optimization exemption
                requestBatteryOptimizationExemption()
                
                // Initialize Bluetooth if enabled
                if (bluetoothAdapter?.isEnabled == true) {
                    android.util.Log.d("BleService", "handlePermissionGranted: Initializing Bluetooth")
                    initializeBluetooth()
                    android.util.Log.d("BleService", "handlePermissionGranted: Bluetooth initialized successfully")
                    
                    // Auto-start alternating mesh mode
                    appendLog("🚀 Auto-starting alternating mesh mode")
                    startAlternatingMeshMode()
                } else {
                    appendLog("ℹ️ Bluetooth not enabled - operations will resume when BT is turned on")
                }
                
                // Reset error state
                if (_connectionState.value == ConnectionState.ERROR) {
                    _connectionState.value = ConnectionState.DISCONNECTED
                }
                
                appendLog("✅ Service recovered successfully after permission grant")
            } catch (e: Exception) {
                android.util.Log.e("BleService", "Failed to recover after permission grant", e)
                appendLog("❌ Failed to recover operations: ${e.message}")
                _connectionState.value = ConnectionState.ERROR
            }
        }
    }
    
    /**
     * Handle service pause when permissions are revoked
     */
    private fun handlePermissionRevoked() {
        // Stop all BLE operations
        stopScanning()
        stopAdvertising()
        closeGattConnection()
        
        // Clear Bluetooth components (they require permissions to use)
        bleScanner = null
        bleAdvertiser = null
        // Note: Keep bluetoothAdapter and bluetoothManager for state checking
        
        // Update connection state
        _connectionState.value = ConnectionState.ERROR
        
        appendLog("⏸️ Service paused due to permission revocation")
        appendLog("   Operations will resume automatically when permissions are re-granted")
    }
    
    /**
     * Check if foreground service is running
     */
    private fun isForegroundServiceRunning(): Boolean {
        return try {
            val activityManager = getSystemService(Context.ACTIVITY_SERVICE) as? android.app.ActivityManager
            activityManager?.getRunningServices(Integer.MAX_VALUE)?.any { service ->
                service.service.className == this::class.java.name && service.foreground
            } ?: false
        } catch (e: Exception) {
            false
        }
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
        @SuppressLint("MissingPermission")
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val peerAddress = result.device.address
            
            appendLog("📡 Discovered PolliNet device $peerAddress (RSSI: ${result.rssi} dBm)")

            // Record in peer map + Rust health monitor (regardless of connection arbitration)
            val alreadyConnected = connectedDevice?.address == peerAddress ||
                    clientGatt?.device?.address == peerAddress
            recordPeer(peerAddress, result.rssi, connected = alreadyConnected)

            // Check if already connected to THIS device
            if (alreadyConnected) {
                appendLog("ℹ️ Already connected to this device, ignoring")
                return
            }

            // Peer cooldown — skip devices we recently disconnected from so the mesh can
            // rotate to a different neighbour instead of re-locking to the same peer.
            val cooldownExpiry = recentlyConnectedPeers[peerAddress]
            if (cooldownExpiry != null && System.currentTimeMillis() < cooldownExpiry) {
                val remainingSecs = (cooldownExpiry - System.currentTimeMillis()) / 1000
                appendLog("⏳ Peer $peerAddress in cooldown for ${remainingSecs}s — skipping")
                return
            }

            // Check if already connected to ANY device (keep it simple - one connection at a time)
            if (connectedDevice != null || clientGatt != null) {
                appendLog("ℹ️ Already connected to another device, ignoring discovery")
                appendLog("   Current server: ${connectedDevice?.address}")
                appendLog("   Current client: ${clientGatt?.device?.address}")
                return
            }
            
            // Check if we're already attempting to connect to this device
            if (pendingConnectionDevice?.address == peerAddress) {
                val elapsed = System.currentTimeMillis() - connectionAttemptTime
                if (elapsed < CONNECTION_TIMEOUT_MS) {
                    appendLog("ℹ️ Connection attempt to $peerAddress already in progress (${elapsed}ms ago)")
                    return
                } else {
                    appendLog("⚠️ Previous connection attempt to $peerAddress timed out, retrying...")
                }
            }
            
            // Connection arbitration using MAC address comparison
            val myAddress = bluetoothAdapter?.address ?: "00:00:00:00:00:00"
            val shouldInitiateConnection = myAddress < peerAddress
            
            if (!shouldInitiateConnection) {
                appendLog("🔀 Arbitration: My MAC ($myAddress) > Peer MAC ($peerAddress)")
                appendLog("   → Acting as SERVER - peer should connect to me")
                appendLog("   → Continuing to scan/advertise (connection decoupled from discovery)")
                // DON'T stop scanning - keep discovery active!
                return
            }
            
            appendLog("🔀 Arbitration: My MAC ($myAddress) < Peer MAC ($peerAddress)")
            appendLog("   → Acting as CLIENT - initiating connection to peer...")
            
            // Mark connection attempt (but keep scanning active)
            pendingConnectionDevice = result.device
            connectionAttemptTime = System.currentTimeMillis()
            
            // Attempt connection without stopping scan (decoupled)
            mainHandler.postDelayed({
                // Double-check we're not already connected
                if (connectedDevice == null && clientGatt == null) {
                appendLog("🔗 Connecting to $peerAddress as GATT client...")
                connectToDevice(result.device)
                } else {
                    appendLog("⚠️ Connection state changed, cancelling connection attempt")
                    pendingConnectionDevice = null
                }
            }, 300) // Small delay to avoid rapid-fire connection attempts
        }

        override fun onScanFailed(errorCode: Int) {
            _isScanning.value = false
            _connectionState.value = ConnectionState.ERROR
            appendLog("❌ Scan failed (code $errorCode)")
            
            // Auto-restart scanning after a delay (unless connected)
            if (connectedDevice == null && clientGatt == null && bluetoothAdapter?.isEnabled == true) {
                appendLog("🔄 Auto-restarting scan after failure...")
                mainHandler.postDelayed({
                    if (connectedDevice == null && clientGatt == null && !_isScanning.value) {
                        startScanning()
                    }
                }, 2000) // 2 second delay before retry
            }
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
            
            // Auto-restart advertising after a delay (unless connected)
            if (connectedDevice == null && clientGatt == null && bluetoothAdapter?.isEnabled == true) {
                appendLog("🔄 Auto-restarting advertising after failure...")
                mainHandler.postDelayed({
                    if (connectedDevice == null && clientGatt == null && !_isAdvertising.value) {
                        startAdvertising()
                    }
                }, 2000) // 2 second delay before retry
            }
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
                        // GATT_INSUFFICIENT_AUTHENTICATION (5), GATT_INSUFFICIENT_ENCRYPTION (15)
                        appendLog("🔐 Authentication/Encryption required - creating bond...")
                        try {
                            gatt.device.createBond()
                        } catch (e: Exception) {
                            appendLog("❌ Failed to create bond: ${e.message}")
                        }
                    }
                    22 -> {
                        // GATT_INSUFFICIENT_AUTHORIZATION (22) - NOT auto-bonding unless explicitly enabled
                        appendLog("🔐 GATT_INSUFFICIENT_AUTHORIZATION (22) – NOT auto-bonding, just logging")
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
                
                // Clear pending connection on error
                if (pendingConnectionDevice?.address == gatt.device.address) {
                    pendingConnectionDevice = null
                }
                
                // Close the failed connection
                gatt.close()
                clientGatt = null
                
                return
            }
            
            // Handle connection states
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    _connectionState.value = ConnectionState.CONNECTED
                    connectedDevice = gatt.device
                    clientGatt = gatt

                    // Record in peer map + Rust health monitor
                    recordPeer(gatt.device.address, rssi = _peers.value[gatt.device.address]?.rssi ?: 0, connected = true)

                    // Clear pending connection on success
                    if (pendingConnectionDevice?.address == gatt.device.address) {
                        pendingConnectionDevice = null
                    }

                    appendLog("✅ Connected to ${gatt.device.address}")
                    
                    // Stop scanning/advertising now that we're connected
                    stopScanning()
                    stopAdvertising()
                    
                    // Request MTU for better throughput (official sample line 137)
                    // Target 247 bytes (common max) for larger fragments
                    // This will reduce fragment count from ~12 to ~3-4
                    appendLog("📏 Requesting MTU negotiation (target: 247 bytes)...")
                    appendLog("   Current default: $currentMtu bytes")
                    val mtuRequested = gatt.requestMtu(247)
                    if (!mtuRequested) {
                        appendLog("⚠️ MTU request failed, using default: $currentMtu")
                    }
                    
                    // Request high connection priority for low latency (~7.5ms interval)
                    // This improves throughput for mesh data transfer
                    val priorityResult = gatt.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
                    appendLog("⚡ Connection priority: HIGH (result=$priorityResult, ~7.5ms interval)")
                    
                    // Service discovery happens in onMtuChanged
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    _connectionState.value = ConnectionState.DISCONNECTED
                    appendLog("🔌 Disconnected from ${gatt.device.address}")

                    // Mark peer as no longer connected in local map
                    val addr = gatt.device.address
                    _peers.value = _peers.value.toMutableMap().apply {
                        get(addr)?.let { put(addr, it.copy(isConnected = false, lastSeenAt = System.currentTimeMillis())) }
                    }

                    // Clear pending connection on disconnect
                    if (pendingConnectionDevice?.address == gatt.device.address) {
                        pendingConnectionDevice = null
                    }

                    // Clean up
                    connectedDevice = null
                    clientGatt = null
                    remoteTxCharacteristic = null
                    remoteRxCharacteristic = null
                    remoteWriteInProgress = false
                    operationInProgress.set(false)
                    operationQueue.clear()
                    descriptorWriteRetries = 0
                    pendingDescriptorWrite = null
                    pendingGatt = null
                    sendingJob?.cancel()
                    
                    // Clear re-fragmentation tracking
                    // Don't clear pending transaction - it might need to be retried on reconnection
                    // pendingTransactionBytes = null
                    fragmentsQueuedWithMtu = 0
                    
                    // Reset descriptor write flag
                    descriptorWriteComplete = false

                    // Reset idle-window tracking for the next connection
                    queueEmptySinceMs = 0L
                    lastInboundDataMs = 0L

                    // Cooldown: suppress reconnection to this peer for PEER_COOLDOWN_MS so the
                    // mesh has a chance to discover a different neighbour next scan cycle.
                    recentlyConnectedPeers[addr] = System.currentTimeMillis() + PEER_COOLDOWN_MS
                    // Evict entries whose cooldown has already expired (time-based, not FIFO)
                    val now = System.currentTimeMillis()
                    recentlyConnectedPeers.entries.removeIf { it.value <= now }

                    // Dancing mesh: Automatically restart alternating mode to find next peer.
                    // Random 2–5 s backoff prevents all nodes from re-scanning simultaneously.
                    appendLog("🔄 Dancing mesh: Restarting alternating mode to find next peer...")
                    val backoffMs = (2000L..5000L).random()
                    mainHandler.postDelayed({
                        if (_connectionState.value == ConnectionState.DISCONNECTED &&
                            connectedDevice == null && clientGatt == null) {
                            startAlternatingMeshMode()
                            appendLog("✅ Dancing mesh: Alternating mode restarted - will find next peer")
                        }
                    }, backoffMs)
                }
            }
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            val oldMtu = currentMtu
            currentMtu = mtu
            val maxPayload = (mtu - 10).coerceAtLeast(20)
            val oldMaxPayload = (oldMtu - 10).coerceAtLeast(20)
            appendLog("📏 MTU negotiation complete: $oldMtu → $mtu bytes (status=$status)")
            appendLog("   Max payload per fragment: $maxPayload bytes")
            appendLog("   Expected fragments for 1KB tx: ~${1024 / maxPayload} (was ~${1024 / oldMaxPayload})")
            
            // Re-queue fragments with new MTU if significantly larger (critical optimization!)
            // This reduces fragment count from ~6 to ~4 for typical 1KB transactions
            val mtuIncrease = mtu - oldMtu
            if (mtuIncrease >= 30 && pendingTransactionBytes != null) {
                appendLog("🔄 MTU increased by $mtuIncrease bytes - re-fragmenting with larger size...")
                appendLog("   Pausing sending loop for re-fragmentation...")
                
                // Pause sending loop
                sendingJob?.cancel()
                
                // Re-fragment with new MTU
                serviceScope.launch {
                    val txBytes = pendingTransactionBytes
                    if (txBytes != null) {
                        val sdkInstance = sdk
                        if (sdkInstance != null) {
                            // Clear outbound queue (old small fragments)
                            // Note: We can't directly clear the Rust queue, but new fragments will be prioritized
                            
                            appendLog("♻️ Re-fragmenting ${txBytes.size} bytes with new MTU...")
                            val newMaxPayload = (currentMtu - 10).coerceAtLeast(20)
                            sdkInstance.fragment(txBytes, newMaxPayload).onSuccess { fragments ->
                                val newCount = fragments.fragments.size
                                val oldCount = (txBytes.size + oldMaxPayload - 1) / oldMaxPayload
                                appendLog("✅ Re-fragmented: $oldCount → $newCount fragments")
                                appendLog("   Improvement: ${((oldCount - newCount).toFloat() / oldCount * 100).toInt()}% fewer fragments")
                                
                                // Update tracking
                                fragmentsQueuedWithMtu = currentMtu
                                
                                // Restart sending loop with optimized fragments
                                ensureSendingLoopStarted()
                            }.onFailure {
                                appendLog("❌ Re-fragmentation failed: ${it.message}")
                                // Continue with old fragments
                                ensureSendingLoopStarted()
                            }
                        } else {
                            appendLog("⚠️ SDK not available for re-fragmentation")
                        }
                    }
                }
            } else if (mtuIncrease < 30) {
                appendLog("   MTU increase too small ($mtuIncrease bytes), keeping existing fragments")
            }
            
            // CRITICAL: Discover services after MTU negotiation
            appendLog("🔍 Starting service discovery...")
            val discoverSuccess = gatt.discoverServices()
            if (!discoverSuccess) {
                appendLog("❌ Failed to start service discovery!")
            }
        }

        @SuppressLint("MissingPermission")
        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            appendLog("📋 Services discovered: status=$status")
            
            if (status != BluetoothGatt.GATT_SUCCESS) {
                appendLog("❌ Service discovery failed with status: $status")
                return
            }
            
            // Log all discovered services and characteristics
            appendLog("🔍 === DISCOVERED SERVICES & CHARACTERISTICS ===")
            gatt.services.forEach { service ->
                appendLog("📦 Service: ${service.uuid}")
                appendLog("   Type: ${if (service.type == BluetoothGattService.SERVICE_TYPE_PRIMARY) "PRIMARY" else "SECONDARY"}")
                
                service.characteristics.forEach { characteristic ->
                    appendLog("   📝 Characteristic: ${characteristic.uuid}")
                    
                    // Log properties
                    val properties = mutableListOf<String>()
                    if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_READ != 0) {
                        properties.add("READ")
                    }
                    if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_WRITE != 0) {
                        properties.add("WRITE")
                    }
                    if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE != 0) {
                        properties.add("WRITE_NO_RESPONSE")
                    }
                    if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0) {
                        properties.add("NOTIFY")
                    }
                    if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_INDICATE != 0) {
                        properties.add("INDICATE")
                    }
                    appendLog("      Properties: ${properties.joinToString(", ")}")
                    
                    // Log descriptors
                    characteristic.descriptors.forEach { descriptor ->
                        appendLog("      🔖 Descriptor: ${descriptor.uuid}")
                    }
                }
            }
            appendLog("🔍 === END OF DISCOVERED SERVICES ===")
            
            // Find our PolliNet service
            val service = gatt.getService(SERVICE_UUID)
            if (service == null) {
                appendLog("⚠️ PolliNet service not found!")
                appendLog("   Expected: $SERVICE_UUID")
                appendLog("   Available services: ${gatt.services.map { it.uuid }}")
                return
            }
            
            appendLog("✅ PolliNet service found: $SERVICE_UUID")
            
            // Get our characteristics
            remoteTxCharacteristic = service.getCharacteristic(TX_CHAR_UUID)
            remoteRxCharacteristic = service.getCharacteristic(RX_CHAR_UUID)
            
            if (remoteTxCharacteristic == null || remoteRxCharacteristic == null) {
                appendLog("❌ Missing PolliNet characteristics!")
                appendLog("   TX characteristic ${if (remoteTxCharacteristic != null) "✅" else "❌"}: $TX_CHAR_UUID")
                appendLog("   RX characteristic ${if (remoteRxCharacteristic != null) "✅" else "❌"}: $RX_CHAR_UUID")
                return
            }
            
            appendLog("✅ Characteristics ready:")
            appendLog("   TX (notify): $TX_CHAR_UUID")
            appendLog("   RX (write): $RX_CHAR_UUID")
            
            // Enable notifications on TX characteristic
            val notifySuccess = gatt.setCharacteristicNotification(remoteTxCharacteristic, true)
            appendLog("📬 setCharacteristicNotification: $notifySuccess")
            
            // Write CCCD to enable remote notifications
            val descriptor = remoteTxCharacteristic?.getDescriptor(cccdUuid)
            if (descriptor == null) {
                appendLog("❌ CCCD descriptor not found!")
                appendLog("   Cannot receive notifications without CCCD")
                return
            }
            
            appendLog("✅ CCCD descriptor found: $cccdUuid")
            
            // Try descriptor write directly - no proactive bonding
            // Bonding will only occur if device requires it (status 5 or 15 in onDescriptorWrite)
            descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
            val writeSuccess = gatt.writeDescriptor(descriptor)
            appendLog("📬 Writing CCCD descriptor to enable notifications: $writeSuccess")
            
            if (!writeSuccess) {
                appendLog("⚠️ Descriptor write queuing failed!")
                appendLog("   This may indicate the GATT queue is full or device is busy")
            } else {
                appendLog("⏳ Waiting for onDescriptorWrite callback...")
                appendLog("   Data transfer will begin after descriptor write confirms")
                // Timeout: if the callback never fires (BLE stack stall or peer disappears),
                // force-mark descriptor write complete after 30s so the send loop can still start.
                mainHandler.postDelayed({
                    if (!descriptorWriteComplete && _connectionState.value == ConnectionState.CONNECTED) {
                        appendLog("⚠️ Descriptor write callback timeout (30s) — forcing send loop start")
                        descriptorWriteComplete = true
                        ensureSendingLoopStarted()
                    }
                }, 30_000L)
            }
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray
        ) {
            appendLog("🔔 NOTIFICATION RECEIVED (Client): char=${characteristic.uuid}, device=${gatt.device.address}, size=${value.size} bytes")
            appendLog("   📦 Raw data: ${value.joinToString(" ") { "%02X".format(it) }}")
            appendLog("   📋 Base64: ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            appendLog("   📝 Preview: ${previewFragment(value)}")
            
            // Forward to Rust FFI
            serviceScope.launch {
                if (sdk == null) {
                    appendLog("⚠️ SDK not initialized; inbound dropped")
                    return@launch
                }
                
                // Log received data in detail for receiver
                appendLog("⬅️ Processing notification data...")
                lastInboundDataMs = System.currentTimeMillis()
                handleReceivedData(value)
            }
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int
        ) {
            appendLog("📝 Characteristic WRITE (Client): char=${characteristic.uuid}, status=$status")
            if (characteristic.uuid == RX_CHAR_UUID) {
                operationInProgress.set(false)
                
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
            } else {
                appendLog("   ⚠️ Write to unexpected characteristic: ${characteristic.uuid}")
            }
        }

        @SuppressLint("MissingPermission")
        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int
        ) {
            appendLog("📖 Characteristic READ (Client): char=${characteristic.uuid}, status=$status")
            characteristic.value?.let { value ->
                appendLog("   Value size: ${value.size} bytes")
                appendLog("   Value: ${value.joinToString(" ") { "%02X".format(it) }}")
                appendLog("   Value (base64): ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            } ?: run {
                appendLog("   Value: null or empty")
            }
        }

        @SuppressLint("MissingPermission")
        override fun onDescriptorRead(
            gatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
            status: Int
        ) {
            appendLog("📖 Descriptor READ (Client): descriptor=${descriptor.uuid}, status=$status")
            descriptor.value?.let { value ->
                appendLog("   Value size: ${value.size} bytes")
                appendLog("   Value: ${value.joinToString(" ") { "%02X".format(it) }}")
                appendLog("   Value (base64): ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            } ?: run {
                appendLog("   Value: null or empty")
            }
        }

        @SuppressLint("MissingPermission")
        override fun onDescriptorWrite(
            gatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
            status: Int
        ) {
            appendLog("📝 Descriptor write: status=$status, connection=${_connectionState.value}")
            
            // Ignore stale callbacks - check if connection is still active
            if (_connectionState.value != ConnectionState.CONNECTED) {
                appendLog("⚠️ Ignoring descriptor write callback - connection is ${_connectionState.value}")
                return
            }
            
            // Ignore if descriptor write already completed successfully
            if (descriptorWriteComplete && status != BluetoothGatt.GATT_SUCCESS) {
                appendLog("⚠️ Ignoring failed descriptor write callback - already completed successfully")
                return
            }
            
            // Verify this is for the current GATT connection
            if (gatt != clientGatt) {
                appendLog("⚠️ Ignoring descriptor write callback - GATT mismatch (stale callback)")
                return
            }
            
            if (status == BluetoothGatt.GATT_SUCCESS) {
                appendLog("✅ Notifications enabled - ready to transfer data!")
                descriptorWriteRetries = 0
                pendingDescriptorWrite = null
                pendingGatt = null
                
                // Mark descriptor write as complete (critical for flow control)
                descriptorWriteComplete = true
                
                // Restart sending loop after successful descriptor write
                ensureSendingLoopStarted()
            } else {
                appendLog("❌ Failed to enable notifications: status=$status")
                
                // Double-check connection is still active before retrying
                if (_connectionState.value != ConnectionState.CONNECTED) {
                    appendLog("⚠️ Connection lost, aborting descriptor write retry")
                    descriptorWriteRetries = 0
                    return
                }
                
                // Handle status 133 with retry logic
                if (status == 133) {
                    // Pause sending loop while we recover (critical fix)
                    sendingJob?.cancel()
                    appendLog("⚠️ Status 133 detected - pausing sending loop for recovery")
                    
                    if (descriptorWriteRetries < MAX_DESCRIPTOR_RETRIES) {
                        descriptorWriteRetries++
                        appendLog("⚠️ Retrying descriptor write (attempt $descriptorWriteRetries/$MAX_DESCRIPTOR_RETRIES)...")
                        
                        // Refresh cache and retry
                        refreshDeviceCache(gatt)
                        
                        // Exponential backoff: wait longer between retries
                        val retryDelay = 1000L * descriptorWriteRetries // 1s, 2s, 3s
                        // Cancel any previously scheduled retry before posting a new one
                        pendingDescriptorRetry?.let { mainHandler.removeCallbacks(it) }
                        val retryRunnable = Runnable retry@{
                            pendingDescriptorRetry = null
                            // Check connection state again before retrying
                            if (_connectionState.value != ConnectionState.CONNECTED) {
                                appendLog("⚠️ Connection lost during retry delay, aborting")
                                descriptorWriteRetries = 0
                                return@retry
                            }

                            // Verify GATT is still valid
                            if (gatt != clientGatt) {
                                appendLog("⚠️ GATT connection changed during retry delay, aborting")
                                descriptorWriteRetries = 0
                                return@retry
                            }

                            try {
                                // Re-enable notifications and write descriptor
                                gatt.setCharacteristicNotification(remoteTxCharacteristic, true)
                                val retryDescriptor = remoteTxCharacteristic?.getDescriptor(cccdUuid)
                                if (retryDescriptor != null) {
                                    retryDescriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                                    pendingDescriptorWrite = retryDescriptor
                                    pendingGatt = gatt
                                    gatt.writeDescriptor(retryDescriptor)
                                    appendLog("🔄 Retrying descriptor write...")
                                } else {
                                    appendLog("❌ CCCD descriptor not found for retry")
                                }
                            } catch (e: Exception) {
                                appendLog("❌ Retry failed: ${e.message}")
                                descriptorWriteRetries = 0
                            }
                        }
                        pendingDescriptorRetry = retryRunnable
                        mainHandler.postDelayed(retryRunnable, retryDelay)
                    } else {
                        appendLog("❌ Max descriptor write retries reached. Giving up.")
                        descriptorWriteRetries = 0
                        sendingJob?.cancel() // Stop loop before GATT closes to avoid write-on-closed-gatt
                        if (_connectionState.value == ConnectionState.CONNECTED) {
                            handleStatus133(gatt)
                        }
                    }
                } else if (status == 5 || status == 15) {
                    // Authentication/Encryption required
                    appendLog("🔐 Bonding required for descriptor write - creating bond...")
                    try {
                        gatt.device.createBond()
                        // Store descriptor for retry after bonding
                        pendingDescriptorWrite = descriptor
                        pendingGatt = gatt
                    } catch (e: Exception) {
                        appendLog("❌ Failed to create bond: ${e.message}")
                    }
                } else if (status == 22) {
                    // GATT_INSUFFICIENT_AUTHORIZATION (22) - NOT auto-bonding unless explicitly enabled
                    appendLog("🔐 GATT_INSUFFICIENT_AUTHORIZATION (22) – NOT auto-bonding, just logging")
                } else {
                    // Other errors - reset retry counter
                    descriptorWriteRetries = 0
                }
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

                    // Record in peer map + Rust health monitor (server-side connection)
                    recordPeer(device.address, rssi = _peers.value[device.address]?.rssi ?: 0, connected = true)

                    // Clear pending connection on success
                    if (pendingConnectionDevice?.address == device.address) {
                        pendingConnectionDevice = null
                    }

                    // Stop scanning/advertising now that we're connected
                    stopScanning()
                    stopAdvertising()

                    appendLog("🤝 🤝 🤝 (SERVER) CONNECTED ${device.address} 🤝 🤝 🤝")
                    appendLog("   Server mode: Can send notifications immediately")
                    appendLog("   ✅ GATT server: ${gattServer != null}")
                    appendLog("   ✅ TX characteristic: ${gattCharacteristicTx != null} (UUID: $TX_CHAR_UUID)")
                    appendLog("   ✅ RX characteristic: ${gattCharacteristicRx != null} (UUID: $RX_CHAR_UUID)")
                    appendLog("   ✅ Ready to receive writes on RX characteristic: $RX_CHAR_UUID")
                    
                    // Log characteristic properties
                    gattCharacteristicRx?.let { rx ->
                        appendLog("   RX Properties: ${rx.properties}")
                        appendLog("   RX Permissions: ${rx.permissions}")
                        appendLog("   RX UUID: ${rx.uuid}")
                    }
                    
                    gattCharacteristicTx?.let { tx ->
                        appendLog("   TX Properties: ${tx.properties}")
                        appendLog("   TX Permissions: ${tx.permissions}")
                        appendLog("   TX UUID: ${tx.uuid}")
                    }
                    
                    // In server mode, we can SEND immediately (don't need descriptor write for TX)
                    // But descriptor write is still needed on client side to RECEIVE
                    // Only set flag if we don't have a client connection active
                    if (clientGatt == null) {
                        descriptorWriteComplete = true
                        appendLog("   No client connection, marking ready to send")
                    } else {
                        appendLog("   Client connection exists, waiting for its descriptor write...")
                    }
                    // Start sending loop for server mode
                    ensureSendingLoopStarted()
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    _connectionState.value = ConnectionState.DISCONNECTED
                    connectedDevice = null
                    sendingJob?.cancel()

                    // Mark peer as no longer connected in local map
                    _peers.value = _peers.value.toMutableMap().apply {
                        get(device.address)?.let { put(device.address, it.copy(isConnected = false, lastSeenAt = System.currentTimeMillis())) }
                    }

                    // Clear pending connection on disconnect
                    if (pendingConnectionDevice?.address == device.address) {
                        pendingConnectionDevice = null
                    }

                    appendLog("🔌 (Server) disconnected ${device.address}")
                    
                    // Clear re-fragmentation tracking
                    pendingTransactionBytes = null
                    fragmentsQueuedWithMtu = 0
                    
                    // Reset descriptor write flag
                    descriptorWriteComplete = false

                    // Reset idle-window tracking for the next connection
                    queueEmptySinceMs = 0L
                    lastInboundDataMs = 0L

                    // Cooldown: suppress reconnection to this peer for PEER_COOLDOWN_MS.
                    recentlyConnectedPeers[device.address] = System.currentTimeMillis() + PEER_COOLDOWN_MS
                    // Evict entries whose cooldown has already expired (time-based, not FIFO)
                    val now = System.currentTimeMillis()
                    recentlyConnectedPeers.entries.removeIf { it.value <= now }

                    // Dancing mesh: Automatically restart alternating mode to find next peer.
                    // Random 2–5 s backoff so nodes don't all re-scan at the same instant.
                    appendLog("🔄 Dancing mesh: Restarting alternating mode to find next peer...")
                    val backoffMs = (2000L..5000L).random()
                    mainHandler.postDelayed({
                        if (_connectionState.value == ConnectionState.DISCONNECTED &&
                            connectedDevice == null && clientGatt == null) {
                            startAlternatingMeshMode()
                            appendLog("✅ Dancing mesh: Alternating mode restarted - will find next peer")
                        }
                    }, backoffMs)
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
            appendLog("🎯 ===== WRITE REQUEST RECEIVED (SERVER) =====")
            appendLog("📥 Device: ${device.address}")
            appendLog("📥 Characteristic UUID: ${characteristic.uuid}")
            appendLog("📥 Expected RX UUID: $RX_CHAR_UUID")
            appendLog("📥 Data size: ${value.size} bytes")
            appendLog("📥 Response needed: $responseNeeded")
            appendLog("📥 Offset: $offset")
            appendLog("📥 Prepared write: $preparedWrite")
            appendLog("📥 Data preview (first 50 bytes): ${value.take(50).joinToString(" ") { "%02X".format(it) }}")
            appendLog("📥 Data (base64): ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            
            val uuidMatches = characteristic.uuid == RX_CHAR_UUID
            appendLog("📥 UUID match: $uuidMatches")
            
            if (uuidMatches) {
                appendLog("✅ ✅ ✅ MATCHED RX CHARACTERISTIC - PROCESSING DATA ✅ ✅ ✅")
                
                // Send response FIRST (synchronously) before processing data
                // This is critical - response must be sent in the callback thread
                if (responseNeeded) {
                    val responseSent = gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null) ?: false
                    appendLog("📤 Sent write response: $responseSent")
                    if (!responseSent) {
                        appendLog("❌ ❌ ❌ FAILED TO SEND WRITE RESPONSE ❌ ❌ ❌")
                    }
                } else {
                    appendLog("ℹ️ No response needed for this write")
                }
                
                // Forward to Rust FFI (async processing)
                lastInboundDataMs = System.currentTimeMillis() // reset idle-disconnect clock
                serviceScope.launch {
                    if (sdk == null) {
                        appendLog("❌ SDK not initialized; write dropped")
                        return@launch
                    }
                    // Log received data in detail for receiver
                    appendLog("⬅️ ⬅️ ⬅️ PROCESSING RECEIVED DATA ⬅️ ⬅️ ⬅️")
                    appendLog("⬅️ RX from ${device.address}: ${previewFragment(value)}")
                    appendLog("   📦 Raw data (${value.size} bytes): ${value.joinToString(" ") { "%02X".format(it) }}")
                    appendLog("   📋 Base64: ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")

                    handleReceivedData(value)
                }
            } else {
                appendLog("⚠️ ⚠️ ⚠️ Write to UNKNOWN characteristic ⚠️ ⚠️ ⚠️")
                appendLog("⚠️ Expected: $RX_CHAR_UUID")
                appendLog("⚠️ Received: ${characteristic.uuid}")
                // Still send response for unknown characteristics to avoid client timeout
                if (responseNeeded) {
                    val responseSent = gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED, 0, null) ?: false
                    appendLog("📤 Sent error response: $responseSent")
                }
            }
            appendLog("🎯 ===== END WRITE REQUEST =====\n")
        }

        @SuppressLint("MissingPermission")
        override fun onCharacteristicReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            characteristic: BluetoothGattCharacteristic
        ) {
            appendLog("📖 READ request: char=${characteristic.uuid}, offset=$offset, from=${device.address}")
            appendLog("   Characteristic value: ${characteristic.value?.size ?: 0} bytes")
            
            // Log the actual value if present
            characteristic.value?.let { value ->
                appendLog("   Value: ${value.joinToString(" ") { "%02X".format(it) }}")
                appendLog("   Value (base64): ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            }
            
            // Send response (default: not supported for our use case)
            val status = if (characteristic.uuid == TX_CHAR_UUID || characteristic.uuid == RX_CHAR_UUID) {
                appendLog("   ✅ Allowing read for PolliNet characteristic")
                BluetoothGatt.GATT_SUCCESS
            } else {
                appendLog("   ⚠️ Read not supported for this characteristic")
                BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED
            }
            
            gattServer?.sendResponse(device, requestId, status, offset, characteristic.value)
            appendLog("   📤 Sent read response: status=$status")
        }

        @SuppressLint("MissingPermission")
        override fun onDescriptorReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            descriptor: BluetoothGattDescriptor
        ) {
            appendLog("📖 DESCRIPTOR READ request: descriptor=${descriptor.uuid}, offset=$offset, from=${device.address}")
            appendLog("   Descriptor value: ${descriptor.value?.size ?: 0} bytes")
            
            // Log the actual value if present
            descriptor.value?.let { value ->
                appendLog("   Value: ${value.joinToString(" ") { "%02X".format(it) }}")
                appendLog("   Value (base64): ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            }
            
            // Send response
            val status = BluetoothGatt.GATT_SUCCESS
            gattServer?.sendResponse(device, requestId, status, offset, descriptor.value)
            appendLog("   📤 Sent descriptor read response: status=$status")
        }

        @SuppressLint("MissingPermission")
        override fun onDescriptorWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray
        ) {
            appendLog("📝 DESCRIPTOR WRITE request: descriptor=${descriptor.uuid}, size=${value.size}, responseNeeded=$responseNeeded, offset=$offset, from=${device.address}")
            appendLog("   Value: ${value.joinToString(" ") { "%02X".format(it) }}")
            appendLog("   Value (base64): ${android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)}")
            
            // Handle CCCD descriptor writes (for enabling notifications)
            if (descriptor.uuid == cccdUuid) {
                appendLog("   ✅ CCCD descriptor write - notifications ${if (value.contentEquals(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)) "ENABLED" else "DISABLED"}")
            }
            
            // Send response
            if (responseNeeded) {
                val responseSent = gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null) ?: false
                appendLog("   📤 Sent descriptor write response: $responseSent")
            }
        }

        override fun onExecuteWrite(device: BluetoothDevice, requestId: Int, execute: Boolean) {
            appendLog("📋 EXECUTE WRITE: device=${device.address}, requestId=$requestId, execute=$execute")
            gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            appendLog("📬 NOTIFICATION SENT: device=${device.address}, status=$status")
            if (status != BluetoothGatt.GATT_SUCCESS) {
                appendLog("   ❌ Notification send failed with status: $status")
            }
        }

        override fun onMtuChanged(device: BluetoothDevice, mtu: Int) {
            appendLog("📏 MTU CHANGED (Server): device=${device.address}, mtu=$mtu")
            val oldMtu = currentMtu
            currentMtu = mtu
            val maxPayload = (mtu - 10).coerceAtLeast(20)
            appendLog("   MTU: $oldMtu → $mtu bytes, maxPayload=$maxPayload bytes")
        }

        override fun onPhyUpdate(device: BluetoothDevice, txPhy: Int, rxPhy: Int, status: Int) {
            appendLog("📡 PHY UPDATE: device=${device.address}, txPhy=$txPhy, rxPhy=$rxPhy, status=$status")
        }

        override fun onPhyRead(device: BluetoothDevice, txPhy: Int, rxPhy: Int, status: Int) {
            appendLog("📡 PHY READ: device=${device.address}, txPhy=$txPhy, rxPhy=$rxPhy, status=$status")
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
                NotificationManager.IMPORTANCE_HIGH  // High priority for persistent service
            ).apply {
                description = "Manages PolliNet Bluetooth connections"
                // Enable sound, vibration, and lights for high priority
                enableLights(false)
                enableVibration(false)  // Disable vibration to avoid annoyance
                setShowBadge(false)
                // Set lockscreen visibility to show on lock screen
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            }
            
            val notificationManager = getSystemService(NotificationManager::class.java)
            notificationManager.createNotificationChannel(channel)
        }
    }

    /**
     * Request exemption from battery optimization to ensure the service
     * continues running even when the app is in the background or killed.
     * 
     * This is called automatically when the service starts.
     */
    private fun requestBatteryOptimizationExemption() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            try {
                val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
                val packageName = packageName
                
                if (!powerManager.isIgnoringBatteryOptimizations(packageName)) {
                    android.util.Log.d("BleService", "Battery optimization not exempted - user should grant exemption")
                    appendLog("⚠️ Battery optimization exemption recommended for persistent operation")
                    appendLog("   Go to Settings > Battery > Battery optimization to exempt this app")
                } else {
                    android.util.Log.d("BleService", "Battery optimization exemption already granted")
                    appendLog("✅ Battery optimization exemption active")
                }
            } catch (e: Exception) {
                android.util.Log.w("BleService", "Failed to check battery optimization status", e)
            }
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
            .setContentText("Managing BLE mesh connections")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setOngoing(true)  // Persistent notification - cannot be dismissed
            .setPriority(NotificationCompat.PRIORITY_HIGH)  // High priority
            .setCategory(NotificationCompat.CATEGORY_SERVICE)  // Service category
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)  // Show on lock screen
            .setShowWhen(false)  // Don't show timestamp for persistent service
            .setOnlyAlertOnce(true)  // Only alert once, don't repeat
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
        if (operationInProgress.get() || operationQueue.isEmpty()) return
        
        val data = operationQueue.poll() ?: return
        operationInProgress.set(true)
        
        appendLog("📤 Processing queued operation (${data.size} bytes)")
        sendToGatt(data)
    }

    private fun appendLog(message: String) {
        val timestamp = SimpleDateFormat("HH:mm:ss", Locale.getDefault()).format(Date())
        val entry = "[$timestamp] $message"
        val current = _logs.value
        _logs.value = (current + entry).takeLast(50)
        
        // Also log to Android Logcat for easy console access
        // Use different log levels based on message content
        when {
            message.startsWith("❌") || message.contains("failed", ignoreCase = true) || 
            message.contains("error", ignoreCase = true) -> {
                android.util.Log.e("PolliNet.BLE", message)
            }
            message.startsWith("⚠️") || message.contains("warning", ignoreCase = true) ||
            message.contains("retry", ignoreCase = true) -> {
                android.util.Log.w("PolliNet.BLE", message)
            }
            message.startsWith("✅") || message.startsWith("🎉") -> {
                android.util.Log.i("PolliNet.BLE", "✓ ${message.substring(2)}")
            }
            message.startsWith("📏") || message.startsWith("📤") || message.startsWith("📥") ||
            message.startsWith("➡️") || message.startsWith("⬅️") -> {
                android.util.Log.d("PolliNet.BLE", message)
            }
            else -> {
                android.util.Log.d("PolliNet.BLE", message)
            }
        }
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