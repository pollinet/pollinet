# PolliNet Queue System - Implementation TODO

**Date Created:** December 22, 2025  
**Status:** Planning Phase  
**Priority:** High  

---

## 📋 Overview

This document outlines the complete implementation plan for the optimized queue system in PolliNet. The system consists of 4 primary queues with a **battery-optimized event-driven architecture** replacing traditional polling loops.

### 🎯 Quick Reference

**Architecture:** Event-Driven (not polling!)
- ✅ Single unified worker using Kotlin Channels
- ✅ WorkManager for scheduled tasks (retries, cleanup)
- ✅ 85%+ battery improvement vs polling approach
- ✅ Doze mode compatible

**4 Primary Queues:**
1. **Outbound Queue** - Transactions waiting for BLE transmission (priority-based)
2. **Reassembly Buffer** - Incoming fragments being reassembled (SHA-256 matching)
3. **Confirmation Queue** - Transaction confirmations to relay back to origin
4. **Retry Queue** - Failed submissions with exponential backoff

**Key Features:**
- 🔐 SHA-256 fragment identification (cross-device compatible)
- 🔋 Event-driven (< 10 wake-ups/min vs 150 with polling)
- 💾 Persistent queues (survive app restart)
- 🔄 Exponential backoff retry logic
- 📊 Real-time metrics and monitoring
- ⚡ < 100ms event processing latency

---

## 🎯 Queue Architecture

### Queue 1: Outbound BLE Transmission Queue
**Purpose:** Queue transactions for BLE relay (sending to other devices)

### Queue 2: Inbound Fragment Reassembly Queue  
**Purpose:** Receive and reassemble fragmented transactions

### Queue 3: Confirmation Relay Queue
**Purpose:** Fragment and relay blockchain confirmations back to origin

### Queue 4: Retry Queue
**Purpose:** Retry failed submission attempts with exponential backoff

---

## ✅ Implementation Tasks

### Phase 1: Core Queue Data Structures (Rust)

#### 1.1 Outbound Queue Implementation
- [ ] Create `src/queue/outbound.rs` module
- [ ] Define `OutboundTransaction` struct with:
  - `tx_id: String` (SHA-256 hash)
  - `original_bytes: Vec<u8>` (original signed transaction)
  - `fragments: Vec<TransactionFragment>` (pre-fragmented)
  - `priority: Priority` enum (HIGH, NORMAL, LOW)
  - `created_at: u64` (timestamp)
  - `retry_count: u8`
  - `max_retries: u8`
- [ ] Implement `OutboundQueue` struct with:
  - `high_priority: VecDeque<OutboundTransaction>`
  - `normal_priority: VecDeque<OutboundTransaction>`
  - `low_priority: VecDeque<OutboundTransaction>`
  - `deduplication_set: HashSet<String>` (prevent duplicates)
- [ ] Add methods:
  - `push(&mut self, tx: OutboundTransaction, priority: Priority) -> Result<(), Error>`
  - `pop(&mut self) -> Option<OutboundTransaction>` (priority-based)
  - `contains(&self, tx_id: &str) -> bool` (deduplication check)
  - `len(&self) -> usize`
  - `clear(&mut self)`
- [ ] Add thread-safety with `Arc<RwLock<OutboundQueue>>`
- [ ] Add serialization support for persistence

#### 1.2 Enhanced Reassembly Buffer

**🔐 Fragment Identification Mechanism (Already Implemented in `src/ble/fragmenter.rs`):**

Your system already has a robust fragment matching mechanism:
- Each fragment contains `transaction_id` (32-byte SHA-256 hash of complete transaction)
- All fragments of same transaction share identical `transaction_id`
- Receiving devices group fragments by `transaction_id` using HashMap
- Works across multiple relaying devices (fragments can arrive from different sources)
- Cryptographic verification after reassembly ensures integrity
- **Zero additional BLE overhead** - `transaction_id` already part of fragment structure

**Reassembly Process Flow:**
1. Receive fragment with `transaction_id` and `fragment_index`
2. Store in buffer: `reassembly_buffers[transaction_id][fragment_index] = fragment`
3. Check if all fragments received (all indices 0..total_fragments present)
4. Reassemble by concatenating fragments in order
5. Verify SHA-256 hash of reassembled transaction matches `transaction_id`
6. If verified: move to received queue for submission
7. If mismatch: reject and log error (tamper/corruption detected)

**Implementation Tasks:**
- [ ] Enhance existing `TransactionCache` in `src/transaction/mod.rs`
- [ ] **NOTE:** TransactionFragment structure for BLE remains unchanged (no additional overhead)
- [ ] Add `FragmentSet` struct **for local storage only** (not transmitted over BLE):
  - `transaction_id: [u8; 32]` (from first fragment received)
  - `total_fragments: u16` (from first fragment received)
  - `received_fragments: Vec<Option<TransactionFragment>>` (storage for incoming fragments)
  - `first_received: Instant` (local timestamp - NOT transmitted)
  - `last_updated: Instant` (local timestamp - NOT transmitted)
- [ ] Add computed methods (no storage overhead):
  - `received_count() -> usize` (count non-None fragments)
  - `expected_size() -> usize` (estimate from received fragments)
  - `is_complete() -> bool` (all fragments received)
  - `is_stale() -> bool` (older than 5 minutes)
- [ ] ~~Remove MetaInfo struct~~ (redundant - use computed properties instead)
- [ ] Implement optimized `add_fragment()` method:
  - O(1) insertion using `fragment_index` as Vec index
  - Use `transaction_id` as HashMap key for O(1) fragment grouping
  - Automatic completion detection via `is_complete()`
  - Checksum verification uses existing `transaction_id`
