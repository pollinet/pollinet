# BLE Mesh Testing Guide

## Overview

This guide covers testing the BLE mesh network at multiple levels: unit tests, integration tests, and real-device testing.

---

## 1. Rust Unit Tests

### Running Rust Tests

```bash
# Test all BLE modules
cd /Users/oghenekparoboreminokanju/pollinet
cargo test --lib ble:: --no-default-features --features android

# Test specific modules
cargo test --lib ble::fragmenter::     # Fragmentation
cargo test --lib ble::mesh::           # Mesh router
cargo test --lib ble::broadcaster::    # Broadcasting
cargo test --lib ble::peer_manager::   # Peer management

# Run with output
cargo test --lib ble:: --no-default-features --features android -- --nocapture
```

### What's Already Tested ✅

**Fragmenter (11 tests):**
- ✅ Small transactions (1 fragment)
- ✅ Large transactions (multiple fragments)
- ✅ Out-of-order reconstruction
- ✅ Missing fragments detection
- ✅ Hash verification
- ✅ Corruption detection

**Mesh Router (3 tests):**
- ✅ Header serialization
- ✅ Packet serialization
- ✅ Fragment reassembly

**Broadcaster (3 tests):**
- ✅ Peer status tracking
- ✅ Retry logic
- ✅ Broadcast info management

---

## 2. Android Instrumentation Tests

### Setup Test Environment

Create test file: `pollinet-android/pollinet-sdk/src/androidTest/java/xyz/pollinet/sdk/BleIntegrationTest.kt`

```kotlin
package xyz.pollinet.sdk

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import android.util.Base64

@RunWith(AndroidJUnit4::class)
class BleIntegrationTest {
    private lateinit var context: Context
    private lateinit var sdk: PolliNetSDK

    @Before
    fun setup() {
        context = InstrumentationRegistry.getInstrumentation().targetContext
        sdk = PolliNetSDK.initialize(
            rpcUrl = null, // No RPC needed for fragmentation tests
            storageDir = context.filesDir.absolutePath
        ).getOrThrow()
    }

    @Test
    fun testFragmentation() = runBlocking {
        // Create a mock transaction
        val txBytes = ByteArray(500) { it.toByte() }
        
        // Fragment it
        val result = sdk.fragmentTransaction(txBytes)
        
        assertTrue(result.isSuccess)
        val fragments = result.getOrThrow()
        assertTrue(fragments.isNotEmpty())
        
        // Reconstruct
        val reconstructResult = sdk.reconstructTransaction(fragments)
        assertTrue(reconstructResult.isSuccess)
        
        val reconstructedBase64 = reconstructResult.getOrThrow()
        val reconstructed = Base64.decode(reconstructedBase64, Base64.NO_WRAP)
        
        assertArrayEquals(txBytes, reconstructed)
    }

    @Test
    fun testFragmentationStats() = runBlocking {
        val txBytes = ByteArray(1000) { it.toByte() }
        
        val result = sdk.getFragmentationStats(txBytes)
        
        assertTrue(result.isSuccess)
        val stats = result.getOrThrow()
        
        assertEquals(1000, stats.originalSize)
        assertTrue(stats.fragmentCount > 0)
        assertTrue(stats.efficiency > 0)
    }

    @Test
    fun testBroadcastPreparation() = runBlocking {
        // Create a realistic transaction (350 bytes typical)
        val txBytes = ByteArray(350) { it.toByte() }
        
        // Prepare broadcast
        val result = sdk.prepareBroadcast(txBytes)
        
        assertTrue(result.isSuccess)
        val prep = result.getOrThrow()
        
        assertNotNull(prep.transactionId)
        assertTrue(prep.fragmentPackets.isNotEmpty())
        
        // Verify each packet has valid data
        for (packet in prep.fragmentPackets) {
            assertFalse(packet.packetBytes.isEmpty())
            assertTrue(packet.fragmentIndex < packet.totalFragments)
            
            // Decode packet bytes
            val packetBytes = Base64.decode(packet.packetBytes, Base64.NO_WRAP)
            assertTrue(packetBytes.isNotEmpty())
        }
    }
}
```

### Run Android Tests

```bash
cd /Users/oghenekparoboreminokanju/pollinet/pollinet-android

# Run all instrumentation tests
./gradlew :pollinet-sdk:connectedAndroidTest

# Run specific test
./gradlew :pollinet-sdk:connectedAndroidTest \
  --tests "xyz.pollinet.sdk.BleIntegrationTest.testFragmentation"
```

---

## 3. Manual Testing with One Device

### Test Fragmentation UI

Add to `DiagnosticsScreen.kt`:

