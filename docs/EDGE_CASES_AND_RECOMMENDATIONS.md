# PolliNet BLE Service - Edge Cases & Recommendations

## Table of Contents
1. [Critical Edge Cases](#-critical-edge-cases)
2. [Race Conditions & Concurrency Issues](#️-race-conditions--concurrency-issues)
3. [Battery & Power Management](#-battery--power-management)
4. [Network & RPC Edge Cases](#-network--rpc-edge-cases)
5. [Connection Edge Cases](#-connection-edge-cases)
6. [Data Integrity](#-data-integrity)
7. [Memory Leaks](#-memory-leaks)
8. [State Management](#-state-management)
9. [Security](#-security)
10. [Testing & Observability](#-testing--observability)
11. [Priority Recommendations](#-priority-recommendations)

---

## 🚨 **Critical Edge Cases**

### 1. **Bluetooth State Changes**

**Status**: ✅ COMPLETED (Dec 30, 2025)

**Location**: `BleService.kt` - No BroadcastReceiver for `BluetoothAdapter.ACTION_STATE_CHANGED`

**Problem**: If user turns off Bluetooth mid-operation, the service doesn't detect it

**Impact**: Keeps trying to scan/advertise on disabled Bluetooth, wastes battery

**Implementation**: Added bluetoothStateReceiver with full state management (ON/OFF/TURNING_ON/TURNING_OFF)

**Fix Needed**:
```kotlin
private val bluetoothStateReceiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        when (intent?.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
            BluetoothAdapter.STATE_OFF -> {
                appendLog("📴 Bluetooth disabled - pausing all operations")
                stopScanning()
                stopAdvertising()
                closeGattConnection()
                // Save state for resume
            }
            BluetoothAdapter.STATE_ON -> {
                appendLog("📶 Bluetooth enabled - resuming operations")
                // Resume operations based on saved state
                if (shouldBeAdvertising) startAdvertising()
            }
            BluetoothAdapter.STATE_TURNING_OFF -> {
                appendLog("⚠️ Bluetooth turning off...")
            }
            BluetoothAdapter.STATE_TURNING_ON -> {
                appendLog("⚠️ Bluetooth turning on...")
            }
        }
    }
}

// Register in onCreate
override fun onCreate() {
    super.onCreate()
    // ... existing code ...
    val btFilter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
    registerReceiver(bluetoothStateReceiver, btFilter)
}

// Unregister in onDestroy
override fun onDestroy() {
    try {
        unregisterReceiver(bluetoothStateReceiver)
    } catch (e: IllegalArgumentException) {
        // Not registered
    }
    // ... rest of cleanup ...
}
```

---

### 2. **Permission Revocation at Runtime**

**Status**: ❌ MISSING

**Location**: Throughout `BleService.kt` - No runtime permission check during operations

**Problem**: If user revokes permissions while service is running (Android Settings → Apps → Permissions)

**Impact**: `SecurityException` crash

**Fix Needed**:
```kotlin
private fun hasRequiredPermissionsNow(): Boolean {
    return hasRequiredPermissions()
}

@SuppressLint("MissingPermission")
fun startScanning() {
    // Add permission check at operation time
    if (!hasRequiredPermissionsNow()) {
        appendLog("❌ Cannot scan: Permissions revoked")
        stopSelf() // Or handle gracefully
        return
    }
    
    // Check if Bluetooth is enabled
    if (bluetoothAdapter?.isEnabled != true) {
        // ... existing code ...
    }
    // ... rest of method ...
}

// Add similar checks to:
// - startAdvertising()
// - connectToDevice()
// - All GATT operations
```

---

### 3. **Queue Size Limits**

**Status**: ✅ COMPLETED (Dec 30, 2025)

**Location**: Line 92 - `private val operationQueue = ConcurrentLinkedQueue<ByteArray>()`

**Problem**: Unbounded queue can cause `OutOfMemoryError`

**Impact**: App crash when receiving flood of fragments

**Implementation**: Added MAX_OPERATION_QUEUE_SIZE (100 items) with FIFO overflow handling

**Fix Needed**:
```kotlin
companion object {
    private const val MAX_OPERATION_QUEUE_SIZE = 100
    private const val MAX_FRAGMENT_SIZE = 512 // bytes
}

@SuppressLint("MissingPermission")
private fun sendToGatt(data: ByteArray) {
    appendLog("📤 sendToGatt: Attempting to send ${data.size} bytes")
    
    // ... existing path selection logic ...
    
    if (gatt != null && remoteRx != null) {
        // Check queue size before adding
        if (operationInProgress) {
            if (operationQueue.size >= MAX_OPERATION_QUEUE_SIZE) {
                appendLog("⚠️ Operation queue full (${operationQueue.size}), dropping oldest")
                operationQueue.poll() // Drop oldest
            }
            operationQueue.offer(data)
            return
        }
        // ... rest of method ...
    }
}

// Also add to server path (line 1518-1522)
if (operationInProgress) {
    if (operationQueue.size >= MAX_OPERATION_QUEUE_SIZE) {
        appendLog("⚠️ Operation queue full (${operationQueue.size}), dropping oldest")
        operationQueue.poll()
    }
    operationQueue.offer(data)
    return
}
```

---

### 4. **Fragment Timeout/Stale Detection**

**Status**: ⚠️ INCOMPLETE

**Location**: Cleanup job exists but no per-fragment timeout

**Problem**: If first fragment arrives but remaining fragments never come, memory leak

**Impact**: Partial transactions accumulate forever

**Fix Needed**:
```kotlin
// In processCleanup() function (line 783-802)
private suspend fun processCleanup() {
    val sdkInstance = sdk ?: return
    
    appendLog("🧹 Running cleanup...")
    
    // Cleanup stale fragments (with timeout)
    val fragmentsCleaned = sdkInstance.cleanupStaleFragments(
        olderThanSeconds = 300 // 5 minutes
    ).getOrNull() ?: 0
    
    // ... rest of cleanup ...
}

// In Rust FFI, add timeout parameter to cleanupStaleFragments
// and track fragment arrival timestamps
```

---

### 5. **Transaction Size Validation**

**Status**: ✅ COMPLETED (Dec 30, 2025)

**Location**: Lines 274-305 `queueSampleTransaction()`, lines 307-344 `queueTransactionFromBase64()`

**Problem**: Can queue arbitrarily large transactions

**Impact**: `OutOfMemoryError`, BLE flooding

**Implementation**: Added MAX_TRANSACTION_SIZE (5KB) validation in all 3 queue methods

**Fix Needed**:
```kotlin
companion object {
    // Reasonable limit: ~10 fragments max at 512 bytes each
    private const val MAX_TRANSACTION_SIZE = 5120 // bytes (~5KB)
}

fun queueSampleTransaction(byteSize: Int = 1024) {
    val sdkInstance = sdk ?: run {
        appendLog("⚠️ SDK not initialized; cannot queue sample transaction")
        return
    }
    
    // Validate size
    if (byteSize > MAX_TRANSACTION_SIZE) {
        appendLog("❌ Transaction too large: $byteSize bytes (max: $MAX_TRANSACTION_SIZE)")
        return
    }
    
    if (byteSize <= 0) {
        appendLog("❌ Invalid transaction size: $byteSize bytes")
        return
    }

    serviceScope.launch {
        appendLog("🧪 Queueing sample transaction (${byteSize} bytes)")
        // ... rest of method ...
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
            
            // Validate size
            if (bytes.size > MAX_TRANSACTION_SIZE) {
                appendLog("❌ Transaction too large: ${bytes.size} bytes (max: $MAX_TRANSACTION_SIZE)")
                return@launch
            }
            
            appendLog("🧾 Queueing provided transaction (${bytes.size} bytes)")
            // ... rest of method ...
        } catch (e: IllegalArgumentException) {
            appendLog("❌ Invalid base64 input: ${e.message}")
        }
    }
}

suspend fun queueSignedTransaction(
    txBytes: ByteArray,
    priority: Priority = Priority.NORMAL
): Result<Int> = withContext(Dispatchers.Default) {
    val sdkInstance = sdk ?: run {
        appendLog("⚠️ SDK not initialized; cannot queue transaction")
        return@withContext Result.failure(Exception("SDK not initialized"))
    }
    
    // Validate size
    if (txBytes.size > MAX_TRANSACTION_SIZE) {
        appendLog("❌ Transaction too large: ${txBytes.size} bytes (max: $MAX_TRANSACTION_SIZE)")
        return@withContext Result.failure(Exception("Transaction too large: ${txBytes.size} bytes"))
    }

    try {
        appendLog("🧾 Queueing signed transaction (${txBytes.size} bytes, priority: $priority) [MWA]")
        // ... rest of method ...
    } catch (e: Exception) {
        appendLog("❌ Failed to queue signed transaction: ${e.message}")
        Result.failure(e)
    }
}
```

---

## ⚡️ **Race Conditions & Concurrency Issues**

### 6. **Descriptor Write vs Connection State**

**Status**: ⚠️ POTENTIAL RACE CONDITION

**Location**: Lines 2089-2113 - `onDescriptorWrite` callback

**Problem**: Sending loop might start between check and cancel

**Impact**: Concurrent operations, status 133 errors

**Fix Needed**:
```kotlin
// Add synchronization mutex
private val descriptorWriteMutex = Mutex()

override fun onDescriptorWrite(
    gatt: BluetoothGatt,
    descriptor: BluetoothGattDescriptor,
    status: Int
) {
    serviceScope.launch {
        descriptorWriteMutex.withLock {
            appendLog("📝 Descriptor write: status=$status, connection=${_connectionState.value}")
            
            // Ignore stale callbacks - check if connection is still active
            if (_connectionState.value != ConnectionState.CONNECTED) {
                appendLog("⚠️ Ignoring descriptor write callback - connection is ${_connectionState.value}")
                return@launch
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
            } else if (status == 133) {
                // Pause sending loop while we recover (critical fix)
                sendingJob?.cancel()
                appendLog("⚠️ Status 133 detected - pausing sending loop for recovery")
                // ... rest of retry logic ...
            }
            // ... rest of method ...
        }
    }
}
```

---

### 7. **Concurrent SDK Access**

**Status**: ⚠️ NEEDS VERIFICATION

**Location**: Multiple coroutines access `sdk` without synchronization

**Problem**: If Rust FFI isn't thread-safe, data corruption

**Impact**: Silent corruption or crashes in native code

**Fix Needed**:
```kotlin
// Option 1: Add Mutex if FFI is not thread-safe
private val sdkMutex = Mutex()

private suspend fun sendNextOutbound() {
    sdkMutex.withLock {
        sendingMutex.lock()
        try {
            // ... existing code ...
            val data = sdkInstance.nextOutbound(maxLen = safeMaxLen)
            // ... rest of method ...
        } finally {
            sendingMutex.unlock()
        }
    }
}

// Option 2: Document and verify Rust FFI is thread-safe
// Add comment:
// SAFETY: The Rust FFI layer is thread-safe and can handle concurrent calls
// This is verified by [link to Rust code showing thread-safety]
```

---

### 8. **operationInProgress Flag Without Mutex**

**Status**: ✅ COMPLETED (Dec 30, 2025)

**Location**: Lines 1474, 1518 - `operationInProgress` checked then set without synchronization

**Problem**: Two threads can both see false and proceed

**Impact**: Concurrent BLE operations → status 133 errors

**Implementation**: Converted to AtomicBoolean with compareAndSet for critical sections

**Fix Needed**:
```kotlin
// Replace boolean with AtomicBoolean
private val operationInProgress = AtomicBoolean(false)

@SuppressLint("MissingPermission")
private fun sendToGatt(data: ByteArray) {
    appendLog("📤 sendToGatt: Attempting to send ${data.size} bytes")
    
    val gatt = clientGatt
    val remoteRx = remoteRxCharacteristic
    
    if (gatt != null && remoteRx != null) {
        appendLog("   → Using CLIENT path (write to remote RX)")
        
        // Atomic check-and-set
        if (!operationInProgress.compareAndSet(false, true)) {
            appendLog("⚠️ Operation in progress, queuing fragment")
            operationQueue.offer(data)
            return
        }
        
        // ... rest of write logic ...
        
        // On failure:
        if (result != BluetoothGatt.GATT_SUCCESS) {
            appendLog("   ⚠️ Write result indicates failure: $result")
            operationInProgress.set(false)
        }
        return
    }
    
    // Server path
    if (server != null && txChar != null && device != null) {
        appendLog("   → Using SERVER path (notify) - no client connection")
        
        // Atomic check-and-set
        if (!operationInProgress.compareAndSet(false, true)) {
            appendLog("⚠️ Operation in progress, queuing fragment")
            operationQueue.offer(data)
            return
        }
        
        // ... rest of notify logic ...
    }
}

// Update all operationInProgress assignments:
// operationInProgress = false → operationInProgress.set(false)
// if (operationInProgress) → if (operationInProgress.get())
```

---

## 🔋 **Battery & Power Management**

### 9. **Doze Mode / App Standby**

**Status**: ❌ MISSING

**Location**: No handling for Doze mode

**Problem**: Android puts app to sleep after screen off for ~1 hour

**Impact**: Mesh relay stops working, transactions queue indefinitely

**Fix Needed**:
```kotlin
// Option 1: Request battery optimization exemption (controversial)
// In MainActivity or setup flow:
@RequiresApi(Build.VERSION_CODES.M)
private fun requestBatteryOptimizationExemption() {
    val intent = Intent()
    val packageName = packageName
    val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
    
    if (!pm.isIgnoringBatteryOptimizations(packageName)) {
        intent.action = Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS
        intent.data = Uri.parse("package:$packageName")
        startActivity(intent)
    }
}

// Option 2: Document expected behavior
// Add to README.md:
// ## Doze Mode Limitations
// - PolliNet mesh relay may be delayed during Doze mode
// - For critical relay scenarios, disable battery optimization for PolliNet
// - Transactions will queue and process when device wakes

// Option 3: Use WorkManager with setExpedited() for critical work
// Already implemented in RetryWorker and CleanupWorker
```

---

### 10. **Low Battery Behavior**

**Status**: ⚠️ METRICS LOGGED BUT NO ACTION

**Location**: Lines 807-828 - `logBatteryMetrics()`

**Problem**: Continues aggressive scanning even at 5% battery

**Impact**: Device dies faster, user frustration

**Fix Needed**:
```kotlin
private var lowBatteryMode = false

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
        
        // Low battery optimization
        if (batteryPct in 1..15 && !lowBatteryMode) {
            lowBatteryMode = true
            appendLog("🔋 LOW BATTERY MODE ACTIVATED - reducing scan frequency")
            
            // Stop scanning if active
            if (_isScanning.value) {
                stopScanning()
            }
            
            // Reduce event processing frequency
            // (Already handled by event-driven architecture)
        } else if (batteryPct > 20 && lowBatteryMode) {
            lowBatteryMode = false
            appendLog("🔋 Battery recovered - resuming normal operation")
        }
        
        // Reset counter every minute
        if (System.currentTimeMillis() - lastEventTime > 60_000) {
            wakeUpCount = 0
        }
    } catch (e: Exception) {
        // Ignore battery metrics errors
    }
}

// Also check battery before starting scan
@SuppressLint("MissingPermission")
fun startScanning() {
    if (lowBatteryMode) {
        appendLog("⚠️ Cannot scan in low battery mode (battery < 15%)")
        return
    }
    // ... rest of method ...
}
```

---

## 🌐 **Network & RPC Edge Cases**

### 11. **RPC Rate Limiting**

**Status**: ❌ MISSING

**Location**: Lines 922-926, 700-703 - `submitOfflineTransaction()` calls

**Problem**: RPC endpoint returns 429 Too Many Requests

**Impact**: Transaction marked as failed permanently

**Fix Needed**:
```kotlin
private suspend fun submitTransactionWithRetry(
    sdkInstance: PolliNetSDK,
    transactionBase64: String,
    maxRetries: Int = 3
): Result<String> {
    var retryCount = 0
    var lastError: Throwable? = null
    
    while (retryCount <= maxRetries) {
        try {
            val result = sdkInstance.submitOfflineTransaction(
                transactionBase64 = transactionBase64,
                verifyNonce = false
            )
            
            result.fold(
                onSuccess = { signature ->
                    return Result.success(signature)
                },
                onFailure = { error ->
                    val errorMsg = error.message ?: ""
                    
                    // Check for rate limiting
                    if (errorMsg.contains("429") || 
                        errorMsg.contains("Too Many Requests") ||
                        errorMsg.contains("rate limit", ignoreCase = true)) {
                        
                        retryCount++
                        if (retryCount <= maxRetries) {
                            val backoffMs = 1000L * (1 shl retryCount) // Exponential: 2s, 4s, 8s
                            appendLog("⚠️ Rate limited (429) - backing off ${backoffMs}ms (attempt $retryCount/$maxRetries)")
                            delay(backoffMs)
                            continue
                        }
                    }
                    
                    // Check for other retryable errors
                    if (errorMsg.contains("timeout", ignoreCase = true) ||
                        errorMsg.contains("connection", ignoreCase = true)) {
                        retryCount++
                        if (retryCount <= maxRetries) {
                            appendLog("⚠️ Transient error - retrying (attempt $retryCount/$maxRetries)")
                            delay(1000L * retryCount)
                            continue
                        }
                    }
                    
                    // Non-retryable error
                    lastError = error
                    break
                }
            )
        } catch (e: Exception) {
            lastError = e
            break
        }
    }
    
    return Result.failure(lastError ?: Exception("Max retries exceeded"))
}

// Use in processReceivedQueue
private suspend fun processReceivedQueue() {
    val sdkInstance = sdk ?: return
    
    if (!hasInternetConnection()) {
        return
    }
    
    var processedCount = 0
    val batchSize = 5
    
    repeat(batchSize) {
        val receivedTx = sdkInstance.nextReceivedTransaction().getOrNull() ?: return@repeat
        
        appendLog("📥 Processing received tx: ${receivedTx.txId}")
        
        try {
            // Use new retry function
            val submitResult = submitTransactionWithRetry(
                sdkInstance = sdkInstance,
                transactionBase64 = receivedTx.transactionBase64
            )
            
            submitResult.onSuccess { signature ->
                appendLog("✅ Auto-submitted: $signature")
                // ... rest of success handling ...
            }.onFailure { error ->
                appendLog("⚠️ Submission failed after retries: ${error.message}")
                // ... rest of failure handling ...
            }
        } catch (e: Exception) {
            appendLog("❌ Exception processing received tx: ${e.message}")
        }
    }
}
```

---

### 12. **Transaction Already Processed**

**Status**: ❌ MISSING

**Location**: Lines 934-938, 716-728 - Error handling in submission

**Problem**: Error "transaction already confirmed" treated as failure

**Impact**: Wastes retry attempts on already-successful transactions

**Fix Needed**:
```kotlin
private fun isTransactionAlreadyConfirmedError(errorMsg: String): Boolean {
    return errorMsg.contains("already confirmed", ignoreCase = true) ||
           errorMsg.contains("already processed", ignoreCase = true) ||
           errorMsg.contains("duplicate transaction", ignoreCase = true) ||
           errorMsg.contains("AlreadyProcessed", ignoreCase = true)
}

private suspend fun processReceivedQueue() {
    // ... existing code ...
    
    submitResult.onSuccess { signature ->
        appendLog("✅ Auto-submitted: $signature")
        sdkInstance.markTransactionSubmitted(
            android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP)
        )
        
        // Queue confirmation for relay (Phase 2)
        sdkInstance.queueConfirmation(receivedTx.txId, signature)
            .onSuccess {
                workChannel.trySend(WorkEvent.ConfirmationReady)
            }
        
        processedCount++
    }.onFailure { error ->
        val errorMsg = error.message ?: ""
        
        // Check if transaction already confirmed (treat as success!)
        if (isTransactionAlreadyConfirmedError(errorMsg)) {
            appendLog("✅ Transaction already confirmed - marking as success")
            sdkInstance.markTransactionSubmitted(
                android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP)
            )
            // Don't send confirmation since we don't have the signature
            processedCount++
        } else {
            appendLog("⚠️ Submission failed: $errorMsg")
            
            // Add to retry queue (Phase 2)
            sdkInstance.addToRetryQueue(
                txBytes = android.util.Base64.decode(receivedTx.transactionBase64, android.util.Base64.NO_WRAP),
                txId = receivedTx.txId,
                error = errorMsg
            )
        }
    }
}
```

---

### 13. **RPC Timeout**

**Status**: ❌ MISSING

**Location**: All `submitOfflineTransaction()` calls

**Problem**: Can hang indefinitely waiting for slow RPC

**Impact**: Blocks event worker, no other transactions processed

**Fix Needed**:
```kotlin
companion object {
    private const val RPC_TIMEOUT_MS = 30_000L // 30 seconds
}

private suspend fun submitTransactionWithTimeout(
    sdkInstance: PolliNetSDK,
    transactionBase64: String
): Result<String> {
    return try {
        withTimeout(RPC_TIMEOUT_MS) {
            sdkInstance.submitOfflineTransaction(
                transactionBase64 = transactionBase64,
                verifyNonce = false
            )
        }
    } catch (e: TimeoutCancellationException) {
        appendLog("⏱️ RPC timeout after ${RPC_TIMEOUT_MS}ms")
        Result.failure(Exception("RPC timeout after ${RPC_TIMEOUT_MS}ms"))
    } catch (e: Exception) {
        Result.failure(e)
    }
}

// Use in all submission paths
private suspend fun processReceivedQueue() {
    // ... existing code ...
    
    try {
        val submitResult = submitTransactionWithTimeout(
            sdkInstance = sdkInstance,
            transactionBase64 = receivedTx.transactionBase64
        )
        // ... rest of handling ...
    } catch (e: Exception) {
        appendLog("❌ Exception processing received tx: ${e.message}")
    }
}
```

---

## 🔄 **Connection Edge Cases**

### 14. **Simultaneous Bidirectional Connections**

**Status**: ⚠️ INCOMPLETE ARBITRATION

**Location**: Lines 1678-1687 - MAC arbitration in `scanCallback`

**Problem**: Device A scans, Device B scans → both connect to each other before arbitration completes

**Impact**: Two GATT connections for same peer pair (wastes resources)

**Fix Needed**:
```kotlin
// Track connected peer addresses
private val connectedPeerAddresses = mutableSetOf<String>()
private val connectionMutex = Mutex()

private val scanCallback = object : ScanCallback() {
    @SuppressLint("MissingPermission")
    override fun onScanResult(callbackType: Int, result: ScanResult) {
        val peerAddress = result.device.address
        
        appendLog("📡 Discovered PolliNet device $peerAddress (RSSI: ${result.rssi} dBm)")
        
        serviceScope.launch {
            connectionMutex.withLock {
                // Check if already connected to this peer
                if (connectedPeerAddresses.contains(peerAddress)) {
                    appendLog("ℹ️ Already connected to $peerAddress, ignoring")
                    return@launch
                }
                
                // Check if already connected to ANY device
                if (connectedDevice != null || clientGatt != null) {
                    appendLog("ℹ️ Already connected to a device, ignoring discovery")
                    return@launch
                }
                
                // Connection arbitration
                val myAddress = bluetoothAdapter?.address ?: "00:00:00:00:00:00"
                val shouldInitiateConnection = myAddress < peerAddress
                
                if (!shouldInitiateConnection) {
                    appendLog("🔀 Arbitration: Acting as SERVER - peer will connect to me")
                    stopScanning()
                    return@launch
                }
                
                // Mark as connecting
                connectedPeerAddresses.add(peerAddress)
                
                appendLog("🔀 Arbitration: Acting as CLIENT - connecting to peer")
                stopScanning()
                
                mainHandler.postDelayed({
                    appendLog("🔗 Connecting to $peerAddress as GATT client...")
                    connectToDevice(result.device)
                }, 500)
            }
        }
    }
}

// In gattServerCallback.onConnectionStateChange
override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
    when (newState) {
        BluetoothProfile.STATE_CONNECTED -> {
            serviceScope.launch {
                connectionMutex.withLock {
                    // Add to connected set
                    connectedPeerAddresses.add(device.address)
                    
                    _connectionState.value = ConnectionState.CONNECTED
                    connectedDevice = device
                    // ... rest of connection handling ...
                }
            }
        }
        BluetoothProfile.STATE_DISCONNECTED -> {
            serviceScope.launch {
                connectionMutex.withLock {
                    // Remove from connected set
                    connectedPeerAddresses.remove(device.address)
                    
                    _connectionState.value = ConnectionState.DISCONNECTED
                    connectedDevice = null
                    // ... rest of disconnection handling ...
                }
            }
        }
    }
}

// Similar for gattCallback.onConnectionStateChange
```

---

### 15. **Connection Flooding**

**Status**: ❌ MISSING

**Location**: Line 1354 - `connectToDevice()` has no rate limiting

**Problem**: Rapid scan results → multiple `connectGatt()` calls → status 133

**Impact**: Connection failures

**Fix Needed**:
```kotlin
// Track last connection attempt per device
private val lastConnectionAttempts = mutableMapOf<String, Long>()
private const val MIN_CONNECTION_INTERVAL_MS = 5000L // 5 seconds

@SuppressLint("MissingPermission")
fun connectToDevice(device: BluetoothDevice) {
    val address = device.address
    val now = System.currentTimeMillis()
    val lastAttempt = lastConnectionAttempts[address] ?: 0L
    
    if (now - lastAttempt < MIN_CONNECTION_INTERVAL_MS) {
        appendLog("⚠️ Connection attempt to $address too soon, ignoring (${now - lastAttempt}ms ago)")
        return
    }
    
    lastConnectionAttempts[address] = now
    
    _connectionState.value = ConnectionState.CONNECTING
    appendLog("🔗 Connecting to ${device.address}")
    device.connectGatt(this, false, gattCallback)
}

// Cleanup old entries periodically
private fun cleanupConnectionAttempts() {
    val now = System.currentTimeMillis()
    val threshold = now - MIN_CONNECTION_INTERVAL_MS * 2
    lastConnectionAttempts.entries.removeIf { it.value < threshold }
}
```

---

### 16. **Stale clientGatt Reference**

**Status**: ⚠️ POTENTIAL ISSUE

**Location**: Lines 1792-1801 - `onConnectionStateChange` DISCONNECTED

**Problem**: Delayed GATT callbacks reference old connection after `clientGatt = null`

**Impact**: Confusing logs, potential NPE

**Fix Needed**:
```kotlin
// Add connection ID for validation
private var currentConnectionId = AtomicInteger(0)

private val gattCallback = object : BluetoothGattCallback() {
    @SuppressLint("MissingPermission")
    override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
        val connectionId = currentConnectionId.get()
        
        when (newState) {
            BluetoothProfile.STATE_CONNECTED -> {
                currentConnectionId.incrementAndGet()
                _connectionState.value = ConnectionState.CONNECTED
                connectedDevice = gatt.device
                clientGatt = gatt
                // ... rest of connection handling ...
            }
            BluetoothProfile.STATE_DISCONNECTED -> {
                _connectionState.value = ConnectionState.DISCONNECTED
                appendLog("🔌 Disconnected from ${gatt.device.address} (connId=$connectionId)")
                // ... cleanup ...
            }
        }
    }
    
    override fun onDescriptorWrite(
        gatt: BluetoothGatt,
        descriptor: BluetoothGattDescriptor,
        status: Int
    ) {
        val connectionId = currentConnectionId.get()
        appendLog("📝 Descriptor write: status=$status, connId=$connectionId")
        
        // Validate this is for current connection
        if (gatt != clientGatt) {
            appendLog("⚠️ Ignoring descriptor write for stale connection (connId mismatch)")
            return
        }
        
        // ... rest of method ...
    }
    
    // Add similar validation to all other callbacks
}
```

---

## 📦 **Data Integrity**

### 17. **Fragment Ordering**

**Status**: ⚠️ NEEDS DOCUMENTATION

**Location**: Line 1034 - `sdk?.pushInbound(data)`

**Problem**: BLE notifications can arrive out of order - does Rust FFI handle this?

**Impact**: If FFI expects in-order delivery, reassembly fails

**Fix Needed**:
```kotlin
// Add documentation comment
/**
 * Handle received BLE data and forward to Rust FFI transport layer
 * 
 * IMPORTANT: Fragment ordering
 * - BLE notifications are NOT guaranteed to arrive in order
 * - The Rust FFI layer MUST handle out-of-order fragments
 * - Each fragment contains:
 *   - Transaction ID (for grouping)
 *   - Fragment index (for ordering)
 *   - Total fragments (for completion detection)
 * 
 * The Rust reassembly buffer will:
 * 1. Group fragments by transaction ID
 * 2. Sort by fragment index
 * 3. Wait for all fragments before reassembling
 * 4. Timeout incomplete transactions after 5 minutes
 */
private suspend fun handleReceivedData(data: ByteArray) {
    // ... existing implementation ...
}

// Also add sequence validation in logs
appendLog("📥 Fragment received: txId=${extractTxId(data)}, index=${extractIndex(data)}/${extractTotal(data)}")
```

---

### 18. **Partial Write Handling**

**Status**: ❌ INCOMPLETE

**Location**: Line 2244-2307 - `onCharacteristicWriteRequest`, line 2386-2389 - `onExecuteWrite`

**Problem**: Large writes use prepared write (queued) + execute write, but execute write just sends success

**Impact**: Data loss if execute write fails

**Fix Needed**:
```kotlin
// Add prepared write queue
private val preparedWrites = mutableMapOf<String, MutableList<PreparedWrite>>()

data class PreparedWrite(
    val device: BluetoothDevice,
    val characteristic: BluetoothGattCharacteristic,
    val offset: Int,
    val value: ByteArray
)

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
    // ... existing logs ...
    
    val uuidMatches = characteristic.uuid == RX_CHAR_UUID
    
    if (uuidMatches) {
        if (preparedWrite) {
            // This is a prepared write - queue it
            appendLog("📝 PREPARED WRITE - queuing for execute")
            val deviceKey = device.address
            preparedWrites.getOrPut(deviceKey) { mutableListOf() }
                .add(PreparedWrite(device, characteristic, offset, value))
            
            // Send response
            if (responseNeeded) {
                val responseSent = gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null) ?: false
                appendLog("📤 Sent prepared write response: $responseSent")
            }
        } else {
            // Normal write - process immediately
            appendLog("✅ NORMAL WRITE - processing")
            
            if (responseNeeded) {
                val responseSent = gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null) ?: false
                appendLog("📤 Sent write response: $responseSent")
            }
            
            // Process data
            serviceScope.launch {
                if (sdk != null) {
                    handleReceivedData(value)
                }
            }
        }
    } else {
        // ... error handling ...
    }
}

@SuppressLint("MissingPermission")
override fun onExecuteWrite(device: BluetoothDevice, requestId: Int, execute: Boolean) {
    appendLog("📋 EXECUTE WRITE: device=${device.address}, requestId=$requestId, execute=$execute")
    
    val deviceKey = device.address
    val writes = preparedWrites[deviceKey]
    
    if (execute && writes != null) {
        // Combine all prepared writes
        val combinedData = writes.sortedBy { it.offset }
            .flatMap { it.value.toList() }
            .toByteArray()
        
        appendLog("✅ Executing ${writes.size} prepared writes, total ${combinedData.size} bytes")
        
        // Process combined data
        serviceScope.launch {
            if (sdk != null) {
                handleReceivedData(combinedData)
            }
        }
        
        // Clear prepared writes
        preparedWrites.remove(deviceKey)
        
        // Send success response
        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
    } else {
        // Cancelled - clear prepared writes
        appendLog("❌ Execute write cancelled, clearing ${writes?.size ?: 0} prepared writes")
        preparedWrites.remove(deviceKey)
        
        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
    }
}
```

---

### 19. **MTU Change Mid-Transaction**

**Status**: ⚠️ ONLY HANDLES INCREASE

**Location**: Lines 1815-1876 - `onMtuChanged` only handles MTU increase

**Problem**: If MTU drops mid-transmission (device thermal throttling), fragments too large

**Impact**: Write failures, transmission stalls

**Fix Needed**:
```kotlin
override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
    val oldMtu = currentMtu
    currentMtu = mtu
    val maxPayload = (mtu - 10).coerceAtLeast(20)
    val oldMaxPayload = (oldMtu - 10).coerceAtLeast(20)
    appendLog("📏 MTU negotiation complete: $oldMtu → $mtu bytes (status=$status)")
    appendLog("   Max payload per fragment: $maxPayload bytes")
    
    // Handle MTU DECREASE (rare but possible)
    if (mtu < oldMtu) {
        appendLog("⚠️ MTU DECREASED by ${oldMtu - mtu} bytes - re-fragmenting with smaller size")
        appendLog("   This may occur due to thermal throttling or connection degradation")
        
        // Pause sending loop
        sendingJob?.cancel()
        
        // Re-fragment with smaller MTU
        serviceScope.launch {
            val txBytes = pendingTransactionBytes
            if (txBytes != null) {
                val sdkInstance = sdk
                if (sdkInstance != null) {
                    appendLog("♻️ Re-fragmenting ${txBytes.size} bytes with smaller MTU...")
                    val newMaxPayload = (currentMtu - 10).coerceAtLeast(20)
                    sdkInstance.fragment(txBytes, newMaxPayload).onSuccess { fragments ->
                        val newCount = fragments.fragments.size
                        val oldCount = (txBytes.size + oldMaxPayload - 1) / oldMaxPayload
                        appendLog("✅ Re-fragmented: $oldCount → $newCount fragments (MORE fragments due to smaller MTU)")
                        
                        fragmentsQueuedWithMtu = currentMtu
                        ensureSendingLoopStarted()
                    }.onFailure {
                        appendLog("❌ Re-fragmentation failed: ${it.message}")
                        ensureSendingLoopStarted()
                    }
                }
            }
        }
        
        // Don't proceed with increase logic
        return
    }
    
    // Handle MTU INCREASE (existing logic)
    val mtuIncrease = mtu - oldMtu
    if (mtuIncrease >= 30 && pendingTransactionBytes != null) {
        // ... existing increase logic ...
    }
    
    // ... rest of method ...
}
```

---

## 🧠 **Memory Leaks**

### 20. **pendingTransactionBytes Never Cleared**

**Status**: ⚠️ INCOMPLETE CLEANUP

**Location**: Lines 296, 332, 406 - `pendingTransactionBytes` set, rarely cleared

**Problem**: If connection stays up for hours, keeps old transaction bytes in memory

**Impact**: Memory leak, especially if transactions are large

**Fix Needed**:
```kotlin
// Add timestamp tracking
private var pendingTransactionTimestamp = 0L
private const val PENDING_TX_TIMEOUT_MS = 300_000L // 5 minutes

// In sendNextOutbound (line 1398-1453)
private suspend fun sendNextOutbound() {
    sendingMutex.lock()
    try {
        // ... existing connection checks ...
        
        val sdkInstance = sdk ?: run {
            appendLog("⚠️ sendNextOutbound: SDK is null")
            return
        }
        
        val safeMaxLen = (currentMtu - 10).coerceAtLeast(20)
        val data = sdkInstance.nextOutbound(maxLen = safeMaxLen)
        
        if (data == null) {
            // Check if pending transaction is stale
            val now = System.currentTimeMillis()
            if (pendingTransactionBytes != null && 
                pendingTransactionTimestamp > 0 &&
                now - pendingTransactionTimestamp > PENDING_TX_TIMEOUT_MS) {
                
                appendLog("⚠️ Pending transaction timed out (${PENDING_TX_TIMEOUT_MS}ms), clearing")
                pendingTransactionBytes = null
                pendingTransactionTimestamp = 0
                fragmentsQueuedWithMtu = 0
                return
            }
            
            // No more data to send - wait before clearing
            if (pendingTransactionBytes != null) {
                appendLog("📭 Queue empty - waiting for notification delivery confirmation...")
                delay(2000)
                
                if (_connectionState.value == ConnectionState.CONNECTED) {
                    appendLog("✅ All fragments delivered successfully, clearing pending transaction")
                    pendingTransactionBytes = null
                    pendingTransactionTimestamp = 0
                    fragmentsQueuedWithMtu = 0
                }
            }
            return
        }

        appendLog("➡️ Sending fragment (${data.size}B)")
        sendToGatt(data)
        
    } catch (e: Exception){
        appendLog("❌ Exception in sendNextOutbound: ${e.message}")
    } finally {
        sendingMutex.unlock()
    }
}

// Update when setting pendingTransactionBytes
fun queueSampleTransaction(byteSize: Int = 1024) {
    // ... validation ...
    serviceScope.launch {
        // ... fragmentation ...
        sdkInstance.fragment(payload, maxPayload).onSuccess { fragments ->
            // ... logging ...
            
            pendingTransactionBytes = payload
            pendingTransactionTimestamp = System.currentTimeMillis()
            fragmentsQueuedWithMtu = currentMtu
            
            ensureSendingLoopStarted()
        }
    }
}

// Similar for queueTransactionFromBase64 and queueSignedTransaction
```

---

### 21. **Handler postDelayed Leaks**

**Status**: ✅ COMPLETED (Dec 30, 2025)

**Location**: Multiple `mainHandler.postDelayed` calls throughout

**Problem**: Pending callbacks can keep service reference alive

**Impact**: Memory leak after service stopped

**Implementation**: Added removeCallbacksAndMessages(null) at start of onDestroy

**Fix Needed**:
```kotlin
// In onDestroy (line 1073-1112)
override fun onDestroy() {
    // Cancel all pending handler callbacks FIRST
    mainHandler.removeCallbacksAndMessages(null)
    appendLog("🧹 Cancelled all pending handler callbacks")
    
    // Phase 5: Force save queues before shutdown
    runBlocking {
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
    unifiedWorker?.cancel()
    autoSaveJob?.cancel()
    serviceScope.cancel()
    
    // ... rest of cleanup ...
    
    super.onDestroy()
}
```

---

### 22. **networkCallback Leak**

**Status**: ⚠️ UNREGISTERED BUT RISKY

**Location**: Line 867 - `registerNetworkCallback`, line 882 - `unregisterNetworkCallback`

**Problem**: NetworkCallback can leak if service crashes before onDestroy

**Impact**: Memory leak

**Fix Needed**:
```kotlin
// Use try-catch in registration
private fun registerNetworkCallback() {
    try {
        val connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        
        networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                // Use weak reference to service
                this@BleService.let { service ->
                    service.appendLog("📡 Network available - triggering pending work")
                    service.workChannel.trySend(WorkEvent.ReceivedReady)
                    service.workChannel.trySend(WorkEvent.RetryReady)
                }
            }
            // ... rest of callbacks ...
        }
        
        val networkRequest = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .build()
        
        connectivityManager.registerNetworkCallback(networkRequest, networkCallback!!)
        appendLog("✅ Network callback registered")
        
    } catch (e: Exception) {
        appendLog("⚠️ Failed to register network callback: ${e.message}")
        networkCallback = null // Ensure it's null on failure
    }
}

// Enhanced unregister
private fun unregisterNetworkCallback() {
    networkCallback?.let { callback ->
        try {
            val connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            connectivityManager?.unregisterNetworkCallback(callback)
            appendLog("✅ Network callback unregistered")
        } catch (e: IllegalArgumentException) {
            appendLog("⚠️ Network callback not registered")
        } catch (e: Exception) {
            appendLog("⚠️ Failed to unregister network callback: ${e.message}")
        } finally {
            networkCallback = null
        }
    }
}
```

---

## 🎯 **State Management**

### 23. **Multiple Advertising/Scanning Instances**

**Status**: ❌ MISSING GUARD

**Location**: Lines 1276-1321 `startAdvertising()`, lines 1221-1257 `startScanning()`

**Problem**: Calling `startAdvertising()` twice creates duplicate ads

**Impact**: Wastes power, confuses peers

**Fix Needed**:
```kotlin
@SuppressLint("MissingPermission")
fun startAdvertising() {
    // Check if already advertising
    if (_isAdvertising.value) {
        appendLog("⚠️ Already advertising - ignoring duplicate start request")
        return
    }
    
    // Check if Bluetooth is enabled
    if (bluetoothAdapter?.isEnabled != true) {
        appendLog("❌ Cannot start advertising: Bluetooth is disabled")
        appendLog("📱 Please enable Bluetooth in Settings")
        return
    }
    
    // ... rest of method ...
}

@SuppressLint("MissingPermission")
fun startScanning() {
    // Check if already scanning
    if (_isScanning.value) {
        appendLog("⚠️ Already scanning - ignoring duplicate start request")
        return
    }
    
    // Check if Bluetooth is enabled
    if (bluetoothAdapter?.isEnabled != true) {
        appendLog("❌ Cannot start scanning: Bluetooth is disabled")
        appendLog("📱 Please enable Bluetooth in Settings")
        return
    }
    
    // ... rest of method ...
}
```

---

### 24. **State Inconsistency on Errors**

**Status**: ⚠️ PARTIAL CLEANUP

**Location**: Line 1758 - `onConnectionStateChange` error handling

**Problem**: State says disconnected but `clientGatt` still set

**Impact**: Confused logic in other parts

**Fix Needed**:
```kotlin
override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
    appendLog("🔄 Connection state change: status=$status, newState=$newState")
    
    // Handle error statuses
    if (status != BluetoothGatt.GATT_SUCCESS) {
        appendLog("❌ Connection error: status=$status")
        
        // Clean up ALL connection state on error
        val deviceAddress = gatt.device.address
        
        when (status) {
            5, 15 -> {
                appendLog("🔐 Authentication/Encryption required - creating bond...")
                try {
                    gatt.device.createBond()
                } catch (e: Exception) {
                    appendLog("❌ Failed to create bond: ${e.message}")
                    // Clean up on bond failure
                    cleanupConnection(gatt)
                }
            }
            133 -> {
                appendLog("⚠️ GATT_ERROR - refreshing cache and retrying...")
                refreshDeviceCache(gatt)
                gatt.close()
                clientGatt = null
                cleanupConnection(gatt)
            }
            else -> {
                appendLog("❌ Error: See https://developer.android.com/reference/android/bluetooth/BluetoothGatt")
                cleanupConnection(gatt)
            }
        }
        
        _connectionState.value = ConnectionState.DISCONNECTED
        return
    }
    
    // ... rest of method ...
}

// Helper function to clean up connection state
private fun cleanupConnection(gatt: BluetoothGatt) {
    if (gatt == clientGatt) {
        connectedDevice = null
        clientGatt = null
        remoteTxCharacteristic = null
        remoteRxCharacteristic = null
        remoteWriteInProgress = false
        operationInProgress.set(false)
        operationQueue.clear()
        descriptorWriteRetries = 0
        descriptorWriteComplete = false
        pendingDescriptorWrite = null
        pendingGatt = null
        sendingJob?.cancel()
        
        appendLog("🧹 Cleaned up connection state for ${gatt.device.address}")
    }
}
```

---

### 25. **Connection State During Retry**

**Status**: ⚠️ POOR UX

**Location**: Lines 2486-2502 - `handleStatus133`

**Problem**: No "RECONNECTING" state, UI shows disconnected during retry

**Impact**: Users think connection failed when it's actually retrying

**Fix Needed**:
```kotlin
// Add new state
enum class ConnectionState {
    DISCONNECTED,
    SCANNING,
    CONNECTING,
    CONNECTED,
    RECONNECTING,  // NEW
    ERROR
}

@SuppressLint("MissingPermission")
private fun handleStatus133(gatt: BluetoothGatt) {
    appendLog("⚠️ Handling status 133 - clearing cache and reconnecting")
    refreshDeviceCache(gatt)
    gatt.close()
    clientGatt = null
    _connectionState.value = ConnectionState.RECONNECTING  // Changed from DISCONNECTED
    
    // Retry connection after delay
    val device = gatt.device
    mainHandler.postDelayed({
        if (_connectionState.value == ConnectionState.RECONNECTING) {
            appendLog("🔄 Retrying connection after status 133...")
            try {
                device.connectGatt(this, false, gattCallback)
            } catch (e: Exception) {
                appendLog("❌ Retry connection failed: ${e.message}")
                _connectionState.value = ConnectionState.DISCONNECTED
            }
        } else {
            appendLog("⚠️ Connection state changed during retry delay, aborting")
        }
    }, 1000)
}

// Similar for descriptor write retry
override fun onDescriptorWrite(
    gatt: BluetoothGatt,
    descriptor: BluetoothGattDescriptor,
    status: Int
) {
    // ... existing checks ...
    
    if (status == 133) {
        sendingJob?.cancel()
        appendLog("⚠️ Status 133 detected - pausing sending loop for recovery")
        
        if (descriptorWriteRetries < MAX_DESCRIPTOR_RETRIES) {
            _connectionState.value = ConnectionState.RECONNECTING  // Show reconnecting
            descriptorWriteRetries++
            // ... rest of retry logic ...
        }
    }
}
```

---

## 🔒 **Security**

### 26. **No Transaction Validation**

**Status**: ❌ MISSING

**Location**: Lines 1027-1063 - `handleReceivedData`

**Problem**: Malicious peer can send garbage data

**Impact**: Wastes relay bandwidth, RPC submission attempts

**Fix Needed**:
```kotlin
/**
 * Validate Solana transaction structure
 * Basic checks to prevent garbage data from malicious peers
 */
private fun isValidSolanaTransaction(data: ByteArray): Boolean {
    try {
        // Minimum Solana transaction size (1 signature + minimal instruction data)
        if (data.size < 64) {
            appendLog("⚠️ Transaction too small: ${data.size} bytes")
            return false
        }
        
        // Maximum reasonable transaction size (1232 bytes per Solana docs)
        if (data.size > MAX_TRANSACTION_SIZE) {
            appendLog("⚠️ Transaction too large: ${data.size} bytes")
            return false
        }
        
        // Check for PolliNet fragment structure (if this is a fragment)
        // First byte could be version/type indicator
        // This is application-specific - adjust based on your protocol
        
        // TODO: Add more validation:
        // - Check signature format (first 64 bytes should be valid Ed25519 signature)
        // - Validate message structure
        // - Check account indices are in bounds
        // For now, size check is sufficient to prevent most garbage
        
        return true
    } catch (e: Exception) {
        appendLog("❌ Transaction validation failed: ${e.message}")
        return false
    }
}

private suspend fun handleReceivedData(data: ByteArray) {
    try {
        appendLog("📥 ===== PROCESSING RECEIVED DATA =====")
        appendLog("📥 Data size: ${data.size} bytes")
        
        // Validate data before processing
        if (!isValidSolanaTransaction(data)) {
            appendLog("❌ Invalid transaction data - rejecting")
            appendLog("📥 ===== END PROCESSING (REJECTED) =====\n")
            return
        }
        
        appendLog("📥 Data preview: ${data.take(32).joinToString(" ") { "%02X".format(it) }}...")
        
        // Push to SDK for reassembly
        val result = sdk?.pushInbound(data)
        // ... rest of method ...
        
    } catch (e: Exception) {
        appendLog("❌ ❌ ❌ Exception in handleReceivedData ❌ ❌ ❌")
        appendLog("❌ Error: ${e.message}")
    }
}
```

---

### 27. **No Peer Authentication**

**Status**: ❌ MISSING

**Location**: Entire BLE service - No peer verification

**Problem**: Anyone can advertise the service UUID and inject data

**Impact**: DOS attack vector, transaction spam

**Fix Needed**:
```kotlin
// This is a significant feature that requires careful design
// Here's a high-level approach:

/**
 * Peer authentication using public key cryptography
 * 
 * Phase 1: Handshake
 * 1. Client connects to server
 * 2. Server sends challenge (random bytes)
 * 3. Client signs challenge with private key
 * 4. Server verifies signature with client's public key
 * 5. If valid, connection trusted; if not, disconnect
 * 
 * Phase 2: Authenticated communication
 * 1. All fragments include signature
 * 2. Receiver verifies signature before processing
 * 3. Invalid signatures are dropped
 */

// Add to companion object
companion object {
    // Authentication characteristic for handshake
    val AUTH_CHAR_UUID: JavaUUID = JavaUUID.fromString("00001823-0000-1000-8000-00805f9b34fb")
}

// Track authenticated peers
private val authenticatedPeers = mutableSetOf<String>()

private fun isAuthenticatedPeer(address: String): Boolean {
    return authenticatedPeers.contains(address)
}

// In handleReceivedData, check authentication
private suspend fun handleReceivedData(data: ByteArray) {
    try {
        // Get sender address (from GATT callback context)
        val senderAddress = getCurrentSenderAddress() // You'll need to track this
        
        // Check if peer is authenticated
        if (!isAuthenticatedPeer(senderAddress)) {
            appendLog("❌ Rejecting data from unauthenticated peer: $senderAddress")
            return
        }
        
        // ... rest of processing ...
    } catch (e: Exception) {
        appendLog("❌ Exception in handleReceivedData: ${e.message}")
    }
}

// TODO: Implement full authentication flow
// This is a complex feature that needs:
// - Key management (store peer public keys)
// - Challenge-response protocol
// - Signature verification (Ed25519)
// - Key distribution mechanism
// 
// Consider using existing libraries:
// - Libsodium for crypto
// - Protocol Buffers for message serialization
```

---

### 28. **No Rate Limiting on Received Data**

**Status**: ❌ MISSING

**Location**: Lines 2282-2295 - `onCharacteristicWriteRequest`

**Problem**: Malicious peer can flood with writes

**Impact**: DOS attack, battery drain, memory exhaustion

**Fix Needed**:
```kotlin
// Add rate limiting per peer
private val peerWriteTimestamps = mutableMapOf<String, MutableList<Long>>()
private const val MAX_WRITES_PER_SECOND = 20
private const val RATE_LIMIT_WINDOW_MS = 1000L

private fun isRateLimited(deviceAddress: String): Boolean {
    val now = System.currentTimeMillis()
    val timestamps = peerWriteTimestamps.getOrPut(deviceAddress) { mutableListOf() }
    
    // Remove timestamps older than window
    timestamps.removeIf { it < now - RATE_LIMIT_WINDOW_MS }
    
    // Check if rate limit exceeded
    if (timestamps.size >= MAX_WRITES_PER_SECOND) {
        return true
    }
    
    // Add current timestamp
    timestamps.add(now)
    return false
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
    
    // Check rate limit
    if (isRateLimited(device.address)) {
        appendLog("⚠️ ⚠️ ⚠️ RATE LIMIT EXCEEDED for ${device.address} ⚠️ ⚠️ ⚠️")
        appendLog("   Rejecting write request")
        
        // Send error response
        if (responseNeeded) {
            gattServer?.sendResponse(
                device,
                requestId,
                BluetoothGatt.GATT_WRITE_NOT_PERMITTED,
                0,
                null
            )
        }
        return
    }
    
    // ... rest of existing logic ...
}

// Cleanup old rate limit data periodically
private fun cleanupRateLimitData() {
    val now = System.currentTimeMillis()
    peerWriteTimestamps.entries.removeIf { (_, timestamps) ->
        timestamps.removeIf { it < now - RATE_LIMIT_WINDOW_MS * 2 }
        timestamps.isEmpty()
    }
}

// Call from cleanup job
private suspend fun processCleanup() {
    val sdkInstance = sdk ?: return
    
    appendLog("🧹 Running cleanup...")
    
    // ... existing cleanup ...
    
    // Cleanup rate limit data
    cleanupRateLimitData()
    cleanupConnectionAttempts()
}
```

---

## 🧪 **Testing & Observability**

### 29. **No Crash Reporting Integration**

**Status**: ❌ MISSING

**Location**: No crash reporting SDK integrated

**Problem**: Production crashes go unreported

**Impact**: Can't fix bugs users encounter

**Fix Needed**:
```kotlin
// Add to build.gradle.kts
dependencies {
    // Firebase Crashlytics (recommended)
    implementation(platform("com.google.firebase:firebase-bom:32.7.0"))
    implementation("com.google.firebase:firebase-crashlytics-ktx")
    implementation("com.google.firebase:firebase-analytics-ktx")
    
    // Or use Sentry as alternative
    // implementation("io.sentry:sentry-android:7.0.0")
}

// In BleService.kt
import com.google.firebase.crashlytics.FirebaseCrashlytics

class BleService : Service() {
    
    private val crashlytics = FirebaseCrashlytics.getInstance()
    
    override fun onCreate() {
        super.onCreate()
        
        // Set custom keys for debugging
        crashlytics.setCustomKey("service_version", "1.0.0")
        crashlytics.setCustomKey("ble_enabled", bluetoothAdapter?.isEnabled ?: false)
        
        // ... rest of onCreate ...
    }
    
    // Wrap critical sections with try-catch and report
    private suspend fun handleReceivedData(data: ByteArray) {
        try {
            appendLog("📥 ===== PROCESSING RECEIVED DATA =====")
            // ... existing code ...
        } catch (e: Exception) {
            appendLog("❌ Exception in handleReceivedData: ${e.message}")
            
            // Report to Crashlytics
            crashlytics.log("handleReceivedData failed with data size: ${data.size}")
            crashlytics.recordException(e)
            
            // Log context
            crashlytics.setCustomKey("last_connection_state", _connectionState.value.toString())
            crashlytics.setCustomKey("sdk_initialized", sdk != null)
        }
    }
    
    // Add non-fatal error tracking
    private fun reportNonFatal(message: String, exception: Throwable? = null) {
        appendLog("⚠️ Non-fatal error: $message")
        crashlytics.log(message)
        exception?.let { crashlytics.recordException(it) }
    }
    
    // Example usage
    @SuppressLint("MissingPermission")
    private fun handleStatus133(gatt: BluetoothGatt) {
        reportNonFatal("Status 133 encountered for device ${gatt.device.address}")
        appendLog("⚠️ Handling status 133 - clearing cache and reconnecting")
        // ... rest of method ...
    }
}
```

---

### 30. **No Analytics/Telemetry**

**Status**: ❌ MISSING

**Location**: No metrics collection for production monitoring

**Problem**: Can't measure system performance in production

**Impact**: Don't know if mesh relay actually works at scale

**Fix Needed**:
```kotlin
// Add to build.gradle.kts
dependencies {
    // Firebase Analytics
    implementation("com.google.firebase:firebase-analytics-ktx")
}

// In BleService.kt
import com.google.firebase.analytics.FirebaseAnalytics
import com.google.firebase.analytics.ktx.analytics
import com.google.firebase.analytics.ktx.logEvent
import com.google.firebase.ktx.Firebase

class BleService : Service() {
    
    private lateinit var analytics: FirebaseAnalytics
    
    override fun onCreate() {
        super.onCreate()
        
        analytics = Firebase.analytics
        
        // ... rest of onCreate ...
    }
    
    // Track key metrics
    private fun trackFragmentSent(fragmentSize: Int, success: Boolean) {
        analytics.logEvent("fragment_sent") {
            param("size_bytes", fragmentSize.toLong())
            param("success", if (success) 1L else 0L)
            param("mtu", currentMtu.toLong())
        }
    }
    
    private fun trackFragmentReceived(fragmentSize: Int, success: Boolean) {
        analytics.logEvent("fragment_received") {
            param("size_bytes", fragmentSize.toLong())
            param("success", if (success) 1L else 0L)
        }
    }
    
    private fun trackTransactionSubmitted(
        duration: Long,
        fragmentCount: Int,
        success: Boolean
    ) {
        analytics.logEvent("transaction_submitted") {
            param("duration_ms", duration)
            param("fragment_count", fragmentCount.toLong())
            param("success", if (success) 1L else 0L)
        }
    }
    
    private fun trackConnectionEvent(
        event: String,  // "connected", "disconnected", "failed"
        rssi: Int = 0,
        status: Int = 0
    ) {
        analytics.logEvent("connection_event") {
            param("event_type", event)
            param("rssi", rssi.toLong())
            param("status", status.toLong())
        }
    }
    
    private fun trackBatteryImpact(
        wakeUpsPerMin: Int,
        batteryLevel: Int
    ) {
        analytics.logEvent("battery_metrics") {
            param("wakeups_per_min", wakeUpsPerMin.toLong())
            param("battery_level", batteryLevel.toLong())
        }
    }
    
    // Use in appropriate places
    @SuppressLint("MissingPermission")
    private fun sendToGatt(data: ByteArray) {
        val startTime = System.currentTimeMillis()
        
        // ... existing send logic ...
        
        // Track success/failure
        if (success) {
            val duration = System.currentTimeMillis() - startTime
            trackFragmentSent(data.size, true)
        } else {
            trackFragmentSent(data.size, false)
        }
    }
    
    private suspend fun handleReceivedData(data: ByteArray) {
        try {
            // ... existing processing ...
            
            result?.onSuccess {
                trackFragmentReceived(data.size, true)
            }?.onFailure {
                trackFragmentReceived(data.size, false)
            }
            
        } catch (e: Exception) {
            trackFragmentReceived(data.size, false)
        }
    }
    
    // Track overall metrics periodically
    private fun logBatteryMetrics() {
        try {
            val batteryManager = getSystemService(Context.BATTERY_SERVICE) as? BatteryManager
            val batteryPct = batteryManager?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY) ?: -1
            
            val wakeUpsPerMin = if (System.currentTimeMillis() - lastEventTime > 60_000) {
                0
            } else {
                wakeUpCount
            }
            
            // Track battery impact
            trackBatteryImpact(wakeUpsPerMin, batteryPct)
            
            // ... rest of existing code ...
        } catch (e: Exception) {
            // Ignore errors
        }
    }
}
```

---

## 📋 **Priority Recommendations**

### 🔴 **Critical (Fix Immediately)**

These issues can cause crashes, data loss, or severe battery drain:

1. **✅ Queue Size Limits** (#3) - ✅ COMPLETED
   - **Risk**: OutOfMemoryError crashes
   - **Effort**: Low (30 minutes) - Actual: 30 minutes
   - **Impact**: High (prevents app crashes)
   - **Completed**: Dec 30, 2025

2. **✅ Bluetooth State Receiver** (#1) - ✅ COMPLETED
   - **Risk**: Battery drain when BT disabled
   - **Effort**: Medium (1-2 hours) - Actual: 30 minutes (!!)
   - **Impact**: High (prevents wasted battery)
   - **Completed**: Dec 30, 2025

3. **✅ operationInProgress Synchronization** (#8) - ✅ COMPLETED
   - **Risk**: Data corruption, status 133 errors
   - **Effort**: Low (30 minutes) - Actual: 25 minutes
   - **Impact**: High (prevents connection failures)
   - **Completed**: Dec 30, 2025

4. **✅ Transaction Size Validation** (#5) - ✅ COMPLETED
   - **Risk**: OutOfMemoryError, DOS attacks
   - **Effort**: Low (30 minutes) - Actual: 20 minutes
   - **Impact**: High (prevents abuse)
   - **Completed**: Dec 30, 2025

5. **✅ Handler Cleanup** (#21) - ✅ COMPLETED
   - **Risk**: Memory leaks
   - **Effort**: Low (15 minutes) - Actual: 10 minutes
   - **Impact**: Medium (prevents slow leaks)
   - **Completed**: Dec 30, 2025

---

### 🟡 **High Priority (Fix This Week)**

Important for production stability:

6. **✅ Permission Runtime Checks** (#2)
   - **Risk**: SecurityException crashes
   - **Effort**: Medium (1-2 hours)
   - **Impact**: Medium (prevents crashes in specific scenarios)

7. **✅ Fragment Timeout** (#4)
   - **Risk**: Memory leaks from incomplete transactions
   - **Effort**: Low (Rust FFI change required)
   - **Impact**: Medium (prevents slow memory leak)

8. **✅ RPC Timeout** (#13)
   - **Risk**: Blocked event worker
   - **Effort**: Low (30 minutes)
   - **Impact**: Medium (prevents hangs)

9. **✅ Rate Limiting on Incoming Data** (#28)
   - **Risk**: DOS attacks
   - **Effort**: Medium (1-2 hours)
   - **Impact**: High (security hardening)

10. **✅ Stale clientGatt Reference** (#16)
    - **Risk**: Confusing logs, potential NPE
    - **Effort**: Low (1 hour)
    - **Impact**: Low (code quality improvement)

---

### 🟢 **Medium Priority (Fix This Month)**

Nice-to-have for better UX and reliability:

11. **✅ Doze Mode Handling** (#9)
    - **Risk**: Mesh relay stops in background
    - **Effort**: High (requires architectural changes)
    - **Impact**: Medium (better background performance)

12. **✅ Low Battery Optimization** (#10)
    - **Risk**: Fast battery drain at low levels
    - **Effort**: Low (1 hour)
    - **Impact**: Medium (better user experience)

13. **✅ Transaction Validation** (#26)
    - **Risk**: Wastes bandwidth on garbage data
    - **Effort**: Medium (2-3 hours)
    - **Impact**: Medium (security hardening)

14. **✅ Connection Flooding Prevention** (#15)
    - **Risk**: Connection failures
    - **Effort**: Low (1 hour)
    - **Impact**: Low (edge case)

15. **✅ Crash Reporting** (#29)
    - **Risk**: Can't diagnose production issues
    - **Effort**: Low (30 minutes setup)
    - **Impact**: High (visibility into production)

16. **✅ RPC Rate Limiting** (#11)
    - **Risk**: Failed transactions on 429 errors
    - **Effort**: Medium (2-3 hours)
    - **Impact**: Medium (robustness)

17. **✅ Transaction Already Confirmed** (#12)
    - **Risk**: Wastes retry attempts
    - **Effort**: Low (1 hour)
    - **Impact**: Low (optimization)

---

### 🔵 **Low Priority (Future Enhancements)**

Nice improvements for the future:

18. **✅ Analytics/Telemetry** (#30)
    - **Risk**: No production metrics
    - **Effort**: Medium (2-3 hours)
    - **Impact**: Medium (observability)

19. **✅ Peer Authentication** (#27)
    - **Risk**: Anyone can inject data
    - **Effort**: Very High (1-2 weeks)
    - **Impact**: High (security, but complex)

20. **✅ Reconnecting State** (#25)
    - **Risk**: Confusing UX during retries
    - **Effort**: Low (30 minutes)
    - **Impact**: Low (UX polish)

21. **✅ Simultaneous Bidirectional Connections** (#14)
    - **Risk**: Resource waste
    - **Effort**: Medium (2-3 hours)
    - **Impact**: Low (rare edge case)

22. **✅ pendingTransactionBytes Cleanup** (#20)
    - **Risk**: Slow memory leak
    - **Effort**: Low (1 hour)
    - **Impact**: Low (long-running connections only)

23. **✅ Multiple Advertising/Scanning Guard** (#23)
    - **Risk**: Duplicate BLE operations
    - **Effort**: Low (15 minutes)
    - **Impact**: Low (code quality)

24. **✅ State Inconsistency on Errors** (#24)
    - **Risk**: Confused state management
    - **Effort**: Low (1 hour)
    - **Impact**: Low (code quality)

25. **✅ MTU Decrease Handling** (#19)
    - **Risk**: Write failures on MTU drop
    - **Effort**: Low (1 hour)
    - **Impact**: Very Low (extremely rare)

26. **✅ Fragment Ordering Documentation** (#17)
    - **Risk**: None (documentation only)
    - **Effort**: Very Low (30 minutes)
    - **Impact**: Very Low (clarity)

27. **✅ Partial Write Handling** (#18)
    - **Risk**: Data loss on large writes
    - **Effort**: Medium (2-3 hours)
    - **Impact**: Very Low (rare, Android handles most cases)

28. **✅ Concurrent SDK Access** (#7)
    - **Risk**: Depends on Rust FFI thread-safety
    - **Effort**: Low (verification + potential mutex)
    - **Impact**: Critical IF FFI isn't thread-safe

29. **✅ Descriptor Write Race Condition** (#6)
    - **Risk**: Concurrent operations
    - **Effort**: Medium (2 hours)
    - **Impact**: Low (hard to trigger)

30. **✅ networkCallback Leak** (#22)
    - **Risk**: Memory leak on crash
    - **Effort**: Low (30 minutes)
    - **Impact**: Very Low (rare)

---

## 📊 **Implementation Timeline**

### Week 1: Critical Fixes
- Day 1-2: #3, #8, #21 (Queue limits, synchronization, handler cleanup)
- Day 3-4: #1, #5 (Bluetooth state, transaction validation)
- Day 5: Testing and validation

### Week 2: High Priority
- Day 1-2: #2, #13, #28 (Permissions, RPC timeout, rate limiting)
- Day 3-4: #4, #16 (Fragment timeout, stale references)
- Day 5: Testing and validation

### Week 3: Medium Priority
- Day 1-2: #10, #15, #29 (Battery optimization, connection flooding, crash reporting)
- Day 3-4: #11, #12, #26 (RPC rate limiting, already confirmed, validation)
- Day 5: Testing and validation

### Week 4+: Low Priority & Polish
- Analytics integration (#30)
- UX improvements (#25)
- Code quality improvements (#20, #23, #24)
- Consider peer authentication (#27) for v2.0

---

## 🎯 **Success Metrics**

Track these metrics to measure improvement:

1. **Stability**
   - Crash-free rate: Target 99.9%
   - OutOfMemoryError count: Target 0

2. **Battery**
   - Wake-ups per minute: Keep < 5 (currently achieved)
   - Battery drain per hour: Target < 2%

3. **Performance**
   - Fragment success rate: Target > 95%
   - Transaction submission success rate: Target > 90%
   - Average fragment latency: Target < 200ms

4. **Security**
   - Rate limit violations: Track count
   - Invalid transaction rejections: Track count

---

## 📝 **Notes**

- This document is a living guide - update as issues are fixed
- Mark items as ✅ when implemented
- Add new edge cases as discovered
- Prioritize based on your production usage patterns

**Last Updated**: December 30, 2025
**Version**: 1.0.0
**Maintainer**: PolliNet Team