- [ ] Add `cleanup_stale_fragments()` method (remove incomplete transactions > 5 minutes)
- [ ] Add metrics collection (for monitoring UI):
  - `incomplete_transactions_count: usize`
  - `average_reassembly_time: Duration`
  - `fragments_per_transaction: HashMap<String, usize>`

#### 1.3 Confirmation Queue Implementation
- [ ] Create `src/queue/confirmation.rs` module
- [ ] Define `Confirmation` struct with:
  - `original_tx_id: [u8; 32]` (hash of original transaction)
  - `signature: String` (blockchain signature)
  - `status: ConfirmationStatus` enum (SUCCESS, FAILED)
  - `timestamp: u64`
  - `relay_count: u8` (hop count for mesh routing)
- [ ] Define `ConfirmationStatus` enum:
  - `Success { signature: String }`
  - `Failed { error: String }`
- [ ] Implement `ConfirmationQueue` struct:
  - `pending: VecDeque<Confirmation>`
- [ ] Add methods:
  - `add_confirmation(&mut self, tx_id: [u8; 32], signature: String)`
  - `pop_next(&mut self) -> Option<Confirmation>`
  - `len(&self) -> usize`
- [ ] Add fragmentation support for confirmations (use existing fragmenter)

#### 1.4 Retry Queue Implementation
- [ ] Create `src/queue/retry.rs` module
- [ ] Define `RetryItem` struct with:
  - `tx_bytes: Vec<u8>`
  - `tx_id: String`
  - `attempt_count: usize`
  - `last_error: String`
  - `next_retry_time: Instant`
  - `created_at: Instant`
  - `backoff_strategy: BackoffStrategy` enum
- [ ] Define `BackoffStrategy` enum:
  - `Exponential { base_seconds: u64 }`
  - `Linear { increment_seconds: u64 }`
  - `Fixed { interval_seconds: u64 }`
- [ ] Implement `RetryQueue` struct:
  - `items: BTreeMap<Instant, RetryItem>` (sorted by retry time)
  - `max_retries: usize`
  - `max_age: Duration` (24 hours)
- [ ] Add methods:
  - `add_for_retry(&mut self, tx_bytes: Vec<u8>, error: String)`
  - `pop_ready(&mut self) -> Option<RetryItem>`
  - `should_give_up(&self, item: &RetryItem) -> bool`
  - `next_retry_time(&self) -> Option<Instant>`
  - `len(&self) -> usize`
- [ ] Implement exponential backoff: 2s, 4s, 8s, 16s, 32s, 64s
- [ ] Add max retry limit (default: 5 attempts)
- [ ] Add max age limit (default: 24 hours)

#### 1.5 Queue Module Integration
- [ ] Create `src/queue/mod.rs` as queue module root
- [ ] Export all queue types publicly
- [ ] Add `QueueManager` struct to coordinate all queues:
  - `outbound: Arc<RwLock<OutboundQueue>>`
  - `reassembly: Arc<RwLock<ReassemblyBuffer>>`
  - `confirmations: Arc<RwLock<ConfirmationQueue>>`
  - `retries: Arc<RwLock<RetryQueue>>`
  - `received: Arc<RwLock<VecDeque<ReceivedTransaction>>>` (existing)
- [ ] Add `QueueManager::new()` constructor
- [ ] Add metrics methods:
  - `get_all_queue_sizes() -> QueueMetrics`
  - `get_queue_health() -> HealthStatus`

---

### Phase 2: FFI Integration (Rust → Android)

#### 2.1 Add Queue FFI Functions
- [ ] Add to `src/ffi/android.rs`:
  - `push_outbound_transaction()` - Add tx to outbound queue
  - `pop_outbound_transaction()` - Get next tx to transmit
  - `get_outbound_queue_size()` - Get queue size
  - `add_to_retry_queue()` - Add failed tx for retry
  - `pop_ready_retry()` - Get next ready retry item
  - `get_retry_queue_size()` - Get retry queue size
  - `queue_confirmation()` - Add confirmation to relay queue
  - `pop_confirmation()` - Get next confirmation to relay
  - `get_confirmation_queue_size()` - Get confirmation queue size
  - `cleanup_stale_fragments()` - Remove old fragments
  - `get_queue_metrics()` - Get all queue metrics
- [ ] Add to `src/ffi/types.rs`:
  - `OutboundTransactionFFI` struct
  - `RetryItemFFI` struct
  - `ConfirmationFFI` struct
  - `QueueMetricsFFI` struct
- [ ] Implement JSON serialization for all FFI types
- [ ] Add error handling for queue operations
- [ ] Add logging for queue operations

#### 2.2 Update PolliNetSDK (Rust)
- [ ] Add `queue_manager: Arc<QueueManager>` field to `PolliNetSDK`
- [ ] Initialize `QueueManager` in `new()` and `new_with_rpc()`
- [ ] Add public queue methods:
  - `pub async fn push_outbound_transaction()`
  - `pub async fn pop_outbound_transaction()`
  - `pub async fn add_to_retry_queue()`
  - `pub async fn pop_ready_retry()`
  - `pub async fn queue_confirmation()`
  - `pub async fn pop_confirmation()`
  - `pub async fn get_queue_metrics()`
- [ ] Integrate with existing `pushInbound()` for reassembly
- [ ] Add queue persistence (save/load from disk)

---

### Phase 3: Android SDK Integration (Kotlin)