```kotlin
@Composable
fun FragmentationTestCard(sdk: PolliNetSDK?) {
    var testStatus by remember { mutableStateOf("Ready") }
    var isLoading by remember { mutableStateOf(false) }
    
    StatusCard(
        title = "🧪 Fragmentation Test",
        content = {
            Column {
                Text(testStatus)
                
                Spacer(modifier = Modifier.height(8.dp))
                
                Button(
                    onClick = {
                        isLoading = true
                        CoroutineScope(Dispatchers.IO).launch {
                            try {
                                // Create test transaction
                                val txBytes = ByteArray(500) { it.toByte() }
                                
                                testStatus = "Fragmenting..."
                                val fragments = sdk!!.fragmentTransaction(txBytes).getOrThrow()
                                
                                testStatus = "Created ${fragments.size} fragments"
                                delay(1000)
                                
                                testStatus = "Reconstructing..."
                                val reconstructed = sdk.reconstructTransaction(fragments).getOrThrow()
                                
                                val reconstructedBytes = Base64.decode(reconstructed, Base64.NO_WRAP)
                                
                                if (reconstructedBytes.contentEquals(txBytes)) {
                                    testStatus = "✅ Test PASSED!"
                                } else {
                                    testStatus = "❌ Reconstruction mismatch"
                                }
                            } catch (e: Exception) {
                                testStatus = "❌ Error: ${e.message}"
                            } finally {
                                isLoading = false
                            }
                        }
                    },
                    enabled = !isLoading && sdk != null
                ) {
                    Text("Run Test")
                }
            }
        }
    )
}
```

**Test Steps:**
1. Launch app
2. Tap "Run Test" button
3. Verify status shows "✅ Test PASSED!"

---

## 4. Multi-Device Testing

### Setup (Minimum 2 Devices)

**Device A (Sender):**
- Enable BLE permissions
- Start advertising
- Prepare to send transaction

**Device B (Receiver):**
- Enable BLE permissions
- Start scanning
- Connect to Device A

### Test Broadcast Transmission

Add to `DiagnosticsScreen.kt`:

```kotlin
@Composable
fun BroadcastTestCard(
    sdk: PolliNetSDK?,
    bleService: BleService?,
    activityResultSender: ActivityResultSender
) {
    var testStatus by remember { mutableStateOf("Ready") }
    var broadcastId by remember { mutableStateOf<String?>(null) }
    
    StatusCard(
        title = "📡 Broadcast Test",
        content = {
            Column {
                Text("Status: $testStatus")
                
                if (broadcastId != null) {
                    Text("TX ID: ${broadcastId!!.take(16)}...", 
                        style = MaterialTheme.typography.bodySmall)
                }
                
                Spacer(modifier = Modifier.height(8.dp))
                
                Button(
                    onClick = {
                        CoroutineScope(Dispatchers.IO).launch {
                            try {
                                testStatus = "Creating transaction..."
                                
                                // Create a small test transaction
                                val txBytes = ByteArray(350) { it.toByte() }
                                
                                testStatus = "Preparing broadcast..."
                                val prep = sdk!!.prepareBroadcast(txBytes).getOrThrow()
                                
                                broadcastId = prep.transactionId
                                testStatus = "Broadcasting ${prep.fragmentPackets.size} fragments..."
                                
                                // Get connected peers
                                val peers = bleService?.getConnectedPeers() ?: emptyList()
                                
                                if (peers.isEmpty()) {
                                    testStatus = "⚠️ No peers connected"
                                    return@launch
                                }
                                
                                // Send each fragment to all peers
                                var sentCount = 0
                                for (packet in prep.fragmentPackets) {
                                    val packetBytes = Base64.decode(
                                        packet.packetBytes,
                                        Base64.NO_WRAP
                                    )
                                    
                                    for (peer in peers) {
                                        bleService.sendToPeer(peer.id, packetBytes)
                                        sentCount++
                                    }
                                    
                                    delay(50) // Small delay between fragments
                                }
                                
                                testStatus = "✅ Sent $sentCount packets to ${peers.size} peer(s)"
                                
                            } catch (e: Exception) {
                                testStatus = "❌ Error: ${e.message}"
                                android.util.Log.e("BroadcastTest", "Error", e)
                            }
                        }
                    },
                    enabled = sdk != null && bleService != null
                ) {
                    Text("Broadcast Test Transaction")
                }
            }
        }
    )
}
```

### Test Procedure

**Phase 1: Connection**
1. Device A: Start advertising
2. Device B: Start scanning
3. Device B: Connect to Device A
4. Verify connection in diagnostics screen

**Phase 2: Broadcast**
1. Device A: Tap "Broadcast Test Transaction"
2. Device B: Monitor logs for incoming fragments
3. Device B: Verify transaction reconstructed
4. Check both devices show success

