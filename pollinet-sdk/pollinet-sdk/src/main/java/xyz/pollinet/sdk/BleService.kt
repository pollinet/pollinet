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
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
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
    
    /** Emitted when a confirmation is received for a transaction originated on this device. */
    sealed class ConfirmationEvent {
        data class Success(val txIdShort: String, val detail: String) : ConfirmationEvent()
        data class Failure(val txIdShort: String, val error: String) : ConfirmationEvent()
    }

    /** Emitted when the rotation policy force-disconnects a peer for fairness. */
    sealed class RotationEvent {
        data class Forced(
            val peerAddress: String,
            val sessionMs: Long,
            val visiblePeers: Int
        ) : RotationEvent()
    }

    /** Lifecycle status of a transaction received over the BLE mesh. */
    enum class ReceivedTxStatus { RECEIVED, SUBMITTED, RELAYED, FAILED }

    /** A snapshot of one received transaction's current state, displayed on the Dev screen. */
    data class ReceivedTxRecord(
        val txId: String,
        val status: ReceivedTxStatus,
        val timestamp: Long,
        val signature: String? = null,
        val error: String? = null,
        val relayHop: Int? = null
    )

    /** A single received confirmation entry. */
    data class ConfirmationRecord(
        val txIdShort: String,
        val success: Boolean,
        val detail: String,
        val timestamp: Long
    )

    companion object {
        // Process-scoped reference to the running BleService instance. Mirrors SdkHolder so
        // callers (e.g. ViewModels that don't have a service binder) can route work through the
        // single SDK that the BLE sending loop is actually polling. Cleared in onDestroy so the
        // service can be GC'd. Returns null when the service is not running yet.
        @Volatile private var runningInstance: BleService? = null
        @JvmStatic fun get(): BleService? = runningInstance

        private const val NOTIFICATION_ID = 1001
        private const val CONFIRMATION_NOTIFICATION_ID = 1002
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

        // Cap for the on-screen received-tx and confirmation history (oldest entries dropped).
        private const val MAX_TX_LOG_SIZE = 50

        // Hard cap on fragment payload regardless of currentMtu. Android negotiates 247 ATT MTU
        // by default (we request 247); subtracting 10 bytes of header overhead leaves a payload
        // that fits on virtually every peer. Using a stale or pre-negotiation currentMtu (e.g.
        // 498 from a previous session) to size fragments leads to oversized notifies that the
        // BLE stack silently drops. Always cap below this value.
        private const val SAFE_MAX_FRAGMENT_PAYLOAD = 237
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
     * Derives a compact 4-byte peer ID (as 8-char hex) from a BLE MAC address.
     * Used by Subsystem 2 (deliveredTo tracking) and Subsystem 3 (cooldown override).
     * Stable across sessions for the same MAC address.
     */
    private fun compactPeerId(macAddress: String): String {
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        val hash = digest.digest(macAddress.toByteArray(Charsets.UTF_8))
        return hash.take(4).joinToString("") { "%02x".format(it) }
    }

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

    // Relevance system: tx currently loaded into the transport frame buffer for this connection
    private var activeTxId: String? = null
    
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

    // De-dupe set for received confirmations, keyed by full txId. Prevents the A↔B echo loop
    // where a confirmation bounces back and forth — without this every round trip would
    // re-record the same confirmation on the Dev screen and re-relay it until the Rust SDK's
    // hop cap (MAX_TX_RELAY_HOPS = 5) finally rejected it. Bounded LRU; oldest evicted on overflow.
    private val seenConfirmationTxIds = object : LinkedHashSet<String>() {
        private val max = 200
        override fun add(element: String): Boolean {
            val added = super.add(element)
            if (added && size > max) {
                val it = iterator()
                if (it.hasNext()) { it.next(); it.remove() }
            }
            return added
        }
    }

    // Reassembly buffer for fragmented confirmations (frame type 0x0C). Confirmations sometimes
    // exceed the ATT notify cap (MTU-3) — particularly Failed confirmations with long error
    // messages. We chunk them across multiple packets and rebuild here on the receiver.
    private val confFragBuffer = mutableListOf<ByteArray>()
    @Volatile private var confFragTotalChunks: Int = 0
    @Volatile private var confFragTotalBytes: Int = 0
    @Volatile private var confFragLastUpdateMs: Long = 0L
    private val CONF_FRAG_HEADER_SIZE = 5  // 0x0C, total_chunks, index, total_bytes_lo, total_bytes_hi
    private val CONF_FRAG_TIMEOUT_MS = 5_000L
    
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
    // Base interval; actual value comes from Rust adaptive params each cycle.
    private val ALTERNATING_INTERVAL_MS = 8_000L
    // Adaptive params updated every 10s by the auto-save/maintenance job.
    @Volatile private var adaptiveSessionTargetMs = 60_000L
    @Volatile private var adaptiveCooldownMs      = 45_000L
    // Time this device entered IDLE (not connected) — for sparse-network override.
    @Volatile private var idleStartMs = 0L

    // Fix: Idle-disconnect window — once our outbound queue empties, keep the connection open
    // for this long so the remote peer has a chance to push data back to us.
    @Volatile private var lastInboundDataMs = 0L
    @Volatile private var queueEmptySinceMs = 0L   // timestamp when our queue first went empty
    private val IDLE_DISCONNECT_WINDOW_MS = 4_000L // 4 s of silence on both sides → disconnect

    // Fix: Stale-connection fallback — if the OS doesn't deliver STATE_DISCONNECTED after we
    // call cancelConnection(), force-reset connection state ourselves after this delay.
    private val FORCE_DISCONNECT_FALLBACK_MS = 1_500L

    // Fix: Peer cooldown — after disconnecting from a device, suppress reconnection for this
    // long so the alternating loop has a chance to find a different peer.
    private val recentlyConnectedPeers = LinkedHashMap<String, Long>() // address → disconnect timestamp
    private val PEER_COOLDOWN_MS = 45_000L // 45 seconds

    // Connection rotation (fairness): when ≥ rotationPeerThreshold peers are visible recently,
    // force-tear-down the active connection after rotationMaxSessionMs so we don't camp on one
    // peer while a third (or more) peer starves. Cooldown + alternating mesh handles re-pairing.
    private val ROTATION_PEER_FRESHNESS_MS = 60_000L
    private val ROTATION_TICK_MS = 1_000L
    @Volatile private var rotationPeerThreshold: Int = 3
    @Volatile private var rotationMaxSessionMs: Long = 30_000L
    @Volatile private var sessionStartMs: Long = 0L
    private var rotationJob: Job? = null

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

    // RPC URL stored at init time for direct Solana submissions
    private var solanaRpcUrl: String = "https://api.devnet.solana.com"
    
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

    /** Per-device connection count. Key = MAC address, value = total times connected. */
    private val _connectionCounts = MutableStateFlow<Map<String, Int>>(emptyMap())
    val connectionCounts: StateFlow<Map<String, Int>> = _connectionCounts

    private val _confirmationEvents = MutableSharedFlow<ConfirmationEvent>(extraBufferCapacity = 8)
    val confirmationEvents: SharedFlow<ConfirmationEvent> = _confirmationEvents

    private val _rotationEvents = MutableSharedFlow<RotationEvent>(extraBufferCapacity = 8)
    val rotationEvents: SharedFlow<RotationEvent> = _rotationEvents

    /** Rolling list (newest first, capped at MAX_TX_LOG_SIZE) of transactions received via BLE. */
    private val _receivedTransactions = MutableStateFlow<List<ReceivedTxRecord>>(emptyList())
    val receivedTransactions: StateFlow<List<ReceivedTxRecord>> = _receivedTransactions

    /** Rolling list (newest first, capped at MAX_TX_LOG_SIZE) of received confirmations. */
    private val _confirmationLog = MutableStateFlow<List<ConfirmationRecord>>(emptyList())
    val confirmationLog: StateFlow<List<ConfirmationRecord>> = _confirmationLog

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
        runningInstance = this

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
            val maxPayload = minOf((currentMtu - 10).coerceAtLeast(20), SAFE_MAX_FRAGMENT_PAYLOAD)
            appendLog("📏 Using MTU=$currentMtu, maxPayload=$maxPayload bytes per fragment (capped at $SAFE_MAX_FRAGMENT_PAYLOAD)")
            sdkInstance.fragment(payload, maxPayload).onSuccess { fragments ->
                val count = fragments.fragments.size
                val firstFragment = fragments.fragments.firstOrNull()
                val firstFragmentData = firstFragment?.data
                val firstFragmentType = firstFragment?.fragmentType
                appendLog("📤 Fragmenting sample tx into $count fragments")
                appendLog("   Fragment size calculation: ${byteSize} bytes ÷ $maxPayload = ~$count fragments")
                appendLog(" Fragment Data: ${firstFragmentData}…")
                appendLog(" Fragment Type: $firstFragmentType")

                if (firstFragment == null) {
                    appendLog("❌ Fragment list is empty")
                    return@onSuccess
                }

                val txId = try {
                    val checksumBytes = android.util.Base64.decode(firstFragment.checksum, android.util.Base64.DEFAULT)
                    if (checksumBytes.size != 32) {
                        appendLog("❌ Invalid checksum size: ${checksumBytes.size} (expected 32)")
                        return@onSuccess
                    }
                    checksumBytes.joinToString("") { "%02x".format(it) }
                } catch (e: Exception) {
                    appendLog("❌ Failed to decode checksum: ${e.message}")
                    return@onSuccess
                }

                val fragmentsFFI = fragments.fragments.map { frag ->
                    FragmentFFI(
                        transactionId = txId,
                        fragmentIndex = frag.index,
                        totalFragments = frag.total,
                        dataBase64 = frag.data
                    )
                }

                sdkInstance.pushOutboundTransaction(
                    txBytes = payload,
                    txId = txId,
                    fragments = fragmentsFFI,
                    priority = Priority.NORMAL
                ).onSuccess {
                    appendLog("✅ Sample tx queued ($count fragments) txId=${txId.take(16)}…")
                    workChannel.trySend(WorkEvent.OutboundReady)
                    pendingTransactionBytes = payload
                    fragmentsQueuedWithMtu = currentMtu
                    ensureSendingLoopStarted()
                }.onFailure { e ->
                    appendLog("❌ Failed to push sample tx to outbound queue: ${e.message}")
                }
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
                val maxPayload = minOf((currentMtu - 10).coerceAtLeast(20), SAFE_MAX_FRAGMENT_PAYLOAD)
                appendLog("📏 Using MTU=$currentMtu, maxPayload=$maxPayload bytes per fragment (capped at $SAFE_MAX_FRAGMENT_PAYLOAD)")
                sdkInstance.fragment(bytes, maxPayload).onSuccess { fragments ->
                    val count = fragments.fragments.size
                    val firstFragment = fragments.fragments.firstOrNull()
                    appendLog("📤 Fragmenting provided tx into $count fragments")
                    appendLog("   Fragment size calculation: ${bytes.size} bytes ÷ $maxPayload = ~$count fragments")

                    if (firstFragment == null) {
                        appendLog("❌ Fragment list is empty")
                        return@onSuccess
                    }

                    val txId = try {
                        val checksumBytes = android.util.Base64.decode(firstFragment.checksum, android.util.Base64.DEFAULT)
                        if (checksumBytes.size != 32) {
                            appendLog("❌ Invalid checksum size: ${checksumBytes.size} (expected 32)")
                            return@onSuccess
                        }
                        checksumBytes.joinToString("") { "%02x".format(it) }
                    } catch (e: Exception) {
                        appendLog("❌ Failed to decode checksum: ${e.message}")
                        return@onSuccess
                    }

                    val fragmentsFFI = fragments.fragments.map { frag ->
                        FragmentFFI(
                            transactionId = txId,
                            fragmentIndex = frag.index,
                            totalFragments = frag.total,
                            dataBase64 = frag.data
                        )
                    }

                    sdkInstance.pushOutboundTransaction(
                        txBytes = bytes,
                        txId = txId,
                        fragments = fragmentsFFI,
                        priority = Priority.NORMAL
                    ).onSuccess {
                        appendLog("✅ Tx queued ($count fragments) txId=${txId.take(16)}…")
                        workChannel.trySend(WorkEvent.OutboundReady)
                        pendingTransactionBytes = bytes
                        fragmentsQueuedWithMtu = currentMtu
                        ensureSendingLoopStarted()

                        // If not connected, ensure scanning/advertising is active to establish connection
                        if (_connectionState.value != ConnectionState.CONNECTED) {
                            appendLog("⚠️ Not connected - ensuring BLE discovery is active...")
                            if (!_isScanning.value && !_isAdvertising.value) {
                                appendLog("   Starting scan to find peers...")
                                startScanning()
                            } else if (_isAdvertising.value) {
                                appendLog("   Already advertising - waiting for peer to connect...")
                            } else {
                                appendLog("   Already scanning - waiting to find peer...")
                            }
                        }
                    }.onFailure { e ->
                        appendLog("❌ Failed to push tx to outbound queue: ${e.message}")
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
            val maxPayload = minOf((currentMtu - 10).coerceAtLeast(20), SAFE_MAX_FRAGMENT_PAYLOAD)
            appendLog("📏 Using MTU=$currentMtu, maxPayload=$maxPayload bytes per fragment (capped at $SAFE_MAX_FRAGMENT_PAYLOAD)")
            
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
                            appendLog("📤 Event: OutboundReady")
                            eventCount++
                            lastEventTime = System.currentTimeMillis()
                            // Pull the freshly-queued tx into the transport-level fragment queue
                            // immediately so the sending loop picks it up on its next 800ms tick
                            // (or sooner — the load resets the idle window).
                            if (_connectionState.value == ConnectionState.CONNECTED && activeTxId == null) {
                                sdk?.loadForSending()?.getOrNull()?.let { loaded ->
                                    activeTxId = loaded.txId
                                    queueEmptySinceMs = 0L
                                    appendLog("📡 Pre-loaded tx ${loaded.txId.take(8)}… on OutboundReady (relevance=${loaded.relevance}, fragments=${loaded.fragmentCount})")
                                }
                            }
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
        
        // Check internet connectivity — if offline, relay received transactions via BLE
        // instead of trying (and failing) to submit them to pollicore directly.
        if (!hasInternetConnection()) {
            appendLog("⚠️ processReceivedQueue: No internet — relaying received transactions via BLE mesh")
            var relayed = 0
            repeat(5) {
                val receivedTx = sdkInstance.nextReceivedTransaction().getOrNull() ?: return@repeat
                val txBytes = android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP)
                val hops = txRelayHops.getOrDefault(receivedTx.txId, 0)
                if (hops >= MAX_TX_RELAY_HOPS) {
                    appendLog("🗑️ TX ${receivedTx.txId.take(8)} hit relay TTL — dropping")
                    txRelayHops.remove(receivedTx.txId)
                    upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.FAILED, error = "Relay TTL exceeded", relayHop = hops)
                } else {
                    txRelayHops[receivedTx.txId] = hops + 1
                    if (txRelayHops.size > 200) txRelayHops.remove(txRelayHops.keys.first())
                    queueSignedTransaction(txBytes, Priority.LOW)
                        .onSuccess { frags ->
                            relayed++
                            appendLog("📤 Relayed TX ${receivedTx.txId.take(8)} — $frags fragment(s) queued (hop ${hops + 1}/$MAX_TX_RELAY_HOPS)")
                            upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.RELAYED, relayHop = hops + 1)
                        }
                        .onFailure { e ->
                            appendLog("⚠️ Relay failed for ${receivedTx.txId.take(8)}: ${e.message}")
                            upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.FAILED, error = "Relay queue failed: ${e.message}", relayHop = hops)
                            sdkInstance.pushReceivedTransaction(txBytes)
                        }
                }
            }
            if (relayed > 0) ensureSendingLoopStarted()
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
            upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.RECEIVED)

            try {
                val submitResult = submitReceivedPayload(receivedTx.transactionBase64)
                
                submitResult.onSuccess { signature ->
                    successCount++
                    val txProgress = txFragmentInfo?.let { "~${it.totalFragments}/${it.totalFragments}" } ?: "~complete"
                    appendLog("✅ ✅ ✅ Transaction submitted SUCCESSFULLY! ✅ ✅ ✅")
                    appendLog("   Transaction ID: ${receivedTx.txId} $txProgress")
                    android.util.Log.d("PolliNet.BLE", "   Transaction ID: ${receivedTx.txId} $txProgress")
                    appendLog("   Signature: $signature")
                    appendLog("   Transaction is now on-chain")
                    upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.SUBMITTED, signature = signature)
                    
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
                    upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.FAILED, error = errorMsg)
                    
                    // Calculate tx hash for failure confirmation (same logic as success path)
                    val txHash = try {
                        val txBytes = android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP)
                        val digest = java.security.MessageDigest.getInstance("SHA-256")
                        digest.update(txBytes)
                        digest.digest().joinToString("") { "%02x".format(it) }
                    } catch (hashEx: Exception) {
                        receivedTx.transactionBase64.take(64)
                    }

                    // Check if this is a stale or permanently invalid transaction error
                    if (isStaleTransactionError(errorMsg)) {
                        appendLog("   🗑️ Invalid transaction detected - dropping (won't retry)")
                        android.util.Log.w("PolliNet.BLE", "Dropping invalid transaction ${receivedTx.txId.take(8)}... due to: $errorMsg")
                        // Send failure confirmation so the origin node learns the tx was dropped
                        sdkInstance.queueFailureConfirmation(txHash, errorMsg)
                            .onSuccess {
                                appendLog("   📤 Queued failure confirmation for relay")
                                workChannel.trySend(WorkEvent.ConfirmationReady)
                            }
                            .onFailure { qe ->
                                appendLog("   ⚠️ Failed to queue failure confirmation: ${qe.message}")
                            }
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
                upsertReceivedTx(receivedTx.txId, ReceivedTxStatus.FAILED, error = e.message ?: "Exception during submission")
                // Best-effort: add to retry queue so the tx isn't silently lost
                try {
                    val txBytes = android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP)
                    sdkInstance.addToRetryQueue(
                        txBytes = txBytes,
                        txId = receivedTx.txId,
                        error = e.message ?: "Exception during submission"
                    )
                    appendLog("   ↩️ Added to retry queue after exception")
                } catch (retryEx: Exception) {
                    appendLog("   ❌ Could not add to retry queue: ${retryEx.message}")
                }
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

            val txBytes = android.util.Base64.decode(retryItem.txBytes, android.util.Base64.NO_WRAP)

            try {
                val submitResult = submitReceivedPayload(retryItem.txBytes)
                
                // Calculate tx hash once — used for both success and failure confirmations
                val txHash = try {
                    val digest = java.security.MessageDigest.getInstance("SHA-256")
                    digest.update(txBytes)
                    digest.digest().joinToString("") { "%02x".format(it) }
                } catch (hashEx: Exception) {
                    retryItem.txBytes.take(64)
                }

                submitResult.onSuccess { signature ->
                    appendLog("✅ Retry successful: $signature")
                    sdkInstance.markTransactionSubmitted(txBytes)
                    upsertReceivedTx(retryItem.txId, ReceivedTxStatus.SUBMITTED, signature = signature)

                    // Queue success confirmation
                    sdkInstance.queueConfirmation(txHash, signature)
                        .onSuccess { workChannel.trySend(WorkEvent.ConfirmationReady) }
                        .onFailure { e -> appendLog("⚠️ Failed to queue success confirmation: ${e.message}") }

                    processedCount++
                }.onFailure { error ->
                    val errorMsg = error.message ?: "Unknown error"
                    appendLog("⚠️ Retry failed (attempt ${retryItem.attemptCount}): $errorMsg")

                    // Check if permanently invalid (stale nonce, bad signature, etc.)
                    if (isStaleTransactionError(errorMsg)) {
                        appendLog("   🗑️ Invalid transaction - dropping permanently")
                        android.util.Log.w("PolliNet.BLE", "Dropping invalid tx ${retryItem.txId.take(8)}... after ${retryItem.attemptCount} attempts: $errorMsg")
                        sdkInstance.queueFailureConfirmation(txHash, errorMsg)
                            .onSuccess {
                                appendLog("   📤 Queued failure confirmation for relay")
                                workChannel.trySend(WorkEvent.ConfirmationReady)
                            }
                            .onFailure { qe -> appendLog("   ⚠️ Failed to queue failure confirmation: ${qe.message}") }
                    } else if (retryItem.attemptCount < 5) {
                        // Re-add to retry queue with incremented count
                        sdkInstance.addToRetryQueue(
                            txBytes = txBytes,
                            txId = retryItem.txId,
                            error = errorMsg
                        )
                    } else {
                        // Max retries exhausted — send failure confirmation
                        appendLog("❌ Max retries (5) exhausted for tx ${retryItem.txId.take(8)}... — sending failure confirmation")
                        sdkInstance.queueFailureConfirmation(txHash, "Max retries (5) exceeded: $errorMsg")
                            .onSuccess {
                                appendLog("   📤 Queued failure confirmation for relay")
                                workChannel.trySend(WorkEvent.ConfirmationReady)
                            }
                            .onFailure { qe -> appendLog("   ⚠️ Failed to queue failure confirmation: ${qe.message}") }
                    }
                }
            } catch (e: Exception) {
                appendLog("❌ Exception processing retry: ${e.message}")
                // Best-effort re-add to retry if we haven't exceeded attempts
                if (retryItem.attemptCount < 5) {
                    try {
                        sdkInstance.addToRetryQueue(
                            txBytes = txBytes,
                            txId = retryItem.txId,
                            error = e.message ?: "Exception during retry"
                        )
                        appendLog("   ↩️ Re-added to retry queue after exception")
                    } catch (retryEx: Exception) {
                        appendLog("   ❌ Could not re-add to retry queue: ${retryEx.message}")
                    }
                }
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
            
                // Single-packet threshold = ATT notify cap (MTU - 3). The previous check used
                // MTU - 10, which falsely rejected 244-byte confirmations on MTU 247. For
                // confirmations that genuinely exceed MTU - 3 (e.g. Failed with long error
                // messages), fall through to multi-packet fragmentation via the 0x0C frame type.
                val singlePacketCap = (currentMtu - 3).coerceAtLeast(20)
                if (jsonBytes.size <= singlePacketCap) {
                    sendConfirmationToGatt(jsonBytes)
                    processedCount++
                } else {
                    appendLog("📦 Confirmation ${jsonBytes.size}B exceeds single-packet cap ${singlePacketCap}B — fragmenting")
                    val ok = sendFragmentedConfirmation(jsonBytes)
                    if (ok) processedCount++
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

            // Echo-loop guard: confirmations bounce back and forth between paired devices in
            // small meshes. Without this, every round trip would re-record this confirmation
            // on the Dev screen and re-relay it until the Rust SDK's hop cap finally rejects.
            // First receive wins; subsequent receives of the same txId are silently dropped.
            if (!seenConfirmationTxIds.add(confirmation.txId)) {
                appendLog("⏭️ Confirmation for ${confirmation.txId.take(8)}… already processed — skipping (echo)")
                return
            }

            val txShort = confirmation.txId.take(8)
            when (confirmation.status) {
                is ConfirmationStatus.Success -> {
                    val sig = (confirmation.status as ConfirmationStatus.Success).signature
                    appendLog("   ✅ SUCCESS: ${sig.take(16)}...")
                    appendLog("   📝 Transaction $txShort... was successfully submitted!")
                    _confirmationEvents.tryEmit(ConfirmationEvent.Success(txShort, sig.take(16)))
                    recordConfirmation(txShort, success = true, detail = sig.take(16))
                    postConfirmationNotification("Transaction Confirmed ✓", "Tx $txShort… confirmed on Solana")
                }
                is ConfirmationStatus.Failed -> {
                    val err = (confirmation.status as ConfirmationStatus.Failed).error
                    appendLog("   ❌ FAILED: $err")
                    appendLog("   📝 Transaction $txShort... submission failed")
                    _confirmationEvents.tryEmit(ConfirmationEvent.Failure(txShort, err))
                    recordConfirmation(txShort, success = false, detail = err)
                    postConfirmationNotification("Transaction Failed", "Tx $txShort… failed: $err")
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
     * Send a confirmation that exceeds the single-packet cap by chunking it across multiple
     * notifies/writes. Wire format per chunk:
     *   byte 0:    0x0C  (frame type CONFIRMATION_FRAG)
     *   byte 1:    total chunks (u8, 1..255)
     *   byte 2:    chunk index (u8, 0-based)
     *   bytes 3-4: total payload length (u16 LE)
     *   bytes 5+:  this chunk's slice of the JSON payload
     *
     * Returns true if all chunks were dispatched. Receiver reassembles in handleConfirmationFragment.
     */
    private suspend fun sendFragmentedConfirmation(jsonBytes: ByteArray): Boolean {
        val maxChunkPayload = ((currentMtu - 3) - CONF_FRAG_HEADER_SIZE).coerceAtLeast(20)
        val totalChunks = (jsonBytes.size + maxChunkPayload - 1) / maxChunkPayload

        if (totalChunks > 255) {
            appendLog("❌ Confirmation ${jsonBytes.size}B too large to fragment (would need $totalChunks chunks, max 255)")
            return false
        }
        if (jsonBytes.size > 0xFFFF) {
            appendLog("❌ Confirmation ${jsonBytes.size}B exceeds u16 length limit (65535)")
            return false
        }

        appendLog("📦 Sending confirmation in $totalChunks chunks (chunkPayload=${maxChunkPayload}B, total=${jsonBytes.size}B)")
        for (i in 0 until totalChunks) {
            val start = i * maxChunkPayload
            val end = minOf(start + maxChunkPayload, jsonBytes.size)
            val chunkData = jsonBytes.copyOfRange(start, end)
            val frame = ByteArray(CONF_FRAG_HEADER_SIZE + chunkData.size)
            frame[0] = 0x0C.toByte()
            frame[1] = totalChunks.toByte()
            frame[2] = i.toByte()
            frame[3] = (jsonBytes.size and 0xFF).toByte()
            frame[4] = ((jsonBytes.size shr 8) and 0xFF).toByte()
            chunkData.copyInto(frame, CONF_FRAG_HEADER_SIZE)
            sendConfirmationToGatt(frame)
            // Small spacing between chunks so the BLE stack doesn't drop notifies under burst
            delay(50)
        }
        return true
    }

    /**
     * Reassemble a multi-packet confirmation. Each chunk carries the total chunk count and the
     * total payload length, so we can detect a new confirmation (chunk index 0) and complete one
     * (buffer length == declared total). Stale buffers older than CONF_FRAG_TIMEOUT_MS are reset
     * on the next chunk arrival to recover from lost fragments.
     */
    private suspend fun handleConfirmationFragment(data: ByteArray) {
        if (data.size < CONF_FRAG_HEADER_SIZE + 1) {
            appendLog("⚠️ Confirmation fragment too short (${data.size}B) — dropping")
            return
        }
        val totalChunks = data[1].toInt() and 0xFF
        val chunkIndex = data[2].toInt() and 0xFF
        val totalBytes = (data[3].toInt() and 0xFF) or ((data[4].toInt() and 0xFF) shl 8)
        val chunk = data.copyOfRange(CONF_FRAG_HEADER_SIZE, data.size)
        val now = System.currentTimeMillis()

        appendLog("📨 Confirmation fragment ${chunkIndex + 1}/$totalChunks (${chunk.size}B, total=${totalBytes}B)")

        // Reset stale buffer (timeout) or on a fresh sequence (chunk 0 with different totals)
        val stale = confFragLastUpdateMs > 0 && (now - confFragLastUpdateMs) > CONF_FRAG_TIMEOUT_MS
        val newSequence = chunkIndex == 0 || confFragTotalChunks != totalChunks || confFragTotalBytes != totalBytes
        if (stale || newSequence) {
            if (stale && confFragBuffer.isNotEmpty()) {
                appendLog("⚠️ Discarding stale confirmation fragments (${confFragBuffer.size} chunks, ${(now - confFragLastUpdateMs)}ms old)")
            }
            confFragBuffer.clear()
            confFragTotalChunks = totalChunks
            confFragTotalBytes = totalBytes
        }

        confFragBuffer.add(chunk)
        confFragLastUpdateMs = now

        if (confFragBuffer.size < confFragTotalChunks) {
            return // wait for more
        }

        // All chunks received — concatenate and hand off to the existing JSON handler.
        val assembled = ByteArray(confFragTotalBytes)
        var pos = 0
        for (frag in confFragBuffer) {
            val copyLen = minOf(frag.size, assembled.size - pos)
            if (copyLen <= 0) break
            frag.copyInto(assembled, pos, 0, copyLen)
            pos += copyLen
        }
        confFragBuffer.clear()
        confFragTotalChunks = 0
        confFragTotalBytes = 0
        confFragLastUpdateMs = 0L

        if (pos != assembled.size) {
            appendLog("⚠️ Confirmation reassembly size mismatch (got ${pos}B, expected ${assembled.size}B) — dropping")
            return
        }

        appendLog("✅ Confirmation reassembled (${assembled.size}B) — handing off to JSON handler")
        handleReceivedConfirmation(assembled)
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
                                val submitResult = submitReceivedPayload(receivedTx.transactionBase64)

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
                                appendLog("⚠️ TX ${receivedTx.txId.take(8)} hit relay TTL ($hops/$MAX_TX_RELAY_HOPS) — dropping")
                                txRelayHops.remove(receivedTx.txId)
                            } else {
                                txRelayHops[receivedTx.txId] = hops + 1
                                appendLog("📡 No internet — relaying TX ${receivedTx.txId.take(8)} via BLE (hop ${hops + 1}/$MAX_TX_RELAY_HOPS)")

                                if (txRelayHops.size > 200) {
                                    txRelayHops.remove(txRelayHops.keys.first())
                                }

                                // Fragment + push into the outbound queue at LOW priority so the
                                // sending loop can deliver it to the next BLE peer in the mesh.
                                queueSignedTransaction(txBytes, Priority.LOW)
                                    .onSuccess { frags ->
                                        appendLog("📤 Queued $frags fragment(s) in outbound queue for BLE relay")
                                        ensureSendingLoopStarted()
                                    }
                                    .onFailure { e ->
                                        appendLog("⚠️ Failed to queue for relay: ${e.message}")
                                        // Put it back so it gets another chance next cycle
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
    /**
     * Intent-envelope wire format used by SendViewModel.transferViaBle: a JSON object containing
     * the user-signed intent (NOT a Solana VersionedTransaction). Pollicore constructs the real
     * transaction with the current blockhash and submits to Solana on behalf of the originator.
     */
    @kotlinx.serialization.Serializable
    private data class IntentEnvelope(
        @kotlinx.serialization.SerialName("intent_bytes") val intentBytes: String,
        val signature: String,
        @kotlinx.serialization.SerialName("from_token_account") val fromTokenAccount: String,
        @kotlinx.serialization.SerialName("token_program") val tokenProgram: String = "spl-token"
    )

    /**
     * Quick sniff of the first bytes to decide whether a received payload is an IntentEnvelope
     * JSON or a raw signed Solana VersionedTransaction. Solana txs start with a u8 signature count
     * (typically 0x01) followed by 64 bytes of signature, never with `{` (0x7B). Returning null
     * means "treat as raw Solana tx".
     */
    private fun parseIntentEnvelope(rawBytes: ByteArray): IntentEnvelope? {
        if (rawBytes.isEmpty() || rawBytes[0] != '{'.code.toByte()) return null
        return try {
            val str = rawBytes.toString(Charsets.UTF_8)
            if (!str.contains("intent_bytes")) return null
            json.decodeFromString<IntentEnvelope>(str)
        } catch (e: Exception) {
            appendLog("⚠️ Payload looked like JSON but failed envelope parse: ${e.message}")
            null
        }
    }

    /**
     * Single entry point for submitting a received payload. Routes to pollicore via submitIntent
     * for IntentEnvelope JSON, or to Solana RPC for raw signed VersionedTransactions. This is the
     * fix for "failed to deserialize VersionedTransaction" — the receiver was blindly handing JSON
     * intent envelopes to sendTransaction, which expects a serialized Solana transaction.
     */
    private suspend fun submitReceivedPayload(transactionBase64: String): Result<String> {
        return try {
            val rawBytes = android.util.Base64.decode(transactionBase64, android.util.Base64.NO_WRAP)
            val envelope = parseIntentEnvelope(rawBytes)
            if (envelope != null) {
                appendLog("📨 Detected intent envelope — submitting to pollicore (fromTokenAccount=${envelope.fromTokenAccount})")
                val sdkInstance = sdk
                    ?: return Result.failure(Exception("SDK not initialized"))
                sdkInstance.submitIntent(
                    intentBytesBase64 = envelope.intentBytes,
                    signatureBase64 = envelope.signature,
                    fromTokenAccount = envelope.fromTokenAccount,
                    tokenProgram = envelope.tokenProgram,
                )
            } else {
                appendLog("🌐 Submitting raw transaction to Solana RPC...")
                submitToSolanaRpc(transactionBase64)
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /** Submit a base64-encoded Solana transaction to the configured RPC and return the signature. */
    private suspend fun submitToSolanaRpc(transactionBase64: String): Result<String> = withContext(Dispatchers.IO) {
        runCatching {
            val body = """{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["$transactionBase64",{"encoding":"base64","preflightCommitment":"processed"}]}"""
            val conn = java.net.URL(solanaRpcUrl).openConnection() as java.net.HttpURLConnection
            conn.requestMethod = "POST"
            conn.setRequestProperty("Content-Type", "application/json")
            conn.doOutput = true
            conn.connectTimeout = 10_000
            conn.readTimeout = 30_000
            conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            val response = conn.inputStream.bufferedReader().readText()
            // Parse "result":"<signature>" from JSON response
            val sigMatch = Regex(""""result"\s*:\s*"([A-Za-z0-9]+)"""").find(response)
            val errMatch = Regex(""""message"\s*:\s*"([^"]+)"""").find(response)
            if (sigMatch != null) {
                sigMatch.groupValues[1]
            } else {
                throw RuntimeException(errMatch?.groupValues?.get(1) ?: "RPC error: $response")
            }
        }
    }

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
            
            // Check frame type byte (first byte of every frame).
            // 0x08 = CONFIRMATION, 0x09 = TX_ABORT, 0x0A = DRAIN_READY, 0x0B = CLOSE_ACK
            // 0x0C = CONFIRMATION_FRAG (multi-packet JSON confirmation, new)
            // Legacy: '{' (0x7B) = JSON confirmation (old path, keep for backward compat)
            // 0x01–0x07 = DATA_FRAGMENT types (fall through to pushInbound)
            if (data.isNotEmpty()) {
                when (data[0].toInt() and 0xFF) {
                    0x08 -> {
                        // Subsystem 3: Pollicore signed confirmation
                        appendLog("Received CONFIRMATION frame (${data.size}B)")
                        sdk?.ingestConfirmation(data)?.onSuccess { result ->
                            appendLog("Confirmation ingested: purged=${result.purged} carrier=${result.addedToCarrier}")
                            if (result.purged) {
                                // This device originated the transaction — it has been confirmed.
                                _confirmationEvents.tryEmit(ConfirmationEvent.Success("", "Confirmed on Solana"))
                                recordConfirmation(txIdShort = "", success = true, detail = "Confirmed on Solana")
                                postConfirmationNotification("Transaction Confirmed ✓", "Your transaction was confirmed on Solana")
                            }
                        }?.onFailure { e ->
                            appendLog("Failed to ingest confirmation: ${e.message}")
                        }
                        return
                    }
                    0x09 -> {
                        // TX_ABORT: peer is about to send us a confirmation for this tx
                        appendLog("Received TX_ABORT frame (${data.size}B) — dropping reassembly buffer")
                        // The tx_id_hash is in bytes 1..16; purge local reassembly buffer
                        if (data.size >= 17) {
                            val txIdHashHex = data.drop(1).take(16).joinToString("") { "%02x".format(it) }
                            sdk?.ingestConfirmation(data)  // let Rust handle it
                        }
                        return
                    }
                    0x0A -> {
                        // DRAIN_READY: peer has sent everything it has for us
                        appendLog("Received DRAIN_READY — peer queue drained")
                        // Signal our side that mutual drain is possible once we also drain
                        // (handled in the sending loop via queueEmptySinceMs logic)
                        return
                    }
                    0x0B -> {
                        // CLOSE_ACK: peer acknowledges graceful close
                        appendLog("Received CLOSE_ACK — graceful close acknowledged")
                        return
                    }
                    0x0C -> {
                        // Multi-packet JSON confirmation — reassemble and dispatch when complete.
                        handleConfirmationFragment(data)
                        return
                    }
                    0x7B -> {
                        // Legacy JSON confirmation ('{' = 0x7B)
                        handleReceivedConfirmation(data)
                        return
                    }
                }
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
        rotationJob?.cancel() // Cancel rotation watchdog
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
        runningInstance = null
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
        val clientPeerAddr = clientGatt?.device?.address
        val serverPeerAddr = connectedDevice?.address

        clientGatt?.let { gatt ->
            appendLog("🔌 Disconnecting and closing GATT connection to ${gatt.device.address}")
            // Official Android sample shows: disconnect() -> close()
            // This ensures proper cleanup and prevents battery drain
            gatt.disconnect()
            gatt.close()
            clientGatt = null
        }
        // Server-mode tear-down: when this device is the GATT server (peer connected to us)
        // gatt.disconnect() above is a no-op because clientGatt is null. We must actively kick
        // the peer via the GATT server, otherwise the idle-disconnect path silently fails and
        // we stay glued to the same peer forever — starving any third device.
        connectedDevice?.let { device ->
            gattServer?.let { server ->
                try {
                    appendLog("🔌 (Server) cancelling connection to ${device.address}")
                    server.cancelConnection(device)
                } catch (e: Exception) {
                    appendLog("⚠️ (Server) cancelConnection threw: ${e.message}")
                }
            }
        }

        // Stale-connection fallback: Android's BluetoothGattServer.cancelConnection() and
        // BluetoothGatt.disconnect() are best-effort. On many devices the link can die at the
        // OS level (RF blip, peer unsubscribed, etc.) without the corresponding STATE_DISCONNECTED
        // callback ever being delivered to user-space. Symptom: notifies/writes return
        // GATT_SUCCESS locally but never reach the peer; _connectionState stays CONNECTED forever
        // and the sending loop keeps shouting into the void. If the platform doesn't fire the
        // callback within FORCE_DISCONNECT_FALLBACK_MS, we manually transition state ourselves.
        val staleAddr = clientPeerAddr ?: serverPeerAddr ?: return
        mainHandler.postDelayed({
            // Bail if the legitimate callback already fired and reset state, or if a fresh
            // connection to a different peer was established in the meantime.
            if (_connectionState.value != ConnectionState.CONNECTED) return@postDelayed
            if (connectedDevice?.address != staleAddr && clientGatt?.device?.address != staleAddr) {
                return@postDelayed
            }
            appendLog("⚠️ STATE_DISCONNECTED never fired for $staleAddr after ${FORCE_DISCONNECT_FALLBACK_MS}ms — forcing local state reset")
            forceDisconnectState(staleAddr)
        }, FORCE_DISCONNECT_FALLBACK_MS)
    }

    /**
     * Manually drive the cleanup the gattCallback / gattServerCallback STATE_DISCONNECTED branches
     * would normally do, plus cooldown insertion and alternating-mesh restart. Used as a fallback
     * when the OS doesn't deliver STATE_DISCONNECTED after closeGattConnection() (see comment on
     * the postDelayed block above).
     */
    @SuppressLint("MissingPermission")
    private fun forceDisconnectState(addr: String) {
        _connectionState.value = ConnectionState.DISCONNECTED

        _peers.value = _peers.value.toMutableMap().apply {
            get(addr)?.let {
                put(addr, it.copy(isConnected = false, lastSeenAt = System.currentTimeMillis()))
            }
        }

        if (pendingConnectionDevice?.address == addr) {
            pendingConnectionDevice = null
        }

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
        fragmentsQueuedWithMtu = 0
        activeTxId = null
        descriptorWriteComplete = false
        queueEmptySinceMs = 0L
        lastInboundDataMs = 0L

        stopRotationTimer()

        val effectiveCooldown = adaptiveCooldownMs.takeIf { it > 0 } ?: PEER_COOLDOWN_MS
        recentlyConnectedPeers[addr] = System.currentTimeMillis() + effectiveCooldown
        val now = System.currentTimeMillis()
        recentlyConnectedPeers.entries.removeIf { it.value <= now }
        serviceScope.launch { sdk?.addPeerToCooldown(addr, effectiveCooldown) }
        idleStartMs = System.currentTimeMillis()

        appendLog("🔄 Force-reset complete; restarting alternating mesh")
        val backoffMs = (2000L..5000L).random()
        mainHandler.postDelayed({
            if (_connectionState.value == ConnectionState.DISCONNECTED &&
                connectedDevice == null && clientGatt == null) {
                startAlternatingMeshMode()
                appendLog("✅ Alternating mode restarted after force-reset")
            }
        }, backoffMs)
    }

    /**
     * Tune the fairness rotation policy.
     *
     * @param peerThreshold Minimum unique peers seen recently before forced rotation kicks in.
     *                      Use 0 to disable rotation entirely.
     * @param maxSessionMs  Maximum time a single connection may run while at/above the threshold.
     */
    fun setRotationConfig(peerThreshold: Int, maxSessionMs: Long) {
        rotationPeerThreshold = peerThreshold.coerceAtLeast(0)
        rotationMaxSessionMs = maxSessionMs.coerceAtLeast(1_000L)
        appendLog("⚙️ Rotation config: threshold=$rotationPeerThreshold peers, maxSession=${rotationMaxSessionMs}ms")
    }

    /**
     * Count peers "around" for the rotation policy.
     *
     * We can't rely on a tight freshness window because Android stops scanning while a GATT
     * connection is active — peers other than the connected one will not have their lastSeenAt
     * refreshed during the session. Instead we union three signals that all imply a peer was
     * recently around: any entry in [_peers] (added on scan/connect, never pruned in-session),
     * any address in the cooldown table, and any address with a connection-count entry.
     */
    private fun visiblePeerCount(): Int {
        val seen = HashSet<String>()
        seen.addAll(_peers.value.keys)
        seen.addAll(recentlyConnectedPeers.keys)
        seen.addAll(_connectionCounts.value.keys)
        return seen.size
    }

    /**
     * Force-disconnect the active peer regardless of role (client or server) so the cooldown +
     * alternating mesh can rotate to the next device. closeGattConnection() now handles both
     * client and server tear-down.
     */
    @SuppressLint("MissingPermission")
    private fun forceRotateNow(reason: String) {
        val peerAddress = clientGatt?.device?.address ?: connectedDevice?.address
        val sessionMs = if (sessionStartMs > 0L) System.currentTimeMillis() - sessionStartMs else 0L
        val visible = visiblePeerCount()
        appendLog("🔁 Rotation: forcing disconnect ($reason) — peer=$peerAddress, sessionMs=$sessionMs, visible=$visible")

        if (peerAddress != null) {
            _rotationEvents.tryEmit(RotationEvent.Forced(peerAddress, sessionMs, visible))
        }

        closeGattConnection()
    }

    /**
     * Start the rotation watchdog. Called from STATE_CONNECTED on both client and server paths.
     * Idempotent — re-entering CONNECTED simply restarts the timer.
     */
    private fun startRotationTimer() {
        rotationJob?.cancel()
        sessionStartMs = System.currentTimeMillis()

        if (rotationPeerThreshold <= 0) {
            return // rotation disabled
        }

        rotationJob = serviceScope.launch {
            while (isActive) {
                delay(ROTATION_TICK_MS)

                if (_connectionState.value != ConnectionState.CONNECTED) {
                    break
                }

                val sessionMs = System.currentTimeMillis() - sessionStartMs
                if (sessionMs < rotationMaxSessionMs) continue

                val visible = visiblePeerCount()
                if (visible < rotationPeerThreshold) continue

                // Avoid killing a fragment in mid-flight — wait for the in-progress write to
                // settle before tearing down (existing watchdog clears this within 5s).
                if (operationInProgress.get()) continue

                forceRotateNow(
                    "≥$rotationPeerThreshold peers visible ($visible) and session ${sessionMs}ms ≥ ${rotationMaxSessionMs}ms"
                )
                break
            }
        }
    }

    /** Stop the rotation watchdog. Called from every STATE_DISCONNECTED path. */
    private fun stopRotationTimer() {
        rotationJob?.cancel()
        rotationJob = null
        sessionStartMs = 0L
    }

    /**
     * Upsert a received-tx record. Newest record is at index 0; entries with the same txId are
     * replaced rather than duplicated so the UI reflects the latest status per transaction.
     */
    private fun upsertReceivedTx(
        txId: String,
        status: ReceivedTxStatus,
        signature: String? = null,
        error: String? = null,
        relayHop: Int? = null
    ) {
        val record = ReceivedTxRecord(
            txId = txId,
            status = status,
            timestamp = System.currentTimeMillis(),
            signature = signature,
            error = error,
            relayHop = relayHop
        )
        val current = _receivedTransactions.value
        val withoutDup = current.filterNot { it.txId == txId }
        val next = (listOf(record) + withoutDup).take(MAX_TX_LOG_SIZE)
        _receivedTransactions.value = next
    }

    /** Append a confirmation entry to the rolling log shown on the Dev screen. */
    private fun recordConfirmation(txIdShort: String, success: Boolean, detail: String) {
        val record = ConfirmationRecord(
            txIdShort = txIdShort,
            success = success,
            detail = detail,
            timestamp = System.currentTimeMillis()
        )
        val next = (listOf(record) + _confirmationLog.value).take(MAX_TX_LOG_SIZE)
        _confirmationLog.value = next
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

        val initResult = PolliNetSDK.initialize(config)
        val newSdk = initResult.getOrNull() ?: run {
            val error = initResult.exceptionOrNull() ?: Exception("Unknown SDK init error")
            appendLog("❌ SDK initialization failed: ${error.message}")
            appendLog("   SDK will remain null - operations requiring SDK will be skipped")
            return Result.failure(error)
        }

        sdk = newSdk
        config.rpcUrl?.let { url -> solanaRpcUrl = url }
        SdkHolder.set(newSdk)
        appendLog("✅ SDK initialized successfully")

        // CRITICAL: wipe persisted queues BEFORE the worker / sending loop comes up if the app
        // versionCode has changed since last init. Stale fragments queued at a different MTU,
        // or with an old fragmentation scheme, will silently brick transfers — see the 482-byte
        // notify drop bug. This is a one-shot per upgrade; the prefs key tracks last-seen version.
        wipeQueuesIfVersionChanged(newSdk)

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

        return Result.success(Unit)
    }

    /**
     * One-shot wipe of persisted SDK queues when the app's versionCode changes (fresh install,
     * upgrade, or downgrade). Prevents fragments persisted by an older SDK build — sized for an
     * older MTU or fragmentation scheme — from poisoning transfers after an upgrade.
     */
    private suspend fun wipeQueuesIfVersionChanged(sdkInstance: PolliNetSDK) {
        try {
            val pkgInfo = packageManager.getPackageInfo(packageName, 0)
            val currentVersion = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                pkgInfo.longVersionCode
            } else {
                @Suppress("DEPRECATION")
                pkgInfo.versionCode.toLong()
            }
            val prefs = getSharedPreferences("pollinet_internal", Context.MODE_PRIVATE)
            val lastVersion = prefs.getLong("last_sdk_version", -1L)

            if (lastVersion == currentVersion) {
                return // no version change, nothing to wipe
            }

            if (lastVersion < 0) {
                appendLog("🆕 First SDK init for this install (v$currentVersion) — wiping any stale persisted queues")
            } else {
                appendLog("♻️ App version changed ($lastVersion → $currentVersion) — wiping persisted queues to prevent stale-fragment poisoning")
            }

            sdkInstance.clearAllQueues()
                .onSuccess {
                    appendLog("✅ Queues wiped (outbound, retry, confirmation, received, reassembly buffers)")
                }
                .onFailure { e ->
                    appendLog("⚠️ Queue wipe failed: ${e.message}")
                }

            // Persist regardless of clear result — we don't want to retry forever if Rust SDK
            // genuinely refuses; the next push will at least produce fresh, properly-sized fragments.
            prefs.edit().putLong("last_sdk_version", currentVersion).apply()
        } catch (e: Exception) {
            appendLog("⚠️ Version-change wipe check failed: ${e.message}")
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

        appendLog("🚀 Starting alternating mesh mode (density-adaptive)")

        // Phase desync: random offset in [0, 8000) ms so not all devices flip simultaneously.
        val phaseOffsetMs = Random.nextLong(0L, 8_000L)
        val startWithScan = Random.nextBoolean()
        appendLog("   Phase offset: ${phaseOffsetMs}ms, starting with: ${if (startWithScan) "SCAN" else "ADVERTISE"}")
        idleStartMs = System.currentTimeMillis()

        alternatingMeshJob = serviceScope.launch {
            // Apply random phase desync before first cycle
            if (phaseOffsetMs > 0) delay(phaseOffsetMs)

            var scanMode = startWithScan

            while (isActive) {
                // Skip alternating if connected (transfer in progress)
                if (_connectionState.value == ConnectionState.CONNECTED) {
                    idleStartMs = 0L
                    delay(1000)
                    continue
                }

                // Update idle start timestamp when transitioning to idle
                if (idleStartMs == 0L) idleStartMs = System.currentTimeMillis()

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

                // Sparse-network override: if idle too long and all peers in cooldown, clear oldest
                val idleDuration = System.currentTimeMillis() - idleStartMs
                if (idleStartMs > 0 && idleDuration > 2 * adaptiveSessionTargetMs) {
                    serviceScope.launch { sdk?.expireOldestCooldown() }
                }

                // Per-cycle jitter: ±1000 ms
                val jitter = Random.nextLong(-1000L, 1001L)
                val cycleMs = (ALTERNATING_INTERVAL_MS + jitter).coerceAtLeast(3_000L)

                if (scanMode) {
                    appendLog("🔄 Mesh: → SCAN (${cycleMs / 1000}s, density=${adaptiveSessionTargetMs / 1000}s target)")
                    stopAdvertising()
                    delay(500)
                    startScanning()
                    delay(cycleMs)
                } else {
                    appendLog("🔄 Mesh: → ADVERTISE (${cycleMs / 1000}s)")
                    stopScanning()
                    delay(500)
                    startAdvertising()
                    delay(cycleMs)
                }

                scanMode = !scanMode
            }
        }

        appendLog("✅ Alternating mesh mode started — density-adaptive rotation active")
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
                    delay(10_000) // Every 10 seconds

                    // Auto-save queues (debounced internally to 5s)
                    sdk?.autoSaveQueues()?.onFailure { error ->
                        appendLog("⚠️ Auto-save failed: ${error.message}")
                    }

                    // Subsystem 1: recompute adaptive params from density estimator
                    sdk?.getAdaptiveParams()?.onSuccess { params ->
                        adaptiveSessionTargetMs = params.sessionTargetMs
                        adaptiveCooldownMs      = params.cooldownMs
                        // Also update the Kotlin-layer cooldown to keep them in sync
                    }?.onFailure { e ->
                        appendLog("⚠️ getAdaptiveParams failed: ${e.message}")
                    }

                    // Subsystem 3: evict expired tombstones and cooldowns
                    sdk?.periodicMaintenance()

                } catch (e: Exception) {
                    appendLog("❌ Auto-save/maintenance job error: ${e.message}")
                    delay(30_000)
                }
            }
        }

        appendLog("✅ Auto-save / adaptive maintenance job started")
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
        
        // CRITICAL: Don't start sending until descriptor write completes — in BOTH directions.
        // - Client mode: we wrote the CCCD on the remote TX char, so we can RECEIVE notifies.
        // - Server mode: peer wrote the CCCD on our TX char, so our notifies will actually be
        //   delivered. Without this gate the first notifies fire before peer subscribes and
        //   are silently dropped → receiver missing fragment 0 → reassembly stuck forever.
        if (!descriptorWriteComplete) {
            appendLog("⚠️ Waiting for CCCD subscribe to complete before sending...")
            appendLog("   This ensures notifications will actually be delivered")
            return
        }
        
        // Purge relay transactions queued more than 5 minutes ago — they are stale and should
        // not be forwarded. Dropping them lets the idle-window fire quickly if there's nothing
        // fresh to send, which frees this connection slot for Device C sooner.
        serviceScope.launch {
            val removed = sdk?.purgeStaleOutbound(maxAgeSecs = 300L)?.getOrNull() ?: 0
            if (removed > 0) appendLog("🗑️ Purged $removed stale outbound transaction(s) before sending")
        }

        appendLog("🚀 Starting sending loop")
        sendingJob = serviceScope.launch {
            // Load the highest-relevance transaction BEFORE the loop starts so that
            // sendNextOutbound()'s first call always finds frames in the transport buffer.
            // Previously this ran in a parallel coroutine, causing a race where the sending
            // loop fired before loadForSending() completed and incorrectly triggered the idle
            // window → 4-second disconnect → transaction never transmitted.
            val result = sdk?.loadForSending()?.getOrNull()
            if (result != null) {
                activeTxId = result.txId
                appendLog("📡 Loaded tx ${result.txId.take(8)}… for sending (relevance=${result.relevance}, fragments=${result.fragmentCount})")
            } else {
                activeTxId = null
                appendLog("📭 Outbound queue empty — nothing to load for this peer")
            }

            while (_connectionState.value == ConnectionState.CONNECTED) {
                sendNextOutbound()
                delay(800)
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
            var data = sdkInstance.nextOutbound(maxLen = safeMaxLen)

            // The transport's low-level outbound_queue only refills via loadForSending(), which
            // pulls from queue_manager().outbound. loadForSending() is called once at connect;
            // if a tx is queued mid-session it never reaches the wire. Re-load defensively only
            // when there is NO active tx — otherwise we'd re-send fragments of the in-flight tx
            // forever, since relevance is only decremented on idle-disconnect.
            if (data == null && activeTxId == null) {
                val loaded = sdkInstance.loadForSending().getOrNull()
                if (loaded != null) {
                    activeTxId = loaded.txId
                    appendLog("📡 Loaded fresh tx ${loaded.txId.take(8)}… mid-session (relevance=${loaded.relevance}, fragments=${loaded.fragmentCount})")
                    data = sdkInstance.nextOutbound(maxLen = safeMaxLen)
                }
            }

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

                // Window expired — disconnect regardless of whether we had data to send.
                // Without this, two devices with empty queues stay connected forever and
                // starve a third device (C) that is waiting for a turn in the mesh.
                appendLog("📭 Idle window expired (${elapsed}ms) — disconnecting to rotate mesh")
                if (_connectionState.value == ConnectionState.CONNECTED) {
                    // Relevance system: confirm delivery for the active transaction.
                    // This decrements its relevance counter; only clear local state if it
                    // was evicted (relevance hit 0). If it still has deliveries remaining,
                    // keep pendingTransactionBytes so MTU re-fragmentation stays possible.
                    val txId = activeTxId
                    if (txId != null) {
                        // Mutual drain achieved — use confirmDeliveredByPeer (Subsystem 2).
                        // Compact peer ID = first 4 bytes of SHA-256(MAC address).
                        val peerAddr = connectedDevice?.address ?: clientGatt?.device?.address ?: ""
                        val peerIdHex = compactPeerId(peerAddr)
                        serviceScope.launch {
                            val removed = sdk?.confirmDeliveredByPeer(txId, peerIdHex)?.getOrNull() ?: true
                            if (removed) {
                                appendLog("TX ${txId.take(8)}… fan-out exhausted — evicted from queue")
                                pendingTransactionBytes = null
                                fragmentsQueuedWithMtu = 0
                            } else {
                                appendLog("TX ${txId.take(8)}… delivered to peer $peerIdHex — relevance decremented")
                            }
                            activeTxId = null
                        }
                    } else if (pendingTransactionBytes != null) {
                        pendingTransactionBytes = null
                        fragmentsQueuedWithMtu = 0
                    }
                    queueEmptySinceMs = 0L

                    appendLog("🔄 Dancing mesh: Idle timeout — disconnecting to find next peer...")
                    mainHandler.postDelayed({
                        if (_connectionState.value == ConnectionState.CONNECTED) {
                            appendLog("🔌 Dancing mesh: Disconnecting from current peer...")
                            closeGattConnection()
                        }
                    }, 200)
                } else {
                    queueEmptySinceMs = 0L
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

        // Last-line-of-defence size guard. ATT notify is capped at MTU-3, write at MTU-5.
        // If a fragment from the queue is somehow oversized for the active link (stale MTU
        // from a previous session, racing re-fragmentation, etc.), the BLE stack will silently
        // drop the notify and the peer will be stuck waiting for missing fragments forever.
        // We refuse to ship it; the next nextOutbound() call will pop the next fragment and
        // these stale oversized ones will eventually drain. The queue-time cap
        // (SAFE_MAX_FRAGMENT_PAYLOAD) prevents this from happening for fresh transactions.
        val maxNotifyPayload = (currentMtu - 3).coerceAtLeast(20)
        val maxWritePayload = (currentMtu - 5).coerceAtLeast(20)
        if (data.size > maxNotifyPayload && data.size > maxWritePayload) {
            appendLog("❌ Fragment ${data.size}B exceeds MTU $currentMtu (notify max $maxNotifyPayload, write max $maxWritePayload) — DROPPING to avoid silent loss on the wire")
            appendLog("   Cause: fragments queued at a higher MTU than the current link supports (stale queue).")
            // Release the in-progress flag so the loop continues; do NOT clear activeTxId or
            // we'll re-load the same tx (with the same oversized fragments) and loop forever.
            operationInProgress.set(false)
            return
        }
        
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

            // Record in peer map + Rust health monitor + density estimator
            val alreadyConnected = connectedDevice?.address == peerAddress ||
                    clientGatt?.device?.address == peerAddress
            recordPeer(peerAddress, result.rssi, connected = alreadyConnected)

            // Subsystem 1: feed density estimator (fire-and-forget)
            serviceScope.launch { sdk?.recordScanResult(peerAddress) }

            // Check if already connected to THIS device
            if (alreadyConnected) {
                appendLog("ℹ️ Already connected to this device, ignoring")
                return
            }

            // Peer cooldown — uses both in-memory Kotlin map AND Rust cooldown list
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

                    // Track per-device connection count
                    val addr0 = gatt.device.address
                    _connectionCounts.value = _connectionCounts.value.toMutableMap().apply {
                        put(addr0, (getOrDefault(addr0, 0) + 1))
                    }

                    // Clear pending connection on success
                    if (pendingConnectionDevice?.address == gatt.device.address) {
                        pendingConnectionDevice = null
                    }

                    // Drain any confirmations that queued while disconnected
                    workChannel.trySend(WorkEvent.ConfirmationReady)

                    // Start rotation watchdog — forces disconnect when ≥3 peers are visible
                    // and the session has run past rotationMaxSessionMs.
                    startRotationTimer()

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

                    // Stop rotation watchdog — session is over
                    stopRotationTimer()

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
                    // Don't clear pending transaction - relevance system keeps it for the next peer
                    fragmentsQueuedWithMtu = 0
                    activeTxId = null   // will be reloaded by loadForSending() on next connection

                    // Reset descriptor write flag
                    descriptorWriteComplete = false

                    // Reset idle-window tracking for the next connection
                    queueEmptySinceMs = 0L
                    lastInboundDataMs = 0L

                    // Subsystem 1: use adaptive cooldown from density estimator
                    val effectiveCooldown = adaptiveCooldownMs.takeIf { it > 0 } ?: PEER_COOLDOWN_MS
                    recentlyConnectedPeers[addr] = System.currentTimeMillis() + effectiveCooldown
                    val now = System.currentTimeMillis()
                    recentlyConnectedPeers.entries.removeIf { it.value <= now }
                    serviceScope.launch { sdk?.addPeerToCooldown(addr, effectiveCooldown) }
                    idleStartMs = System.currentTimeMillis()

                    // Dancing mesh: Automatically restart alternating mode to find next peer.
                    appendLog("🔄 Dancing mesh: Restarting alternating mode...")
                    val backoffMs = (2000L..5000L).random()
                    mainHandler.postDelayed({
                        if (_connectionState.value == ConnectionState.DISCONNECTED &&
                            connectedDevice == null && clientGatt == null) {
                            startAlternatingMeshMode()
                            appendLog("✅ Dancing mesh: Alternating mode restarted")
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
            
            // Re-fragment when EITHER (a) the new MTU is below what we queued fragments at —
            // they would otherwise be dropped silently by the BLE stack, OR (b) the MTU is
            // significantly larger than the queue-time MTU (fewer fragments = less overhead).
            // Skipping re-fragmentation on a decrease is the bug that left receivers stuck
            // at "fragment 1/2" forever.
            val mtuDelta = mtu - oldMtu
            val needReFragmentForShrink = pendingTransactionBytes != null &&
                fragmentsQueuedWithMtu > 0 &&
                mtu < fragmentsQueuedWithMtu
            val needReFragmentForGrowth = pendingTransactionBytes != null && mtuDelta >= 30
            if (needReFragmentForShrink || needReFragmentForGrowth) {
                val reason = if (needReFragmentForShrink)
                    "MTU shrunk below queue-time MTU ($fragmentsQueuedWithMtu → $mtu) — fragments would be dropped"
                else
                    "MTU increased by $mtuDelta bytes — fewer fragments possible"
                appendLog("🔄 Re-fragmenting: $reason")

                sendingJob?.cancel()

                serviceScope.launch {
                    val txBytes = pendingTransactionBytes
                    if (txBytes != null) {
                        val sdkInstance = sdk
                        if (sdkInstance != null) {
                            val newMaxPayload = minOf((currentMtu - 10).coerceAtLeast(20), SAFE_MAX_FRAGMENT_PAYLOAD)
                            appendLog("♻️ Re-fragmenting ${txBytes.size} bytes with maxPayload=$newMaxPayload")
                            sdkInstance.fragment(txBytes, newMaxPayload).onSuccess { fragments ->
                                val newCount = fragments.fragments.size
                                val oldCount = (txBytes.size + oldMaxPayload - 1) / oldMaxPayload
                                appendLog("✅ Re-fragmented: $oldCount → $newCount fragments")
                                fragmentsQueuedWithMtu = currentMtu
                                ensureSendingLoopStarted()
                            }.onFailure {
                                appendLog("❌ Re-fragmentation failed: ${it.message}")
                                ensureSendingLoopStarted()
                            }
                        } else {
                            appendLog("⚠️ SDK not available for re-fragmentation")
                        }
                    }
                }
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

                    // Track per-device connection count
                    val addr1 = device.address
                    _connectionCounts.value = _connectionCounts.value.toMutableMap().apply {
                        put(addr1, (getOrDefault(addr1, 0) + 1))
                    }

                    // Clear pending connection on success
                    if (pendingConnectionDevice?.address == device.address) {
                        pendingConnectionDevice = null
                    }

                    // Drain any confirmations that queued while disconnected
                    workChannel.trySend(WorkEvent.ConfirmationReady)

                    // Stop scanning/advertising now that we're connected
                    stopScanning()
                    stopAdvertising()

                    // Start rotation watchdog — same fairness policy as client mode
                    startRotationTimer()

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
                    
                    // CRITICAL: In server mode we cannot send immediately. The peer (acting as
                    // GATT client) still needs ~500ms after STATE_CONNECTED to do service
                    // discovery and write our TX characteristic's CCCD descriptor — only after
                    // that does Android actually deliver our notifies. Notifies fired before
                    // that are silently dropped at the BLE stack, leaving the receiver missing
                    // the first fragment(s) and reassembly stuck forever.
                    //
                    // Wait for the CCCD subscribe (handled in onDescriptorWriteRequest below).
                    // If the peer never subscribes within 3s, fall back to sending anyway as a
                    // best-effort safety net (some peers don't honour the standard CCCD flow).
                    appendLog("   Server mode: waiting for peer CCCD subscribe before sending notifies")
                    val peerAddr = device.address
                    mainHandler.postDelayed({
                        if (_connectionState.value == ConnectionState.CONNECTED &&
                            connectedDevice?.address == peerAddr &&
                            !descriptorWriteComplete) {
                            appendLog("⚠️ Peer $peerAddr did not write CCCD within 3s — falling back to sending anyway")
                            descriptorWriteComplete = true
                            ensureSendingLoopStarted()
                        }
                    }, 3_000L)
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    _connectionState.value = ConnectionState.DISCONNECTED
                    connectedDevice = null
                    sendingJob?.cancel()

                    // Stop rotation watchdog — session is over
                    stopRotationTimer()

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
                    activeTxId = null   // will be reloaded by loadForSending() on next connection

                    // Reset descriptor write flag
                    descriptorWriteComplete = false

                    // Reset idle-window tracking for the next connection
                    queueEmptySinceMs = 0L
                    lastInboundDataMs = 0L

                    // Subsystem 1: adaptive cooldown
                    val effectiveCooldown2 = adaptiveCooldownMs.takeIf { it > 0 } ?: PEER_COOLDOWN_MS
                    recentlyConnectedPeers[device.address] = System.currentTimeMillis() + effectiveCooldown2
                    val now = System.currentTimeMillis()
                    recentlyConnectedPeers.entries.removeIf { it.value <= now }
                    serviceScope.launch { sdk?.addPeerToCooldown(device.address, effectiveCooldown2) }
                    idleStartMs = System.currentTimeMillis()

                    // Dancing mesh: restart alternating mode
                    appendLog("🔄 Dancing mesh: Restarting alternating mode...")
                    val backoffMs = (2000L..5000L).random()
                    mainHandler.postDelayed({
                        if (_connectionState.value == ConnectionState.DISCONNECTED &&
                            connectedDevice == null && clientGatt == null) {
                            startAlternatingMeshMode()
                            appendLog("✅ Dancing mesh: Alternating mode restarted")
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
                val enabled = value.contentEquals(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)
                appendLog("   ✅ CCCD descriptor write - notifications ${if (enabled) "ENABLED" else "DISABLED"}")

                // The peer (GATT client) has now subscribed to our TX characteristic. Notifies
                // fired before this point would have been silently dropped by the BLE stack.
                // Now it is safe to start the sending loop. This is the missing-fragment-0 fix.
                if (enabled && _connectionState.value == ConnectionState.CONNECTED &&
                    connectedDevice?.address == device.address && !descriptorWriteComplete) {
                    descriptorWriteComplete = true
                    appendLog("   🚀 Peer subscribed to TX — starting sending loop now")
                    ensureSendingLoopStarted()
                }
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

            // Same re-fragmentation rule as the client-side handler: if the new MTU is below
            // what fragments were sized for, they will be silently dropped at the BLE stack
            // (notify is capped at MTU-3). Re-fragment so notifies actually reach the peer.
            val needReFragmentForShrink = pendingTransactionBytes != null &&
                fragmentsQueuedWithMtu > 0 &&
                mtu < fragmentsQueuedWithMtu
            if (needReFragmentForShrink) {
                appendLog("🔄 (Server) Re-fragmenting: MTU shrunk below queue-time MTU ($fragmentsQueuedWithMtu → $mtu)")
                sendingJob?.cancel()
                serviceScope.launch {
                    val txBytes = pendingTransactionBytes
                    val sdkInstance = sdk
                    if (txBytes != null && sdkInstance != null) {
                        val newMaxPayload = minOf((currentMtu - 10).coerceAtLeast(20), SAFE_MAX_FRAGMENT_PAYLOAD)
                        appendLog("♻️ (Server) Re-fragmenting ${txBytes.size} bytes with maxPayload=$newMaxPayload")
                        sdkInstance.fragment(txBytes, newMaxPayload).onSuccess { fragments ->
                            appendLog("✅ (Server) Re-fragmented into ${fragments.fragments.size} fragments")
                            fragmentsQueuedWithMtu = currentMtu
                            ensureSendingLoopStarted()
                        }.onFailure {
                            appendLog("❌ (Server) Re-fragmentation failed: ${it.message}")
                            ensureSendingLoopStarted()
                        }
                    }
                }
            }
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

    private fun postConfirmationNotification(title: String, text: String) {
        val nm = getSystemService(NotificationManager::class.java) ?: return
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(title)
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()
        nm.notify(CONFIRMATION_NOTIFICATION_ID, notification)
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