#### 3.1 Update PolliNetFFI.kt
- [ ] Add JNI declarations for new FFI functions:
  - `external fun pushOutboundTransaction(handle: Long, txBytes: ByteArray, priority: Int): String`
  - `external fun popOutboundTransaction(handle: Long): String`
  - `external fun getOutboundQueueSize(handle: Long): String`
  - `external fun addToRetryQueue(handle: Long, txBytes: ByteArray, error: String): String`
  - `external fun popReadyRetry(handle: Long): String`
  - `external fun getRetryQueueSize(handle: Long): String`
  - `external fun queueConfirmation(handle: Long, txId: String, signature: String): String`
  - `external fun popConfirmation(handle: Long): String`
  - `external fun getConfirmationQueueSize(handle: Long): String`
  - `external fun cleanupStaleFragments(handle: Long): String`
  - `external fun getQueueMetrics(handle: Long): String`

#### 3.2 Update PolliNetSDK.kt
- [ ] Add data classes:
  - `data class OutboundTransaction`
  - `data class RetryItem`
  - `data class Confirmation`
  - `data class QueueMetrics`
  - `enum class Priority { HIGH, NORMAL, LOW }`
- [ ] Add suspend functions:
  - `suspend fun pushOutboundTransaction(txBytes: ByteArray, priority: Priority): Result<Unit>`
  - `suspend fun popOutboundTransaction(): Result<OutboundTransaction?>`
  - `suspend fun getOutboundQueueSize(): Result<Int>`
  - `suspend fun addToRetryQueue(txBytes: ByteArray, error: String): Result<Unit>`
  - `suspend fun popReadyRetry(): Result<RetryItem?>`
  - `suspend fun getRetryQueueSize(): Result<Int>`
  - `suspend fun queueConfirmation(txId: String, signature: String): Result<Unit>`
  - `suspend fun popConfirmation(): Result<Confirmation?>`
  - `suspend fun getConfirmationQueueSize(): Result<Int>`
  - `suspend fun cleanupStaleFragments(): Result<Unit>`
  - `suspend fun getQueueMetrics(): Result<QueueMetrics>`
- [ ] Add JSON parsing for all new response types
- [ ] Add error handling and logging

#### 3.3 Update queueSignedTransaction() Method
- [ ] Modify `BleService.kt::queueSignedTransaction()` to use new outbound queue
- [ ] Add priority parameter (default: NORMAL)
- [ ] Change flow:
  1. Fragment transaction
  2. Push to outbound queue with priority
  3. Return fragment count
- [ ] Remove direct sending logic (will be handled by listener)

---

### Phase 4: Event-Driven Worker Implementation (Kotlin) - Battery-Optimized

**⚠️ CRITICAL ARCHITECTURE DECISION:**
- ❌ **NO multiple polling listeners** (4-5 separate coroutine loops)
- ✅ **Single event-driven worker** using Kotlin Channels
- ✅ **WorkManager** for scheduled tasks (optional but recommended)
- 🎯 **Goal:** < 10 CPU wake-ups/minute when idle (vs 150 with polling)

```
┌─────────────────────────────────────────────────────────────┐
│                    OLD APPROACH (DON'T DO THIS)             │
├─────────────────────────────────────────────────────────────┤
│  Loop 1: while(true) { checkOutbound(); delay(2s) }        │
│  Loop 2: while(true) { checkReceived(); delay(2s) }        │
│  Loop 3: while(true) { checkRetry(); delay(2s) }           │
│  Loop 4: while(true) { checkConfirm(); delay(2s) }         │
│  Loop 5: while(true) { cleanup(); delay(5min) }            │
│                                                              │
│  Result: 150 wake-ups/min, 5% battery/hour ❌               │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                   NEW APPROACH (IMPLEMENT THIS)             │
├─────────────────────────────────────────────────────────────┤
│  workChannel = Channel<WorkEvent>()                         │
│                                                              │
│  Single Worker:                                             │
│    while(true) {                                            │
│      event = workChannel.receive(timeout=30s)              │
│      when(event) {                                          │
│        OutboundReady -> processOutbound()                   │
│        ReceivedReady -> processReceived()                   │
│        RetryReady -> processRetry()                         │
│        ConfirmationReady -> processConfirmation()           │
│        timeout -> fallbackCheck()                           │
│      }                                                       │
│    }                                                         │
│                                                              │
│  + WorkManager for retries (every 15min)                    │
│  + WorkManager for cleanup (every 30min)                    │
│                                                              │
│  Result: 5 wake-ups/min, 0.8% battery/hour ✅               │
│  Improvement: 85% battery savings! 🎉                       │
└─────────────────────────────────────────────────────────────┘
```

#### 4.1 Event-Driven Infrastructure Setup
- [ ] Create event channel system in `BleService.kt`:
  // Single channel for all work events (replaces 4-5 polling loops!)
  private val workChannel = Channel<WorkEvent>(Channel.UNLIMITED)
  
  sealed class WorkEvent {
      object OutboundReady : WorkEvent()
      object ReceivedReady : WorkEvent()
      object RetryReady : WorkEvent()
      object ConfirmationReady : WorkEvent()
      object CleanupNeeded : WorkEvent()
  }
  - [ ] No polling loops - pure event-driven!
- [ ] Events triggered by actions (queue operations, BLE receives, network changes)
- [ ] Zero CPU usage when no work to do

#### 4.2 Single Unified Worker (Replaces All Listeners)
- [ ] Create `startUnifiedEventWorker()` in `BleService.kt`:
  - **One coroutine instead of 4-5 separate pollers**
  - Uses `select { }` to multiplex all event types
  - Processes events immediately when they arrive
  - Falls back to checking received queue every 30 seconds (only fallback needed)