**Phase 3: Multi-hop (3+ devices)**
1. Device A → Device B → Device C chain
2. Device A broadcasts transaction
3. Device B receives and forwards
4. Device C receives from B
5. Verify all devices reconstruct correctly

---

## 5. Debugging Tools

### Enable Detailed Logging

**Rust Side:**
```rust
// Already configured in android.rs
tracing::info!("Message with context");
tracing::debug!("Detailed debug info");
tracing::warn!("Warning message");
```

**Android Logcat Filters:**
```bash
# Rust logs
adb logcat -s "PolliNet-Rust:*"

# BLE specific
adb logcat -s "PolliNet-Rust:*" | grep -E "fragment|broadcast|mesh"

# All PolliNet logs
adb logcat | grep -i pollinet

# Clear and follow
adb logcat -c && adb logcat -s "PolliNet-Rust:*" "BleService:*" "MeshRouter:*"
```

### Diagnostic Commands

Add to your app:

```kotlin
// Get mesh statistics
lifecycleScope.launch {
    val stats = sdk.getMeshStats().getOrNull()
    stats?.let {
        Log.d("Mesh", """
            Device ID: ${it.deviceId}
            Seen messages: ${it.seenMessages}
            Incomplete TXs: ${it.incompleteTransactions}
            Completed TXs: ${it.completedTransactions}
        """.trimIndent())
    }
}

// Get fragmentation stats
val txBytes = getTransaction()
val stats = sdk.getFragmentationStats(txBytes).getOrThrow()
Log.d("Fragment", """
    Original: ${stats.originalSize} bytes
    Fragments: ${stats.fragmentCount}
    Efficiency: ${stats.efficiency}%
""".trimIndent())
```

---

## 6. Test Scenarios

### Scenario 1: Single Fragment Transaction
**Goal:** Verify small transactions work
```
1. Create 200-byte transaction
2. Fragment (should be 1 fragment)
3. Broadcast to peer
4. Verify received and reconstructed
Expected: 100% success rate
```

### Scenario 2: Multi-Fragment Transaction
**Goal:** Verify fragmentation works
```
1. Create 1000-byte transaction
2. Fragment (should be 3 fragments)
3. Broadcast to peer
4. Verify all fragments received
5. Verify reconstruction
Expected: All fragments arrive, perfect reconstruction
```

### Scenario 3: Out-of-Order Fragments
**Goal:** Verify order independence
```
1. Fragment transaction
2. Send fragments in random order
3. Verify reconstruction still works
Expected: Order doesn't matter
```

### Scenario 4: Parallel Broadcasts
**Goal:** Verify multiple transactions
```
1. Device A broadcasts TX1
2. Device B broadcasts TX2 (simultaneously)
3. Both receive both transactions
4. Verify no interference
Expected: Both transactions reconstruct correctly
```

### Scenario 5: Multi-Hop Propagation
**Goal:** Verify mesh routing
```
Device layout: A ← → B ← → C
(A can't reach C directly)

1. Device A broadcasts transaction
2. Device B receives and forwards
3. Device C receives from B
4. Verify C reconstructs correctly
Expected: Transaction propagates across hops
```

### Scenario 6: Network Partition
**Goal:** Verify resilience
```
1. Start with A ← → B ← → C
2. Disconnect B (partition network)
3. A broadcasts transaction
4. Verify A knows C didn't receive
Expected: Graceful handling of partition
```

---

## 7. Performance Benchmarks

### Metrics to Measure

```kotlin
data class BenchmarkResults(
    val fragmentationTime: Long,      // Time to fragment (ms)
    val transmissionTime: Long,        // Time to send all fragments (ms)
    val reconstructionTime: Long,      // Time to reconstruct (ms)
    val totalLatency: Long,            // End-to-end (ms)
    val fragmentCount: Int,
    val bytesTransferred: Int,
    val successRate: Float             // % of fragments received
)

suspend fun runBenchmark(
    sdk: PolliNetSDK,
    txSize: Int,
    peerCount: Int
): BenchmarkResults {
    val startTime = System.currentTimeMillis()
    
    // Create transaction
    val txBytes = ByteArray(txSize) { it.toByte() }
    
    // Measure fragmentation
    val fragStart = System.currentTimeMillis()
    val prep = sdk.prepareBroadcast(txBytes).getOrThrow()
    val fragTime = System.currentTimeMillis() - fragStart
    
    // Measure transmission
    val txStart = System.currentTimeMillis()
    // ... send to peers ...
    val txTime = System.currentTimeMillis() - txStart
    
    // Measure reconstruction (on receiving device)
    val reconStart = System.currentTimeMillis()
    // ... reconstruct ...
    val reconTime = System.currentTimeMillis() - reconStart
    
    val totalTime = System.currentTimeMillis() - startTime
    
    return BenchmarkResults(
        fragmentationTime = fragTime,
        transmissionTime = txTime,
        reconstructionTime = reconTime,
        totalLatency = totalTime,
        fragmentCount = prep.fragmentPackets.size,
        bytesTransferred = txSize,
        successRate = 1.0f // Update based on actual results
    )
}
```