- [ ] Implementation structure:
  private fun startUnifiedEventWorker() {
      serviceScope.launch {
          var lastCleanup = System.currentTimeMillis()
          
          while (isActive) {
              // Wait for ANY event OR 30-second timeout
              val event = withTimeoutOrNull(30_000) {
                  workChannel.receive()
              }
              
              when (event) {
                  WorkEvent.OutboundReady -> processOutboundQueue()
                  WorkEvent.ReceivedReady -> processReceivedQueue()
                  WorkEvent.RetryReady -> processRetryQueue()
                  WorkEvent.ConfirmationReady -> processConfirmationQueue()
                  WorkEvent.CleanupNeeded -> processCleanup()
                  null -> {
                      // Timeout - check received queue as fallback
                      processReceivedQueue()
                      
                      // Periodic cleanup check (every 5 minutes)
                      if (System.currentTimeMillis() - lastCleanup > 300_000) {
                          processCleanup()
                          lastCleanup = System.currentTimeMillis()
                      }
                  }
              }
          }
      }
  }
  - [ ] Add lifecycle management (start/stop with service)
- [ ] Add error handling and recovery
- [ ] Add metrics: events processed, average latency, wake-ups per minute

#### 4.3 Outbound Queue Event Integration
- [ ] Modify `queueSignedTransaction()` to trigger event:
  suspend fun queueSignedTransaction(txBytes: ByteArray): Result<Int> {
      // Fragment and queue
      val result = sdk?.pushOutboundTransaction(txBytes, Priority.NORMAL)
      
      // Trigger immediate processing (no polling wait!)
      result?.onSuccess {
          workChannel.trySend(WorkEvent.OutboundReady)
      }
      
      return result ?: Result.failure(Exception("SDK not initialized"))
  }
  - [ ] Implement `processOutboundQueue()`:
  - Check connection state (only process if CONNECTED)
  - Pop transaction from outbound queue (priority-based)
  - Send fragments over BLE with delays
  - **Batch processing:** Process up to 10 transactions per event
  - Log transmission progress
  - Re-trigger event if more work remains: `workChannel.trySend(WorkEvent.OutboundReady)`
- [ ] Add error handling and retry logic
- [ ] Add metrics tracking (fragments sent, success rate)
- [ ] **NO separate polling loop!**

#### 4.4 Fragment Reassembly Event Integration
- [ ] Enhance `handleReceivedData()` to trigger event:
  
  private suspend fun handleReceivedData(data: ByteArray) {
      val result = sdk?.pushInbound(data)
      
      result?.onSuccess {
          // Check if transaction completed
          val queueSize = sdk?.getReceivedQueueSize()?.getOrNull() ?: 0
          if (queueSize > 0) {
              // Transaction ready! Trigger immediate processing
              workChannel.trySend(WorkEvent.ReceivedReady)
          }
      }?.onFailure { e ->
          appendLog("❌ Fragment processing error: ${e.message}")
      }
  }
  - [ ] Implement `processReceivedQueue()`:
  - Check internet connectivity
  - Pop next received transaction
  - Submit to blockchain
  - On success: queue confirmation, trigger confirmation event
  - On failure: add to retry queue (WorkManager handles scheduling)
  - **Batch processing:** Process multiple if available
- [ ] Add better logging for fragment progress
- [ ] Add metrics tracking (fragments received, reassembly time)
- [ ] **NO separate polling loop!**

#### 4.5 Network State Event Integration
- [ ] Add connectivity change listener:
  private val networkCallback = object : ConnectivityManager.NetworkCallback() {
      override fun onAvailable(network: Network) {
          // Internet restored - process pending work!
          workChannel.trySend(WorkEvent.ReceivedReady)
          workChannel.trySend(WorkEvent.RetryReady)
      }
      
      override fun onLost(network: Network) {
          appendLog("📡 Internet lost - queuing mode activated")
      }
  }
  - [ ] Register callback in `onCreate()`:
  val connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
  val networkRequest = NetworkRequest.Builder()
      .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
      .build()
  connectivityManager.registerNetworkCallback(networkRequest, networkCallback)
  - [ ] Unregister in `onDestroy()`
- [ ] Immediate response to network changes (no polling delay!)

#### 4.6 Retry Queue - WorkManager Implementation (RECOMMENDED)
- [ ] **PRIMARY: WorkManager for scheduled retries** (battery-optimal)
  - [ ] Create `RetryWorker.kt` (extends `CoroutineWorker`):
    class RetryWorker(context: Context, params: WorkerParameters) 
        : CoroutineWorker(context, params) {
        
        override suspend fun doWork(): Result {
            val sdk = getPolliNetSDK() ?: return Result.failure()
            
            var processedCount = 0
            var successCount = 0
            
            // Process ALL ready retries in one batch
            while (true) {
                val retry = sdk.popReadyRetry().getOrNull() ?: break
                processedCount++
                
                val result = sdk.submitOfflineTransaction(
                    transactionBase64 = retry.txBase64,
                    verifyNonce = false
                )
                
                result.onSuccess { signature ->
                    successCount++
                    sdk.markTransactionSubmitted(retry.txBytes)
                    sdk.queueConfirmation(retry.txId, signature)
                }.onFailure { error ->
                    if (retry.attemptCount < 5) {
                        sdk.addToRetryQueue(retry.copy(
                            attemptCount = retry.attemptCount + 1
                        ))
                    }
                }
            }
            
            return Result.success(
                workDataOf(
                    "processed" to processedCount,
                    "succeeded" to successCount
                )
            )
        }
    }
      - [ ] Schedule periodic retry work in `initializeSdk()`:
    private fun scheduleRetryWorker() {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .setRequiresBatteryNotLow(true)
            .build()
        
        val retryWork = PeriodicWorkRequestBuilder<RetryWorker>(
            15, TimeUnit.MINUTES // Android minimum
        ).setConstraints(constraints)
         .setBackoffCriteria(
             BackoffPolicy.EXPONENTIAL,
             WorkRequest.MIN_BACKOFF_MILLIS,
             TimeUnit.MILLISECONDS
         ).build()
        
        WorkManager.getInstance(this)
            .enqueueUniquePeriodicWork(
                "pollinet_retry",
                ExistingPeriodicWorkPolicy.KEEP,
                retryWork
            )
    }
      - [ ] Add WorkManager dependency to `build.gradle`:
    implementation "androidx.work:work-runtime-ktx:2.9.0"
      - [ ] **Battery Impact:** 4 wake-ups/hour, Android-managed, Doze-friendly
  - [ ] Add metrics tracking to Worker result

- [ ] **FALLBACK: Event-driven retry** (if WorkManager unsuitable)
  - [ ] Implement `processRetryQueue()`:
    - Check if any retries are ready (based on `next_retry_time`)
    - Process ready retries
    - Schedule next check using AlarmManager if needed
  - [ ] Only use if WorkManager can't be used for some reason

#### 4.7 Confirmation Queue Event Integration
- [ ] Queue confirmation triggers event:
  // After successful transaction submission
  sdk?.queueConfirmation(txId, signature)?.onSuccess {
      workChannel.trySend(WorkEvent.ConfirmationReady)
  }
  - [ ] Implement `processConfirmationQueue()`:
  - Check connection state (only send if CONNECTED)
  - Pop next confirmation from queue
  - Fragment if needed
  - Send over BLE to origin device
  - **Batch processing:** Process multiple confirmations
  - Log relay progress
- [ ] Add hop count limit (max: 5 hops)
- [ ] Add TTL check (max: 1 hour)
- [ ] Add metrics tracking
- [ ] **NO separate polling loop!**

#### 4.8 Cleanup - WorkManager Implementation (RECOMMENDED)
- [ ] **PRIMARY: WorkManager for scheduled cleanup** (battery-optimal)
  - [ ] Create `CleanupWorker.kt`:
    class CleanupWorker(context: Context, params: WorkerParameters) 
        : CoroutineWorker(context, params) {
        
        override suspend fun doWork(): Result {
            val sdk = getPolliNetSDK() ?: return Result.failure()
            
            val cleanupResult = sdk.cleanupStaleFragments().getOrNull()
            
            return Result.success(
                workDataOf(
                    "fragments_cleaned" to (cleanupResult?.fragmentsCleaned ?: 0),
                    "memory_freed" to (cleanupResult?.memoryFreed ?: 0)
                )
            )
        }
    }
      - [ ] Schedule periodic cleanup:
    val cleanupWork = PeriodicWorkRequestBuilder<CleanupWorker>(
        30, TimeUnit.MINUTES
    ).build()
    
    WorkManager.getInstance(this)
        .enqueueUniquePeriodicWork(
            "pollinet_cleanup",
            ExistingPeriodicWorkPolicy.KEEP,
            cleanupWork
        )
      - [ ] **Battery Impact:** 2 wake-ups/hour, Android-managed

- [ ] **FALLBACK: Piggyback on unified worker timeout**
  - [ ] Already implemented in unified worker (every 5 minutes)
  - [ ] Minimal battery impact (reuses existing wake-ups)

#### 4.9 Battery Optimization Features
- [ ] Add auto-disconnect on BLE idle:
  private var lastBleActivity = System.currentTimeMillis()
  
  private fun scheduleAutoDisconnect() {
      serviceScope.launch {
          delay(30_000) // 30 seconds
          if (System.currentTimeMillis() - lastBleActivity > 30_000) {
              appendLog("💤 BLE idle - auto-disconnecting to save battery")
              closeGattConnection()
          }
      }
  }
  
  // Reconnect when OutboundReady event arrives
  - [ ] Add batch processing optimization:
  private suspend fun processOutboundQueue() {
      // Process up to 10 transactions in one wake-up
      repeat(10) {
          val tx = sdk?.popOutboundTransaction()?.getOrNull() ?: return
          transmitTransaction(tx)
      }
      
      // Check if more work remains
      if (sdk?.getOutboundQueueSize()?.getOrNull() ?: 0 > 0) {
          workChannel.trySend(WorkEvent.OutboundReady) // Re-trigger
      }
  }
  - [ ] Add adaptive behavior based on queue size:
  val batchSize = when {
      queueSize > 50 -> 20  // Aggressive when backlog
      queueSize > 10 -> 10  // Normal processing
      else -> 5             // Conservative when near empty
  }
  - [ ] Monitor and log battery impact:
  private fun logBatteryMetrics() {
      val batteryManager = getSystemService(Context.BATTERY_SERVICE) as BatteryManager
      val batteryPct = batteryManager.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
      val currentNow = batteryManager.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW)
      
      appendLog("🔋 Battery: $batteryPct%, Current: ${currentNow/1000}mA")
  }
  #### 4.10 Testing & Validation
- [ ] Test event-driven behavior:
  - Queue transaction → immediate event → processes within 100ms
  - No queued transaction → zero CPU wake-ups
  - Multiple rapid events → batched processing
- [ ] Test battery efficiency:
  - Idle state: < 10 wake-ups/minute
  - Active state: responsive (< 1 second latency)
  - Doze mode: only WorkManager tasks execute