### Expected Performance

| Transaction Size | Fragments | Fragmentation | Transmission (1 peer) | Total |
|-----------------|-----------|---------------|----------------------|-------|
| 200 bytes       | 1         | < 1 ms        | ~20 ms              | ~25 ms |
| 500 bytes       | 2         | < 1 ms        | ~40 ms              | ~45 ms |
| 1000 bytes      | 3         | < 1 ms        | ~60 ms              | ~65 ms |

*Note: Transmission time depends on BLE MTU and connection interval*

---

## 8. Common Issues & Solutions

### Issue: Fragments Not Received
**Symptoms:** Device B doesn't see fragments from Device A

**Debug:**
```bash
# On Device B
adb logcat -s "PolliNet-Rust:*" | grep "fragment"
```

**Solutions:**
- Check BLE permissions granted
- Verify devices are connected (not just discovered)
- Check GATT characteristic is writable
- Verify MTU is sufficient

### Issue: Reconstruction Fails
**Symptoms:** "Missing fragments" or "Hash mismatch"

**Debug:**
```kotlin
// Check which fragments were received
Log.d("Mesh", "Received fragments: ${incompleteTx.receivedFragments}")
Log.d("Mesh", "Total expected: ${incompleteTx.totalFragments}")
```

**Solutions:**
- Verify all fragments sent
- Check for transmission errors
- Look for fragment corruption
- Verify transaction ID matches

### Issue: Poor Performance
**Symptoms:** Slow transmission, timeouts

**Debug:**
```kotlin
// Measure per-fragment time
val start = System.currentTimeMillis()
sendFragment(...)
val elapsed = System.currentTimeMillis() - start
Log.d("Perf", "Fragment sent in ${elapsed}ms")
```

**Solutions:**
- Increase MTU if possible
- Reduce delay between fragments
- Check BLE connection interval
- Verify signal strength (RSSI)

---

## 9. Automated Test Suite

### Create Test Runner

```kotlin
class BleTestSuite {
    suspend fun runAllTests(): TestResults {
        val results = mutableListOf<TestResult>()
        
        results.add(testFragmentation())
        results.add(testBroadcast())
        results.add(testReconstruction())
        results.add(testMultiPeer())
        
        return TestResults(
            total = results.size,
            passed = results.count { it.passed },
            failed = results.count { !it.passed },
            results = results
        )
    }
    
    private suspend fun testFragmentation(): TestResult {
        return try {
            val tx = ByteArray(500) { it.toByte() }
            val fragments = sdk.fragmentTransaction(tx).getOrThrow()
            val reconstructed = sdk.reconstructTransaction(fragments).getOrThrow()
            val bytes = Base64.decode(reconstructed, Base64.NO_WRAP)
            
            TestResult(
                name = "Fragmentation",
                passed = bytes.contentEquals(tx),
                duration = 0,
                message = "✅ Fragmentation works"
            )
        } catch (e: Exception) {
            TestResult(
                name = "Fragmentation",
                passed = false,
                duration = 0,
                message = "❌ ${e.message}"
            )
        }
    }
}
```

---

## 10. Quick Start Checklist

### Single Device Testing
- [ ] Build and install app
- [ ] Grant BLE permissions
- [ ] Run fragmentation test
- [ ] Verify logs show success
- [ ] Check diagnostics screen

### Two Device Testing
- [ ] Install on both devices
- [ ] Device A: Start advertising
- [ ] Device B: Start scanning
- [ ] Verify connection established
- [ ] Device A: Send test transaction
- [ ] Device B: Verify received
- [ ] Check logs on both devices

### Multi-Device Testing (3+)
- [ ] Setup device chain A→B→C
- [ ] Verify all connections
- [ ] Device A: Broadcast transaction
- [ ] Device B: Verify forwarding
- [ ] Device C: Verify reception
- [ ] Measure end-to-end latency
- [ ] Test with different topologies

---

## Conclusion

This testing guide covers:
- ✅ Unit tests (Rust)
- ✅ Integration tests (Android)
- ✅ Manual testing procedures
- ✅ Multi-device scenarios
- ✅ Debugging tools
- ✅ Performance benchmarks
- ✅ Common issues

Start with single-device tests, then move to multi-device, and finally test complex mesh scenarios!

---

*Last Updated: November 7, 2025*