- [ ] Compare with polling baseline:
  - Measure: Event-driven vs continuous polling
  - Expected: 80-90% battery improvement
- [ ] Use Android Profiler:
  - Track CPU wake-ups
  - Track energy usage
  - Verify no wake locks held when idle
- [ ] Test WorkManager:
  - Verify retries execute every 15 minutes
  - Verify respects battery constraints
  - Verify works in Doze/App Standby

#### 4.11 Migration from Polling (If Applicable)
- [ ] Remove OLD polling listeners if they exist:
  - ❌ Delete `startOutboundTransmitter()` (separate loop)
  - ❌ Delete `startRetryHandler()` (separate loop)
  - ❌ Delete `startConfirmationRelay()` (separate loop)
  - ✅ Keep `startAutoSubmitLoop()` but extend timeout to 30s
- [ ] Replace with single `startUnifiedEventWorker()`
- [ ] Verify no regressions in functionality
- [ ] Measure and document battery improvement

---

### Phase 4.5: Battery Efficiency Validation

#### Success Criteria - Battery Performance
- [ ] **Idle State (no transactions):**
  - Target: < 10 CPU wake-ups per minute
  - Target: < 1% battery drain per hour
  - Target: Zero wake-ups in Doze mode (except WorkManager maintenance)
  
- [ ] **Active State (processing transactions):**
  - Target: < 1 second latency from event to processing
  - Target: < 3% battery drain per hour
  - Target: Responsive to user actions
  
- [ ] **Improvement vs Polling:**
  - Baseline (5 listeners × 2s polling): ~150 wake-ups/min, ~5% drain/hour
  - Event-driven: ~5 wake-ups/min, ~0.8% drain/hour
  - Expected improvement: **85%+ battery savings**

#### Battery Monitoring Dashboard
- [ ] Add battery metrics to `DiagnosticsScreen.kt`:
  ```kotlin
  Card {
      Text("🔋 Battery Metrics")
      Text("Current drain: ${batteryDrainRate}%/hour")
      Text("Wake-ups (last min): ${wakeUpsPerMinute}")
      Text("Last event: ${lastEventTime}")
      Text("Event queue size: ${eventChannelSize}")
      Text("WorkManager jobs: ${workManagerJobCount}")
  }
  ```
- [ ] Add battery health warnings:
  - Warn if drain > 3% per hour
  - Warn if wake-ups > 30 per minute
  - Suggest switching to AGGRESSIVE mode if battery < 20%

#### Configuration - Battery Modes
- [ ] Add battery optimization profiles:
  ```kotlin
  enum class BatteryMode {
      AGGRESSIVE,   // WorkManager only, long timeouts, aggressive auto-disconnect
      BALANCED,     // Event-driven + WorkManager, standard timeouts
      PERFORMANCE   // Event-driven, short timeouts, no auto-disconnect
  }
  ```
- [ ] Auto-switch based on battery level:
  - < 20% battery: AGGRESSIVE
  - 20-50% battery: BALANCED
  - > 50% battery: PERFORMANCE
- [ ] User override in settings

---

### Phase 5: Queue Persistence

#### 5.1 Disk Persistence
- [ ] Create `src/storage/queue_storage.rs` module
- [ ] Implement save/load for each queue:
  - `save_outbound_queue(path: &str) -> Result<()>`
  - `load_outbound_queue(path: &str) -> Result<OutboundQueue>`
  - `save_retry_queue(path: &str) -> Result<()>`
  - `load_retry_queue(path: &str) -> Result<RetryQueue>`
  - `save_confirmation_queue(path: &str) -> Result<()>`
  - `load_confirmation_queue(path: &str) -> Result<ConfirmationQueue>`
- [ ] Use JSON or bincode serialization
- [ ] Add atomic write (write to temp file, then rename)
- [ ] Add file locking to prevent corruption

#### 5.2 Auto-save on Changes
- [ ] Add auto-save trigger after queue modifications
- [ ] Add debouncing (save at most once per 5 seconds)
- [ ] Add save on app background/termination

#### 5.3 Load on Initialization
- [ ] Load all queues on SDK initialization
- [ ] Handle missing/corrupted files gracefully
- [ ] Log queue restoration status

---

### Phase 6: Metrics & Monitoring

#### 6.1 Queue Metrics System
- [ ] Create `src/metrics/queue_metrics.rs` module
- [ ] Define `QueueMetrics` struct:
  - `outbound_size: usize`
  - `outbound_high_priority: usize`
  - `outbound_normal_priority: usize`
  - `outbound_low_priority: usize`
  - `reassembly_incomplete: usize`
  - `reassembly_avg_time_ms: u64`
  - `retry_size: usize`
  - `retry_avg_attempts: f32`
  - `confirmation_size: usize`
  - `received_size: usize`
- [ ] Add metrics collection methods
- [ ] Add metrics export (JSON format)
- [ ] Add metrics logging

#### 6.2 Android UI Integration
- [ ] Add queue metrics display to `DiagnosticsScreen.kt`
- [ ] Show real-time queue sizes
- [ ] Show success/failure rates
- [ ] Show retry statistics
- [ ] Add "Clear All Queues" button for testing

#### 6.3 Health Status
- [ ] Implement queue health checks:
  - Warn if outbound queue > 100 items
  - Warn if retry queue > 50 items
  - Warn if reassembly has incomplete items > 30 minutes old
- [ ] Add health status to metrics
- [ ] Add health status notifications (optional)

---

### Phase 7: Testing & Validation

#### 7.1 Unit Tests (Rust)
- [ ] Test `OutboundQueue`:
  - Priority ordering
  - Deduplication
  - Thread safety
- [ ] Test `ReassemblyBuffer`:
  - Fragment insertion (in-order and out-of-order)
  - Completion detection
  - Checksum verification
  - Stale cleanup
- [ ] Test `RetryQueue`:
  - Exponential backoff calculation
  - Ready item selection
  - Give-up logic
- [ ] Test `ConfirmationQueue`:
  - FIFO ordering
  - Hop count tracking

#### 7.2 Integration Tests (Android)
- [ ] Test full transaction flow:
  1. Sign transaction
  2. Queue in outbound
  3. Transmit over BLE
  4. Receive and reassemble
  5. Auto-submit
  6. Queue confirmation
  7. Relay confirmation back
- [ ] Test retry flow:
  1. Simulate submission failure
  2. Verify added to retry queue
  3. Wait for backoff
  4. Verify retry attempt
- [ ] Test persistence:
  1. Queue transactions
  2. Kill app
  3. Restart app
  4. Verify queues restored

#### 7.3 End-to-End Tests
- [ ] Test with 2 devices:
  - Device A: Sign and send transaction
  - Device B: Receive, submit, and confirm
  - Device A: Receive confirmation
- [ ] Test mesh relay (3+ devices):
  - A → B → C transaction relay
  - C → B → A confirmation relay
- [ ] Test under poor connectivity:
  - Intermittent BLE connection
  - No internet (queue builds up)
  - Internet restored (queue drains)

#### 7.4 Performance Tests
- [ ] Measure queue operation latency:
  - Push operation < 1ms
  - Pop operation < 1ms
  - Reassembly < 10ms
- [ ] Measure memory usage:
  - 1000 queued transactions < 50MB
- [ ] Measure throughput:
  - > 10 transactions/second transmission
  - > 20 fragments/second reception

---

### Phase 8: Documentation & Polish

#### 8.1 Code Documentation
- [ ] Add rustdoc comments for all queue types
- [ ] Add KDoc comments for all Kotlin types
- [ ] Add architecture diagrams
- [ ] Add sequence diagrams for each flow

#### 8.2 User Documentation
- [ ] Update TESTING.md with queue testing instructions
- [ ] Update SETUP.md with queue configuration
- [ ] Add QUEUE_ARCHITECTURE.md explaining the system
- [ ] Add troubleshooting guide

#### 8.3 Performance Tuning
- [ ] Tune queue sizes (defaults and max limits)
- [ ] Tune retry backoff parameters
- [ ] Tune cleanup intervals
- [ ] Tune listener polling intervals

#### 8.4 Configuration (Battery-Aware)
- [ ] Add queue configuration options:
  - Max outbound queue size
  - Max retry attempts
  - Retry backoff strategy
  - Stale fragment timeout
  - Confirmation TTL
  - Auto-save interval
  - **Battery optimization mode:** AGGRESSIVE, BALANCED, PERFORMANCE
    - AGGRESSIVE: WorkManager only, 30s timeouts, auto-disconnect after 15s
    - BALANCED: Event-driven + WorkManager, 30s timeouts, auto-disconnect after 30s
    - PERFORMANCE: Event-driven + 5s fallback, no auto-disconnect
- [ ] Add to `SdkConfig`
- [ ] Document all configuration options
- [ ] Add runtime battery mode switching based on battery level:
  - < 20% battery: Switch to AGGRESSIVE
  - < 50% battery: Switch to BALANCED
  - > 50% battery: PERFORMANCE

---

## 📊 Success Criteria

### Functional Requirements
- ✅ All transactions queued for BLE relay are transmitted in priority order
- ✅ All received fragments are correctly reassembled
- ✅ Failed submissions are automatically retried with exponential backoff
- ✅ Confirmations are relayed back to origin devices
- ✅ Queues persist across app restarts

### Performance Requirements
- ✅ Queue operations complete in < 1ms
- ✅ Reassembly completes in < 10ms
- ✅ System handles 1000+ queued transactions
- ✅ Memory usage < 50MB for full queues
- ✅ Throughput > 10 tx/sec

### Reliability Requirements
- ✅ No data loss on app crash
- ✅ No duplicate submissions
- ✅ Stale fragments cleaned up automatically
- ✅ Failed transactions eventually succeed or give up gracefully

---

## 🔧 Technical Debt & Future Improvements

### Phase 9: Advanced Features (Future)
- [ ] Add priority auto-adjustment based on age
- [ ] Add queue compression for large queues
- [ ] Add queue sharing across multiple SDK instances
- [ ] Add distributed queue coordination (mesh-wide)
- [ ] Add machine learning for optimal retry strategy
- [ ] Add queue analytics dashboard
- [ ] Add remote queue monitoring
- [ ] Add queue backup to cloud

### Phase 10: Optimizations (Future)
- [ ] Replace `VecDeque` with lock-free concurrent queue
- [ ] Use bloom filters for faster deduplication
- [ ] Add queue batching (process multiple items at once)
- [ ] Add adaptive polling intervals (faster when busy)
- [ ] Add queue compaction (remove completed items periodically)

---

## 📅 Implementation Timeline

### Week 1: Foundation (Phase 1-2)
- Days 1-2: Core queue data structures (Rust)
- Days 3-4: FFI integration
- Day 5: Testing and debugging

### Week 2: Integration (Phase 3-4)
- Days 1-2: Android SDK integration
- Days 3-4: Listener implementation
- Day 5: Testing and debugging

### Week 3: Persistence & Metrics (Phase 5-6)
- Days 1-2: Queue persistence
- Days 3-4: Metrics and monitoring
- Day 5: Testing and debugging

### Week 4: Testing & Documentation (Phase 7-8)
- Days 1-2: Comprehensive testing
- Days 3-4: Documentation
- Day 5: Performance tuning and polish

---

## 📝 Notes

### Current System State
- ✅ Basic reassembly buffer exists (`TransactionCache`)
- ✅ Received transaction queue exists (accessed via FFI)
- ✅ Autonomous submission loop exists
- ✅ Fragment identification uses SHA-256 hash (robust cross-device matching)
- ✅ Fragment structure supports multi-device relay (transaction_id in every fragment)
- ✅ Fragmentation system accounts for BLE MTU overhead (~40 bytes per fragment)
- ⚠️ No dedicated outbound queue (currently sends immediately)
- ⚠️ No retry queue (currently simple requeue)
- ⚠️ No confirmation relay
- ⚠️ No queue persistence
- ⚠️ Multiple polling listeners (needs event-driven refactor for battery)

### Key Design Decisions
1. **Priority-based outbound**: User transactions > relay transactions
2. **Exponential backoff**: Prevents network overload
3. **Persistent queues**: Survive app restarts
4. **SHA-256 for deduplication**: Cryptographically secure, fast
5. **BTreeMap for retry queue**: Efficient time-based ordering
6. ⭐ **Event-driven architecture**: Single worker replaces 4-5 polling loops
7. ⭐ **Kotlin Channels for events**: Zero CPU usage when idle (vs 150 wake-ups/min)
8. ⭐ **WorkManager for scheduled tasks**: Android-managed, Doze-friendly retries
9. **Batch processing**: 10+ items per wake-up reduces overhead
10. **Auto-disconnect idle BLE**: Saves radio power after 30s inactivity
11. **Network callback listener**: Immediate response to connectivity changes
12. **Single 30s timeout**: Only fallback polling needed (vs 2s continuous)
13. **SHA-256 fragment matching**: Enables cross-device fragment reassembly

### Architecture Rationale

**Why Event-Driven instead of Multiple Pollers?**
- ❌ **OLD:** 5 separate coroutines polling every 2 seconds = 150 CPU wake-ups/minute
- ✅ **NEW:** 1 event-driven worker + Channels = 0-5 wake-ups/minute when idle
- 💰 **Battery Savings:** 85-90% reduction in power consumption
- 📱 **Android Compliance:** Doze mode compatible, App Standby friendly
- ⚡ **Responsiveness:** Events process in < 100ms (vs up to 2s polling delay)

**Why SHA-256 for Fragment Identification?**
- ✅ **Globally unique**: Collision probability ~1 in 2^256
- ✅ **Self-describing**: Each fragment knows which transaction it belongs to
- ✅ **Cross-device compatible**: Fragments can arrive from multiple relay paths
- ✅ **Tamper detection**: Hash verification ensures data integrity
- ✅ **Already implemented**: No changes needed to BLE protocol

### Dependencies
- Rust: `tokio`, `serde`, `sha2`, `hex`, `tracing`
- Android: `kotlinx-coroutines`, `kotlinx-serialization`
- Existing: PolliNet fragmentation system, BLE service

---

## 🎯 Current Priority & Implementation Order

**Phase 1 (Week 1):** Core Queue Data Structures
1. **START HERE:** Phase 1.1 - Create `src/queue/outbound.rs` module
2. Phase 1.2 - Enhance reassembly buffer (clarify fragment matching)
3. Phase 1.3 - Confirmation queue
4. Phase 1.4 - Retry queue with exponential backoff
5. Phase 1.5 - QueueManager integration

**Phase 2-3 (Week 2):** FFI & Android Integration
- Connect Rust queues to Android via FFI
- Update PolliNetSDK.kt with queue operations
- Prepare for event-driven worker

**Phase 4 (Week 2-3):** Event-Driven Worker (CRITICAL FOR BATTERY)
- Replace polling loops with single event-driven worker
- Implement WorkManager for scheduled tasks
- Add battery optimization features
- **Expected Result:** 85%+ battery improvement

**Phase 5-8 (Week 3-4):** Persistence, Metrics, Testing, Documentation

---

## 🔋 Battery Optimization Summary

### What's Changing
- ❌ **REMOVING:** 4-5 separate polling coroutines (150 wake-ups/min)
- ✅ **ADDING:** Single event-driven worker with Kotlin Channels (0-5 wake-ups/min idle)
- ✅ **ADDING:** WorkManager for retry/cleanup (Android-managed, Doze-friendly)

### Expected Impact
- **Idle battery drain:** 5% → 0.8% per hour (84% improvement)
- **CPU wake-ups:** 150 → 5 per minute (97% improvement)
- **Responsiveness:** 2s max delay → < 100ms (instant)
- **Doze compatibility:** Broken → Fully compatible

### Configuration
Users can choose battery mode based on needs:
- **AGGRESSIVE:** Maximum battery savings, 15-30 min retry intervals
- **BALANCED:** Good battery + responsiveness (recommended)
- **PERFORMANCE:** Instant response, higher battery usage

---

**Last Updated:** December 22, 2025 (Reworked with battery optimizations)  
**Next Review:** After Phase 1 completion

---

## 📚 Additional Documentation

For more details on specific topics, see:
- **Battery Optimization:** See Phase 4 & Phase 4.5
- **Fragment Matching:** See Phase 1.2 (SHA-256 identification)
- **Event-Driven Architecture:** See Phase 4 visual diagram
- **Queue Persistence:** See Phase 5
- **Testing Strategy:** See Phase 7
- **Configuration Options:** See Phase 8.4

