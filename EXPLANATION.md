# PolliNet Architecture: Complete Technical Explanation

## Table of Contents

1. [Project Overview](#project-overview)
2. [Architecture Layers](#architecture-layers)
3. [Communication Flow: Kotlin → Rust](#communication-flow-kotlin--rust)
4. [Detailed Component Analysis](#detailed-component-analysis)
5. [Data Flow Examples](#data-flow-examples)
6. [Understanding Rust Concepts](#understanding-rust-concepts)
7. [External Libraries & Dependencies](#external-libraries--dependencies)

---

## 1. Project Overview

**PolliNet** is a decentralized system that enables **offline Solana transaction propagation** over Bluetooth Low Energy (BLE) mesh networks. Think of it like biological pollination: transactions are created offline, passed from device to device via Bluetooth, and submitted to the Solana blockchain when any device gets internet access.

### Key Innovation
- **Offline Transaction Creation**: Create valid Solana transactions without internet using cached nonce accounts
- **BLE Mesh Networking**: Devices relay transactions to each other over Bluetooth
- **Store-and-Forward**: Any device can hold and relay transactions until submission

### Project Structure
```
pollinet/
├── src/                          # Rust core library
│   ├── lib.rs                   # Main SDK entry point
│   ├── ffi/                     # Foreign Function Interface (Kotlin ↔ Rust)
│   ├── transaction/             # Transaction building & management
│   ├── ble/                     # Bluetooth LE mesh networking
│   ├── nonce/                   # Nonce account management
│   └── util/                    # Helper utilities
│
└── pollinet-android/            # Android implementation
    ├── pollinet-sdk/            # Android SDK (AAR library)
    │   └── src/main/java/xyz/pollinet/sdk/
    │       ├── PolliNetFFI.kt   # JNI bindings (calls Rust)
    │       ├── PolliNetSDK.kt   # High-level Kotlin API
    │       └── BleService.kt    # Android BLE service
    │
    └── app/                     # Demo Android app
        └── src/main/java/xyz/pollinet/android/ui/
            └── DiagnosticsScreen.kt  # UI that uses SDK
```

---

## 2. Architecture Layers

The PolliNet system has **5 distinct layers** working together:

### Layer 1: Android UI (Kotlin)
- **Location**: `pollinet-android/app/src/main/java/xyz/pollinet/android/ui/`
- **Purpose**: User interface and app logic
- **Technology**: Jetpack Compose (modern Android UI)
- **Example**: `DiagnosticsScreen.kt` - displays metrics, buttons for BLE operations

### Layer 2: Android SDK (Kotlin)
- **Location**: `pollinet-android/pollinet-sdk/src/main/java/xyz/pollinet/sdk/`
- **Purpose**: High-level Kotlin API that Android apps use
- **Key Files**:
  - `PolliNetSDK.kt` - User-friendly API with coroutines and Result types
  - `BleService.kt` - Android foreground service managing BLE
  - `PolliNetFFI.kt` - Low-level JNI interface to Rust

### Layer 3: JNI Bridge (Kotlin ↔ Rust)
- **Location**: `pollinet-android/pollinet-sdk/src/main/java/xyz/pollinet/sdk/PolliNetFFI.kt`
- **Purpose**: Bridge between Kotlin (Java bytecode) and Rust (native code)
- **Technology**: JNI (Java Native Interface)
- **Data Format**: JSON for complex types, raw bytes for performance

### Layer 4: Rust FFI Layer
- **Location**: `src/ffi/android.rs`
- **Purpose**: Expose Rust functions to JNI, handle data conversion
- **Key Concept**: This is the "entry point" from Kotlin into Rust

### Layer 5: Rust Core Library
- **Location**: `src/lib.rs`, `src/transaction/`, `src/ble/`
- **Purpose**: Core business logic - transactions, BLE, cryptography
- **Technology**: Pure Rust with async/await (Tokio runtime)

### Layer 6: External Libraries
- **Solana SDK**: Transaction building, signing, blockchain interaction
- **btleplug**: Cross-platform Bluetooth LE library
- **LZ4**: Transaction compression
- **OpenSSL**: Cryptographic operations

---

## 3. Communication Flow: Kotlin → Rust

Let's trace a complete example: **Creating an offline transaction**

### Step 1: User Clicks Button in UI (Kotlin)
**File**: `DiagnosticsScreen.kt` (Line 576-617)

```kotlin
Button(onClick = {
    scope.launch {
        // Step 1: Initialize SDK
        val config = SdkConfig(
            rpcUrl = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6",
            enableLogging = true
        )
        
        PolliNetSDK.initialize(config).onSuccess { sdk ->
            // Step 2: Prepare offline bundle
            sdk.prepareOfflineBundle(
                count = 3,
                senderKeypair = keypairBytes,
                bundleFile = null
            ).onSuccess { bundle ->
                // Bundle contains 3 nonce accounts for offline use
                println("Bundle ready with ${bundle.totalNonces()} nonces")
            }
        }
    }
})
```

**What happens here:**
- Kotlin coroutine (`scope.launch`) runs async code
- `PolliNetSDK.initialize()` is called
- `Result<T>` type handles success/failure elegantly
- Data flows to next layer...

---

### Step 2: High-Level SDK (Kotlin)
**File**: `PolliNetSDK.kt` (Line 221-246)

```kotlin
suspend fun prepareOfflineBundle(
    count: Int,
    senderKeypair: ByteArray,
    bundleFile: String? = null
): Result<OfflineTransactionBundle> = withContext(Dispatchers.IO) {
    try {
        // Step 2a: Create request object
        val request = PrepareOfflineBundleRequest(
            count = count,
            senderKeypairBase64 = Base64.encodeToString(
                senderKeypair, 
                Base64.NO_WRAP
            ),
            bundleFile = bundleFile
        )
        
        // Step 2b: Serialize to JSON
        val requestJson = json.encodeToString(request).toByteArray()
        
        // Step 2c: Call JNI (jump to Rust!)
        val resultJson = PolliNetFFI.prepareOfflineBundle(handle, requestJson)
        
        // Step 2d: Parse JSON result
        val bundleJsonResult = parseResult<String>(resultJson)
        bundleJsonResult.map { bundleJsonStr ->
            json.decodeFromString<OfflineTransactionBundle>(bundleJsonStr)
        }
    } catch (e: Exception) {
        Result.failure(e)
    }
}
```

**Key Concepts:**
- `withContext(Dispatchers.IO)` - runs on background thread (don't block UI)
- **Serialization**: Convert Kotlin objects to JSON bytes
- **Base64 encoding**: Binary data (keypair) → text-safe format
- `PolliNetFFI.prepareOfflineBundle()` - **THE BOUNDARY** into Rust

---

### Step 3: JNI Bindings (Kotlin)
**File**: `PolliNetFFI.kt` (Line 136-141)

```kotlin
object PolliNetFFI {
    init {
        // Load native library (libpollinet.so on Android)
        System.loadLibrary("pollinet")
    }

    /**
     * Prepare offline bundle for creating transactions without internet
     * @param requestJson JSON-encoded PrepareOfflineBundleRequest
     * @return JSON FfiResult with OfflineTransactionBundle JSON string
     */
    external fun prepareOfflineBundle(
        handle: Long, 
        requestJson: ByteArray
    ): String
}
```

**What `external` means:**
- This function has **no implementation** in Kotlin
- Implementation is in **native code** (Rust compiled to `.so` library)
- JNI runtime finds the matching function in the `.so` file
- Function name follows JNI convention: `Java_packagename_classname_methodname`

**The Compiled Library:**
- **Location**: `pollinet-android/pollinet-sdk/src/main/jniLibs/arm64-v8a/libpollinet.so`
- **Built from**: Rust code in `src/` directory
- **Build tool**: `cargo-ndk` (compiles Rust for Android)

---

### Step 4: Rust FFI Entry Point
**File**: `src/ffi/android.rs` (Line 636-691)

This is where Kotlin crosses into Rust!

```rust
/// Prepare offline bundle for creating transactions without internet
/// This is a CORE PolliNet feature for offline/mesh transaction creation
#[cfg(feature = "android")]
#[no_mangle]  // ← CRITICAL: Keeps function name unchanged for JNI to find
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_prepareOfflineBundle(
    mut env: JNIEnv,      // ← JNI environment (interact with Java)
    _class: JClass,       // ← Class that called this
    handle: jlong,        // ← SDK instance handle
    request_json: JByteArray,  // ← JSON request from Kotlin
) -> jstring {            // ← Return JSON string to Kotlin
    
    // Step 4a: Safe error handling wrapper
    let result = (|| {
        // Step 4b: Get the SDK instance using handle
        let transport = get_transport(handle)?;
        
        // Step 4c: Convert JNI ByteArray to Rust Vec<u8>
        let request_data: Vec<u8> = env
            .convert_byte_array(&request_json)
            .map_err(|e| format!("Failed to read request: {}", e))?;

        // Step 4d: Deserialize JSON to Rust struct
        let request: PrepareOfflineBundleRequest =
            serde_json::from_slice(&request_data)
                .map_err(|e| format!("Failed to parse request: {}", e))?;

        // Step 4e: Decode base64 keypair
        let keypair_bytes = base64::decode(&request.sender_keypair_base64)
            .map_err(|e| format!("Invalid keypair base64: {}", e))?;
        let sender_keypair = solana_sdk::signature::Keypair::from_bytes(&keypair_bytes)
            .map_err(|e| format!("Invalid keypair bytes: {}", e))?;

        // Step 4f: Call actual business logic (async!)
        let bundle = runtime::block_on(async {
            transport
                .transaction_service()
                .prepare_offline_bundle(
                    request.count,
                    &sender_keypair,
                    request.bundle_file.as_deref(),
                )
                .await
        })
        .map_err(|e| format!("Failed to prepare bundle: {}", e))?;

        // Step 4g: Convert to FFI-friendly type
        let ffi_bundle = crate::ffi::types::OfflineTransactionBundle::from_transaction_bundle(&bundle);
        
        // Step 4h: Serialize response to JSON
        let bundle_json = serde_json::to_string(&ffi_bundle)
            .map_err(|e| format!("Failed to serialize bundle: {}", e))?;

        // Step 4i: Wrap in success response
        let response: FfiResult<String> = FfiResult::success(bundle_json);
        serde_json::to_string(&response)
            .map_err(|e| format!("Serialization error: {}", e))
    })();

    // Step 4j: Convert Result to JNI string (handles errors)
    create_result_string(&mut env, result)
}
```

**Rust Concepts Explained:**

1. **`#[no_mangle]`**: Tells compiler not to change function name
   - Without this: `Java_xyz_pollinet_sdk_PolliNetFFI_prepareOfflineBundle` → `_ZN7pollinet3ffi7android42Java_xyz_...`
   - With this: name stays exactly as written (JNI needs exact name)

2. **`extern "C"`**: Use C calling convention (JNI standard)
   - Rust has its own calling convention, but JNI expects C
   - This makes Rust function callable from Java/Kotlin

3. **`JNIEnv`**: Bridge to Java world
   - Convert Java types to Rust types
   - Create Java strings/objects from Rust
   - Call Java methods from Rust

4. **`?` operator**: Error handling shorthand
   ```rust
   let data = convert_byte_array(&request_json)?;
   // Equivalent to:
   let data = match convert_byte_array(&request_json) {
       Ok(d) => d,
       Err(e) => return Err(e.into()),
   };
   ```

5. **`runtime::block_on(async { ... })`**: Run async code synchronously
   - Rust transactions use async/await (non-blocking)
   - JNI is synchronous (blocks until complete)
   - `block_on` bridges the two: waits for async code to finish

---

### Step 5: Rust Core Logic
**File**: `src/lib.rs` → `src/transaction/mod.rs`

Now we're in pure Rust business logic!

**Step 5a: SDK Layer** (`src/lib.rs`, Line 213-223)
```rust
pub async fn prepare_offline_bundle(
    &self,
    count: usize,
    sender_keypair: &solana_sdk::signature::Keypair,
    bundle_file: Option<&str>,
) -> Result<transaction::OfflineTransactionBundle, PolliNetError> {
    Ok(self
        .transaction_service
        .prepare_offline_bundle(count, sender_keypair, bundle_file)
        .await?)
}
```

**Rust Concepts:**
- `async fn`: Function that can be paused/resumed (non-blocking I/O)
- `.await?`: Wait for async result, propagate errors
- `&self`: Reference to SDK instance (doesn't take ownership)
- `&Keypair`: Borrow keypair (don't move/consume it)

**Step 5b: Transaction Service** (`src/transaction/mod.rs`, Line 500-650)
```rust
pub async fn prepare_offline_bundle(
    &self,
    count: usize,
    sender_keypair: &Keypair,
    bundle_file: Option<&str>,
) -> Result<OfflineTransactionBundle, TransactionError> {
    // Load existing bundle if file exists
    let mut bundle = if let Some(path) = bundle_file {
        OfflineTransactionBundle::load_from_file(path)
            .unwrap_or_else(|_| OfflineTransactionBundle::new())
    } else {
        OfflineTransactionBundle::new()
    };

    // Refresh used nonces (fetch new blockhash) - FREE!
    for nonce_cache in &mut bundle.nonce_caches {
        if nonce_cache.used {
            // Fetch fresh nonce data from blockchain
            let fresh_nonce = self.fetch_nonce_data(&nonce_cache.nonce_account).await?;
            nonce_cache.blockhash = fresh_nonce.blockhash;
            nonce_cache.used = false;  // Ready to use again!
            nonce_cache.cached_at = current_timestamp();
        }
    }

    // Create new nonce accounts if needed
    let needed = count.saturating_sub(bundle.total_nonces());
    if needed > 0 {
        for _ in 0..needed {
            // Create new nonce account (costs ~0.0015 SOL)
            let nonce_account = self.create_nonce_account(sender_keypair).await?;
            let cached_nonce = self.fetch_nonce_data(&nonce_account).await?;
            bundle.add_nonce(cached_nonce);
        }
    }

    bundle.created_at = current_timestamp();
    Ok(bundle)
}
```

**What's happening:**
1. **Load or create** bundle from file
2. **Refresh used nonces** by fetching new blockhash (FREE operation!)
3. **Create new nonces** only if total < requested count (costs money)
4. Return bundle ready for offline use

**Rust Ownership:**
- `&mut bundle`: Mutable reference (can modify but don't own)
- `bundle.nonce_caches`: Move ownership into loop (we own it)
- `.await?`: Wait for async RPC call, propagate errors

---

### Step 6: External Library Interaction

**Solana RPC Client** (from `solana-client` crate):

```rust
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;

pub async fn fetch_nonce_data(&self, nonce_account: &str) -> Result<CachedNonceData, TransactionError> {
    let client = self.rpc_client
        .as_ref()
        .ok_or(TransactionError::NoRpcClient)?;

    // Parse nonce account address
    let nonce_pubkey = Pubkey::from_str(nonce_account)?;

    // Fetch account data from Solana blockchain
    let account = client
        .get_account_with_commitment(&nonce_pubkey, CommitmentConfig::confirmed())
        .await?
        .value
        .ok_or(TransactionError::AccountNotFound)?;

    // Deserialize nonce account state
    let nonce_state: NonceState = bincode::deserialize(&account.data)?;
    
    // Extract durable blockhash
    let blockhash = match nonce_state {
        NonceState::Initialized(data) => data.blockhash().to_string(),
        _ => return Err(TransactionError::InvalidNonceState),
    };

    Ok(CachedNonceData {
        nonce_account: nonce_account.to_string(),
        authority: nonce_state.authority().to_string(),
        blockhash,
        lamports_per_signature: 5000,
        cached_at: current_timestamp(),
        used: false,
    })
}
```

**External Libraries Used:**
1. **`solana-client`**: HTTP RPC client for Solana blockchain
2. **`solana-sdk`**: Core Solana types (Pubkey, Transaction, Instruction)
3. **`bincode`**: Binary serialization (Solana's wire format)

**Flow:**
1. Create `RpcClient` with endpoint URL
2. Call `.get_account_with_commitment()` - **HTTP request to Solana**
3. Deserialize account data (binary format)
4. Extract nonce state and blockhash
5. Return cached data structure

---

### Step 7: Response Flow Back to Kotlin

The result flows back through all layers:

**Step 7a: Rust FFI wraps response in JSON**
```rust
let response: FfiResult<String> = FfiResult::success(bundle_json);
serde_json::to_string(&response)
```

**Step 7b: JNI converts to Java String**
```rust
env.new_string(json)
    .expect("Failed to create Java string")
    .into_raw()
```

**Step 7c: Kotlin SDK parses JSON**
```kotlin
val bundleJsonResult = parseResult<String>(resultJson)
bundleJsonResult.map { bundleJsonStr ->
    json.decodeFromString<OfflineTransactionBundle>(bundleJsonStr)
}
```

**Step 7d: UI updates**
```kotlin
.onSuccess { bundle ->
    println("✅ Bundle ready: ${bundle.totalNonces()} nonces")
    // Update UI state
    bundleState = bundle
}
```

---

## 4. Detailed Component Analysis

### 4.1 FFI Layer Architecture

The FFI (Foreign Function Interface) is the bridge between Kotlin and Rust. Understanding this is key to the entire system.

#### 4.1.1 Why FFI?

**Problem**: Android apps are written in Kotlin/Java, but we want high-performance crypto and networking in Rust.

**Solution**: FFI lets Kotlin call Rust functions as if they were native libraries.

**Benefits**:
- ✅ Use Rust's speed for crypto operations
- ✅ Share code across platforms (iOS, Android, Desktop)
- ✅ Rust's safety prevents crashes and memory leaks
- ✅ Kotlin keeps friendly Android APIs

#### 4.1.2 FFI Data Types

**Challenge**: Kotlin and Rust have different type systems. We need a common format.

**Solution**: Use JSON for complex types, raw bytes for performance-critical data.

**Example - FFI Request Type** (`src/ffi/types.rs`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareOfflineBundleRequest {
    #[serde(default = "default_version")]
    pub version: u32,
    
    pub count: usize,
    
    #[serde(rename = "senderKeypairBase64")]
    pub sender_keypair_base64: String,  // Base64-encoded bytes
    
    #[serde(rename = "bundleFile")]
    pub bundle_file: Option<String>,
}
```

**Key Annotations:**
- `#[derive(Serialize, Deserialize)]`: Auto-generate JSON conversion
- `#[serde(rename = "...")]`: Match Kotlin camelCase naming
- `Option<String>`: Nullable field (can be null)

**Corresponding Kotlin Type** (`PolliNetSDK.kt`):
```kotlin
@Serializable
data class PrepareOfflineBundleRequest(
    val version: Int = 1,
    val count: Int,
    val senderKeypairBase64: String,
    val bundleFile: String? = null  // Nullable
)
```

#### 4.1.3 Error Handling Across FFI

**Challenge**: Rust `Result<T, E>` doesn't exist in Kotlin.

**Solution**: Wrap everything in JSON response envelope:

**Rust FFI Result Type**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FfiResult<T> {
    Ok { ok: bool, data: T },
    Err { ok: bool, code: String, message: String },
}

impl<T> FfiResult<T> {
    pub fn success(data: T) -> Self {
        FfiResult::Ok { ok: true, data }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        FfiResult::Err {
            ok: false,
            code: code.into(),
            message: message.into(),
        }
    }
}
```

**Example Success JSON**:
```json
{
  "ok": true,
  "data": {
    "nonceCaches": [...],
    "maxTransactions": 3,
    "createdAt": 1234567890
  }
}
```

**Example Error JSON**:
```json
{
  "ok": false,
  "code": "ERR_INTERNAL",
  "message": "Failed to fetch nonce data: Connection timeout"
}
```

**Kotlin Parsing**:
```kotlin
private inline fun <reified T> parseResult(json: String): Result<T> {
    return try {
        val successResult = this.json.decodeFromString<FfiResultSuccess<T>>(json)
        if (successResult.ok) {
            Result.success(successResult.data)
        } else {
            Result.failure(Exception("Unexpected result format"))
        }
    } catch (e: Exception) {
        try {
            val errorResult = this.json.decodeFromString<FfiResultError>(json)
            Result.failure(PolliNetException(errorResult.code, errorResult.message))
        } catch (e2: Exception) {
            Result.failure(Exception("Failed to parse FFI result"))
        }
    }
}
```

---

### 4.2 BLE Service Architecture

The BLE (Bluetooth Low Energy) service manages wireless communication between devices.

#### 4.2.1 Android BLE Components

**Location**: `pollinet-android/pollinet-sdk/src/main/java/xyz/pollinet/sdk/BleService.kt`

**Purpose**: Android foreground service that keeps BLE running even when app is backgrounded.

**Key Components**:

1. **GATT Server** (Peripheral mode - this device advertises)
   ```kotlin
   private var gattServer: BluetoothGattServer? = null
   
   private fun setupGattServer() {
       val service = BluetoothGattService(
           SERVICE_UUID,  // "00001820-..."
           BluetoothGattService.SERVICE_TYPE_PRIMARY
       )

       // TX characteristic (server → client)
       gattCharacteristicTx = BluetoothGattCharacteristic(
           TX_CHAR_UUID,
           BluetoothGattCharacteristic.PROPERTY_NOTIFY,
           BluetoothGattCharacteristic.PERMISSION_READ
       )

       // RX characteristic (client → server)
       gattCharacteristicRx = BluetoothGattCharacteristic(
           RX_CHAR_UUID,
           BluetoothGattCharacteristic.PROPERTY_WRITE,
           BluetoothGattCharacteristic.PERMISSION_WRITE
       )

       service.addCharacteristic(gattCharacteristicTx)
       service.addCharacteristic(gattCharacteristicRx)

       gattServer = bluetoothManager?.openGattServer(this, gattServerCallback)
       gattServer?.addService(service)
   }
   ```

2. **BLE Scanner** (Central mode - discover other devices)
   ```kotlin
   fun startScanning() {
       bleScanner?.let { scanner ->
           val scanFilter = ScanFilter.Builder()
               .setServiceUuid(ParcelUuid(SERVICE_UUID))
               .build()
           
           val scanSettings = ScanSettings.Builder()
               .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
               .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
               .build()
           
           scanner.startScan(listOf(scanFilter), scanSettings, scanCallback)
       }
   }
   ```

3. **BLE Advertiser** (Make this device discoverable)
   ```kotlin
   fun startAdvertising() {
       bleAdvertiser?.let { advertiser ->
           val settings = AdvertiseSettings.Builder()
               .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
               .setConnectable(true)
               .setTimeout(0)
               .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
               .build()
           
           val data = AdvertiseData.Builder()
               .setIncludeDeviceName(false)
               .addServiceUuid(ParcelUuid(SERVICE_UUID))
               .build()
           
           advertiser.startAdvertising(settings, data, advertiseCallback)
       }
   }
   ```

#### 4.2.2 GATT to Rust Bridge

**When data arrives via BLE**, it flows from GATT callback to Rust FFI:

```kotlin
private val gattServerCallback = object : BluetoothGattServerCallback() {
    override fun onCharacteristicWriteRequest(
        device: BluetoothDevice,
        requestId: Int,
        characteristic: BluetoothGattCharacteristic,
        preparedWrite: Boolean,
        responseNeeded: Boolean,
        offset: Int,
        value: ByteArray  // ← Data received from other device
    ) {
        if (characteristic.uuid == RX_CHAR_UUID) {
            // Forward to Rust FFI
            serviceScope.launch {
                sdk?.pushInbound(value)  // ← Calls into Rust!
            }
            
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            }
        }
    }
}
```

**Flow**: 
BLE chip → Android OS → GATT callback → Kotlin → Rust FFI → Rust transport layer

---

### 4.3 Rust BLE Transport Layer

**Location**: `src/ffi/transport.rs`

**Purpose**: Host-driven transport where Android controls BLE hardware, Rust handles protocol logic.

#### 4.3.1 Host-Driven Architecture

**Traditional BLE**: Rust code directly controls Bluetooth adapter
**PolliNet (Host-Driven)**: Android controls Bluetooth, Rust provides packetization/reassembly

**Why?**
- ✅ Android BLE APIs are more reliable than generic Rust BLE libraries
- ✅ Easier permission handling in Android
- ✅ Better power management (Android knows about battery)
- ✅ Simpler architecture (no Rust BLE threads)

#### 4.3.2 Transport State Machine

```rust
pub struct HostBleTransport {
    /// Queue of outbound frames ready to send
    outbound_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    
    /// Inbound reassembly buffers keyed by transaction ID
    inbound_buffers: Arc<Mutex<HashMap<String, Vec<TxFragment>>>>,
    
    /// Completed transactions ready for processing
    completed_transactions: Arc<Mutex<VecDeque<(String, Vec<u8>)>>>,
    
    /// Metrics
    metrics: Arc<Mutex<TransportMetrics>>,
    
    /// Transaction service for fragmentation and building
    transaction_service: Arc<TransactionService>,
}
```

**Rust Concepts:**
- `Arc<T>`: Thread-safe reference counting (multiple owners)
- `Mutex<T>`: Mutual exclusion lock (only one thread at a time)
- `VecDeque<T>`: Double-ended queue (efficient push/pop both ends)
- `HashMap<K, V>`: Hash table (key-value store)

**Example - Push Inbound Data**:
```rust
pub fn push_inbound(&self, data: Vec<u8>) -> Result<(), String> {
    // Step 1: Deserialize fragment from JSON
    let fragment: TxFragment = serde_json::from_slice(&data)
        .map_err(|e| format!("Failed to deserialize fragment: {}", e))?;

    let tx_id = fragment.id.clone();
    
    // Step 2: Store in reassembly buffer
    let mut buffers = self.inbound_buffers.lock();  // ← Acquire lock
    let buffer = buffers.entry(tx_id.clone()).or_insert_with(Vec::new);
    buffer.push(fragment.clone());
    
    // Step 3: Check if all fragments received
    let total_fragments = fragment.total;
    let all_received = buffer.len() == total_fragments;
    
    if all_received {
        // Step 4: Reassemble transaction
        let fragments = buffer.clone();
        drop(buffers);  // ← Release lock early
        
        match self.transaction_service.reassemble_fragments(&fragments) {
            Ok(tx_bytes) => {
                // Step 5: Move to completed queue
                let mut completed = self.completed_transactions.lock();
                completed.push_back((tx_id.clone(), tx_bytes));
                
                // Step 6: Remove from inbound buffers
                self.inbound_buffers.lock().remove(&tx_id);
                
                tracing::info!("✅ Transaction {} reassembled", tx_id);
                Ok(())
            }
            Err(e) => Err(format!("Reassembly failed: {}", e))
        }
    } else {
        Ok(())  // Waiting for more fragments
    }
}
```

**Locking Strategy:**
- Acquire lock → Do minimal work → Release lock ASAP
- Avoid holding locks across async calls (deadlock risk)
- Clone data when you need to keep it across lock releases

---

### 4.4 Transaction Fragmentation

**Problem**: BLE packets are limited to ~512 bytes. Solana transactions can be larger.

**Solution**: Split transaction into fragments, send each separately, reassemble on receiver.

#### 4.4.1 Fragmentation Algorithm

```rust
pub fn fragment_transaction(&self, compressed_tx: &[u8]) -> Vec<Fragment> {
    const MAX_FRAGMENT_SIZE: usize = 400;  // Conservative size
    
    let tx_id = uuid::Uuid::new_v4().to_string();
    let checksum = sha256_checksum(compressed_tx);
    
    let chunks: Vec<&[u8]> = compressed_tx
        .chunks(MAX_FRAGMENT_SIZE)
        .collect();
    
    let total_fragments = chunks.len();
    
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let fragment_type = if index == 0 {
                FragmentType::FragmentStart
            } else if index == total_fragments - 1 {
                FragmentType::FragmentEnd
            } else {
                FragmentType::FragmentContinue
            };
            
            Fragment {
                id: tx_id.clone(),
                index,
                total: total_fragments,
                data: chunk.to_vec(),
                fragment_type,
                checksum: checksum.clone(),
            }
        })
        .collect()
}
```

**Example - 1200 byte transaction**:
```
Original: [1200 bytes of transaction data]

Fragment 0 (START):   [400 bytes] + metadata
Fragment 1 (CONTINUE): [400 bytes] + metadata  
Fragment 2 (END):     [400 bytes] + metadata
```

**Metadata** (sent with each fragment):
```json
{
  "id": "a1b2c3d4-...",     // Same ID for all fragments
  "index": 0,               // Position in sequence
  "total": 3,               // Total fragments
  "data": "base64...",      // Fragment payload
  "fragment_type": "FragmentStart",
  "checksum": "abc123..."   // SHA-256 of original transaction
}
```

#### 4.4.2 Reassembly Algorithm

```rust
pub fn reassemble_fragments(&self, fragments: &[Fragment]) -> Result<Vec<u8>, TransactionError> {
    // Step 1: Validate all fragments have same ID and checksum
    let first = fragments.first()
        .ok_or(TransactionError::InvalidFragment("Empty fragment list".to_string()))?;
    
    let tx_id = &first.id;
    let expected_checksum = &first.checksum;
    
    for fragment in fragments {
        if fragment.id != *tx_id {
            return Err(TransactionError::InvalidFragment(
                format!("Fragment ID mismatch: {} != {}", fragment.id, tx_id)
            ));
        }
        if fragment.checksum != *expected_checksum {
            return Err(TransactionError::InvalidFragment("Checksum mismatch".to_string()));
        }
    }
    
    // Step 2: Sort fragments by index
    let mut sorted = fragments.to_vec();
    sorted.sort_by_key(|f| f.index);
    
    // Step 3: Check for missing fragments
    let total = first.total;
    if sorted.len() != total {
        return Err(TransactionError::InvalidFragment(
            format!("Missing fragments: got {}, expected {}", sorted.len(), total)
        ));
    }
    
    // Step 4: Concatenate data
    let mut reassembled = Vec::new();
    for (expected_index, fragment) in sorted.iter().enumerate() {
        if fragment.index != expected_index {
            return Err(TransactionError::InvalidFragment(
                format!("Fragment index mismatch: expected {}, got {}", expected_index, fragment.index)
            ));
        }
        reassembled.extend_from_slice(&fragment.data);
    }
    
    // Step 5: Verify checksum
    let computed_checksum = sha256_checksum(&reassembled);
    if computed_checksum != *expected_checksum {
        return Err(TransactionError::ChecksumMismatch);
    }
    
    Ok(reassembled)
}
```

---

## 5. Data Flow Examples

### Example 1: Complete Offline Transaction Flow

**Scenario**: User creates and sends a transaction while completely offline.

#### Phase 1: Preparation (While Online)

1. **User**: Clicks "Prepare Bundle" in UI
2. **Kotlin UI**: Calls `sdk.prepareOfflineBundle(count=3)`
3. **Kotlin SDK**: Serializes request to JSON → Calls FFI
4. **Rust FFI**: Deserializes JSON → Calls transaction service
5. **Rust Transaction Service**: 
   - Connects to Solana RPC
   - Creates 3 nonce accounts (~$0.60 total cost)
   - Fetches nonce data (blockhash, authority)
   - Packages into `OfflineTransactionBundle`
6. **Rust FFI**: Serializes bundle to JSON → Returns to Kotlin
7. **Kotlin SDK**: Deserializes bundle → Returns to UI
8. **UI**: Displays "Bundle ready: 3 nonces available"

**Result**: Bundle saved with cached nonce data. No internet needed for next steps!

#### Phase 2: Transaction Creation (Offline)

1. **User**: Clicks "Create Transaction (Offline)"
2. **Kotlin UI**: Calls `sdk.createOfflineTransaction(...)`
3. **Kotlin SDK**: 
   - Takes first available nonce from bundle
   - Serializes request to JSON → Calls FFI
4. **Rust FFI**: Deserializes → Calls transaction service
5. **Rust Transaction Service**:
   ```rust
   pub fn create_offline_transaction(
       &self,
       sender_keypair: &Keypair,
       recipient: &str,
       amount: u64,
       nonce_authority_keypair: &Keypair,
       cached_nonce: &CachedNonceData,
   ) -> Result<Vec<u8>, TransactionError> {
       // Step 1: Parse recipient address
       let recipient_pubkey = Pubkey::from_str(recipient)?;
       
       // Step 2: Parse cached blockhash (NO RPC needed!)
       let blockhash = Hash::from_str(&cached_nonce.blockhash)?;
       
       // Step 3: Build transaction with durable nonce
       let mut transaction = Transaction::new_with_payer(
           &[
               // Advance nonce instruction (MUST be first!)
               solana_sdk::system_instruction::advance_nonce_account(
                   &nonce_account_pubkey,
                   &nonce_authority_keypair.pubkey(),
               ),
               // Transfer instruction
               solana_sdk::system_instruction::transfer(
                   &sender_keypair.pubkey(),
                   &recipient_pubkey,
                   amount,
               ),
           ],
           Some(&sender_keypair.pubkey()),  // Fee payer
       );
       
       // Step 4: Set durable blockhash (from cached nonce)
       transaction.message.recent_blockhash = blockhash;
       
       // Step 5: Sign transaction (sender + nonce authority)
       transaction.sign(
           &[sender_keypair, nonce_authority_keypair],
           transaction.message.recent_blockhash,
       );
       
       // Step 6: Serialize and compress
       let tx_bytes = bincode::serialize(&transaction)?;
       let compressed = lz4::compress(&tx_bytes)?;
       
       Ok(compressed)
   }
   ```
6. **Rust FFI**: Base64 encode → Return JSON to Kotlin
7. **Kotlin SDK**: Decode → Return to UI
8. **UI**: Displays "Transaction created offline: ready for BLE"

**Key Insight**: NO internet required! Transaction is fully signed and valid using cached nonce data.

#### Phase 3: BLE Transmission (Offline)

1. **User**: Clicks "Send via BLE"
2. **Kotlin UI**: Calls `sdk.fragment(txBytes)` → `bleService.sendFragments()`
3. **Rust FFI**: Fragments transaction:
   ```
   Compressed TX: [800 bytes]
   Fragment 0: [400 bytes] + metadata
   Fragment 1: [400 bytes] + metadata
   ```
4. **BLE Service**: 
   - Places fragments in outbound queue
   - Periodically calls `sdk.nextOutbound()`
5. **Rust Transport**: Returns next fragment from queue
6. **BLE Service**: Sends fragment via GATT notification:
   ```kotlin
   gattCharacteristicTx.value = fragmentData
   gattServer.notifyCharacteristicChanged(connectedDevice, gattCharacteristicTx, false)
   ```
7. **Receiving Device**: Gets GATT callback → Forwards to its Rust FFI
8. **Receiving Rust**: Calls `push_inbound(fragmentData)`
9. **Rust Transport**: Stores fragment, checks if all received
10. **When all fragments received**: Reassembles → Stores in completed queue

**Result**: Transaction propagated to another device over BLE mesh!

#### Phase 4: Submission (Back Online)

1. **Receiving Device**: Regains internet access
2. **Background Task**: Checks completed transaction queue
3. **Rust Transaction Service**: 
   ```rust
   pub async fn submit_offline_transaction(
       &self,
       compressed_tx: &[u8],
       verify_nonce: bool,
   ) -> Result<String, TransactionError> {
       // Step 1: Decompress transaction
       let tx_bytes = lz4::decompress(compressed_tx)?;
       let transaction: Transaction = bincode::deserialize(&tx_bytes)?;
       
       // Step 2: Optionally verify nonce still valid
       if verify_nonce {
           // Check nonce hasn't been advanced yet
           self.verify_nonce_valid(&transaction).await?;
       }
       
       // Step 3: Submit to Solana
       let client = self.rpc_client.as_ref().ok_or(TransactionError::NoRpcClient)?;
       let signature = client.send_and_confirm_transaction(&transaction).await?;
       
       Ok(signature.to_string())
   }
   ```
4. **Solana Blockchain**: Validates transaction → Processes → Confirms
5. **Rust**: Returns signature
6. **UI**: Displays "Transaction confirmed! Signature: abc123..."

**Result**: Transaction created offline, relayed over BLE mesh, submitted when internet available!

---

## 6. Understanding Rust Concepts

### 6.1 Ownership and Borrowing

**The Problem**: Memory management in programming has two extremes:
- **Manual** (C/C++): Programmer calls `malloc`/`free`. Fast but error-prone (leaks, use-after-free).
- **Garbage Collection** (Java/Kotlin): Automatic but unpredictable pauses.

**Rust's Solution**: Ownership system enforced at compile time. No runtime cost!

#### Rule 1: Each value has one owner

```rust
let tx_bytes = vec![1, 2, 3, 4];  // tx_bytes owns the Vec
let other = tx_bytes;             // Ownership moved to other
// println!("{:?}", tx_bytes);    // ❌ ERROR: tx_bytes no longer valid
println!("{:?}", other);          // ✅ OK: other owns it now
```

**Why?** Prevents double-free bugs. Only one variable responsible for cleanup.

#### Rule 2: You can borrow with references

```rust
fn process_tx(tx_bytes: &Vec<u8>) {  // Borrow (don't take ownership)
    println!("Processing {} bytes", tx_bytes.len());
}  // Borrow ends here

let tx_bytes = vec![1, 2, 3, 4];
process_tx(&tx_bytes);  // Lend to function
process_tx(&tx_bytes);  // Can borrow multiple times
println!("{:?}", tx_bytes);  // ✅ Still valid! We kept ownership
```

**Why?** Allows sharing without copying. Compiler ensures borrows don't outlive owner.

#### Rule 3: Mutable XOR shared

```rust
let mut data = vec![1, 2, 3];

let ref1 = &data;     // Shared borrow (read-only)
let ref2 = &data;     // Multiple shared borrows OK
// data.push(4);      // ❌ ERROR: Can't modify while borrowed

println!("{:?} {:?}", ref1, ref2);  // Last use of borrows
data.push(4);  // ✅ OK: Borrows ended

// OR

let mut data = vec![1, 2, 3];
let ref_mut = &mut data;  // Mutable borrow (read-write)
ref_mut.push(4);          // ✅ Can modify
// let ref2 = &data;      // ❌ ERROR: Can't have shared borrow while mutable borrow exists
```

**Why?** Prevents data races at compile time. No need for locks in single-threaded code!

#### Example in PolliNet Code

```rust
pub fn fragment_transaction(&self, compressed_tx: &[u8]) -> Vec<Fragment> {
    //                                 ^^^^^^^^^^^
    //                                 Borrow slice (don't take ownership)
    
    let checksum = sha256_checksum(compressed_tx);  // ✅ Can use multiple times
    
    compressed_tx
        .chunks(MAX_FRAGMENT_SIZE)  // ✅ Can borrow again
        .enumerate()
        .map(|(index, chunk)| {
            Fragment {
                data: chunk.to_vec(),  // ← Clone the chunk (now we own the copy)
                checksum: checksum.clone(),
                // ...
            }
        })
        .collect()
}
```

**Why borrow?** Transaction data is large. Borrowing avoids copying entire transaction into function.

---

### 6.2 Async/Await (Tokio Runtime)

**The Problem**: Network I/O (RPC calls) is slow. Blocking threads waste resources.

**Traditional Solution (Kotlin)**: Coroutines with `suspend fun`
**Rust Solution**: Async/await with runtime (Tokio)

#### What is `async fn`?

```rust
async fn fetch_data(url: &str) -> Result<String, Error> {
    let response = reqwest::get(url).await?;  // ← Suspends here until data arrives
    let text = response.text().await?;         // ← Can suspend multiple times
    Ok(text)
}
```

**What actually happens:**
1. Function returns a `Future` (like Kotlin's `Deferred`)
2. `.await` yields control to runtime
3. Runtime runs other tasks while waiting
4. When data arrives, runtime resumes this function

#### Example - RPC Call

```rust
pub async fn fetch_nonce_data(&self, nonce_account: &str) -> Result<CachedNonceData, Error> {
    let client = self.rpc_client.as_ref().ok_or(Error::NoRpcClient)?;
    
    // This might take 200ms, but doesn't block the thread!
    let account = client
        .get_account_with_commitment(&nonce_pubkey, CommitmentConfig::confirmed())
        .await?;  // ← Suspends function, lets other work happen
    
    // Resumed here when data arrives
    let nonce_state: NonceState = bincode::deserialize(&account.data)?;
    Ok(extract_nonce_data(nonce_state))
}
```

**Why async?** Handles thousands of concurrent operations on few threads. Perfect for network-heavy apps.

#### Bridging Async and Sync (JNI)

**Problem**: JNI is synchronous, but Rust SDK is async.

**Solution**: `runtime::block_on()` - blocks current thread until async completes.

```rust
pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_prepareOfflineBundle(
    // ... JNI parameters
) -> jstring {
    // JNI expects synchronous response
    
    let bundle = runtime::block_on(async {
        //           ^^^^^^^^^^^^^^^^
        //           Blocks this thread until async completes
        
        transport
            .transaction_service()
            .prepare_offline_bundle(count, &sender_keypair, bundle_file)
            .await  // ← Async operation
    })?;
    
    // Return to Kotlin
    create_result_string(&env, Ok(serialize_bundle(bundle)))
}
```

**Runtime**: Background executor that runs async tasks. Think of it as Kotlin's `CoroutineScope`.

---

### 6.3 Error Handling with Result

**Kotlin**:
```kotlin
try {
    val data = fetchData()
    process(data)
} catch (e: Exception) {
    log("Error: ${e.message}")
}
```

**Rust**:
```rust
match fetch_data() {
    Ok(data) => process(data),
    Err(e) => log!("Error: {}", e),
}

// Or more concisely with ? operator:
fn example() -> Result<(), Error> {
    let data = fetch_data()?;  // ← If error, return early
    process(data)?;            // ← Propagate errors up
    Ok(())
}
```

#### Why `Result` instead of exceptions?

1. **Explicit**: Function signature says "this can fail"
   ```rust
   fn divide(a: i32, b: i32) -> Result<i32, String> {
       //                       ^^^^^^^^^^^^^^^^
       //                       Compiler forces caller to handle error
       if b == 0 {
           Err("Division by zero".to_string())
       } else {
           Ok(a / b)
       }
   }
   ```

2. **No hidden control flow**: Can't forget to catch exceptions
   ```kotlin
   // Kotlin - might crash if you forget try/catch
   val result = divide(10, 0)  // 💥 Runtime exception!
   ```
   ```rust
   // Rust - compiler error if you don't handle Result
   let result = divide(10, 0);  // ❌ ERROR: Result must be used
   let result = divide(10, 0)?; // ✅ OK: Propagated up
   match divide(10, 0) {        // ✅ OK: Handled
       Ok(v) => println!("{}", v),
       Err(e) => println!("Error: {}", e),
   }
   ```

3. **Zero-cost**: `Result` is compile-time abstraction, no runtime overhead

#### Example from PolliNet

```rust
pub async fn create_unsigned_transaction(
    &self,
    sender: &str,
    recipient: &str,
    fee_payer: &str,
    amount: u64,
    nonce_account: &str,
) -> Result<String, TransactionError> {
    //   ^^^^^^^^^^^^^^^^^^^^^^^^^^
    //   Returns either String OR error
    
    // Parse addresses (can fail if invalid)
    let sender_pubkey = Pubkey::from_str(sender)?;
    let recipient_pubkey = Pubkey::from_str(recipient)?;
    let fee_payer_pubkey = Pubkey::from_str(fee_payer)?;
    let nonce_pubkey = Pubkey::from_str(nonce_account)?;
    
    // Fetch nonce data (can fail if no RPC or network error)
    let nonce_data = self.fetch_nonce_data(nonce_account).await?;
    
    // Build transaction (can fail if serialization error)
    let transaction = self.build_transaction(
        sender_pubkey,
        recipient_pubkey,
        amount,
        nonce_data,
    )?;
    
    // Serialize (can fail if transaction too large)
    let tx_bytes = bincode::serialize(&transaction)?;
    let base64 = base64::encode(&tx_bytes);
    
    Ok(base64)  // Success!
}
```

**Error Propagation**: Each `?` either unwraps the value or returns the error. Clean!

---

### 6.4 Smart Pointers (Arc, Mutex)

**The Problem**: Need to share data between threads safely.

#### `Arc<T>` - Atomic Reference Counting

```rust
use std::sync::Arc;

let data = vec![1, 2, 3, 4];
let arc1 = Arc::new(data);      // Reference count = 1
let arc2 = arc1.clone();        // Reference count = 2 (cheap clone, just increments counter)
let arc3 = arc1.clone();        // Reference count = 3

// All three Arcs point to same data
println!("{:?}", arc1);  // [1, 2, 3, 4]
println!("{:?}", arc2);  // [1, 2, 3, 4]

drop(arc1);  // Reference count = 2
drop(arc2);  // Reference count = 1
drop(arc3);  // Reference count = 0 → data deallocated
```

**Why?** Allows multiple threads to share ownership. Last one out cleans up.

#### `Mutex<T>` - Mutual Exclusion

```rust
use std::sync::Mutex;

let counter = Mutex::new(0);

{
    let mut guard = counter.lock().unwrap();  // ← Acquire lock
    *guard += 1;                              // ← Modify data
}  // ← Lock released automatically (guard dropped)

let value = *counter.lock().unwrap();  // ← Acquire again
println!("Counter: {}", value);        // 1
```

**Why?** Ensures only one thread accesses data at a time. Prevents data races.

#### `Arc<Mutex<T>>` - Thread-Safe Shared Mutable State

```rust
use std::sync::{Arc, Mutex};
use std::thread;

let counter = Arc::new(Mutex::new(0));

let mut handles = vec![];
for _ in 0..10 {
    let counter_clone = counter.clone();  // Clone Arc (cheap)
    let handle = thread::spawn(move || {
        let mut num = counter_clone.lock().unwrap();
        *num += 1;
    });
    handles.push(handle);
}

for handle in handles {
    handle.join().unwrap();
}

println!("Result: {}", *counter.lock().unwrap());  // 10
```

#### Example from PolliNet

```rust
pub struct HostBleTransport {
    /// Queue of outbound frames
    outbound_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    //              ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //              Thread-safe shared queue
    
    /// Inbound reassembly buffers
    inbound_buffers: Arc<Mutex<HashMap<String, Vec<TxFragment>>>>,
    //               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //               Multiple threads can access safely
}

impl HostBleTransport {
    pub fn push_inbound(&self, data: Vec<u8>) -> Result<(), String> {
        // Acquire lock, insert fragment, release lock
        let mut buffers = self.inbound_buffers.lock();
        //                                     ^^^^^^
        //                                     Blocks if another thread holds lock
        
        buffers.entry(tx_id).or_insert_with(Vec::new).push(fragment);
        
        // Lock automatically released when `buffers` goes out of scope
        Ok(())
    }
    
    pub fn next_outbound(&self, max_len: usize) -> Option<Vec<u8>> {
        let mut queue = self.outbound_queue.lock();
        queue.pop_front()  // Remove from queue
        // Lock released here
    }
}
```

**Why `Arc<Mutex<...>>`?**
- `Arc`: Multiple threads can hold reference to same queue
- `Mutex`: Ensures only one thread modifies queue at a time
- Combination: Thread-safe shared mutable state!

---

## 7. External Libraries & Dependencies

### 7.1 Solana Libraries

#### `solana-sdk` - Core Solana Types

**What it does**: Provides fundamental Solana types and cryptography.

**Key Types**:
```rust
use solana_sdk::{
    pubkey::Pubkey,           // 32-byte public key (wallet address)
    signature::{Keypair, Signature, Signer},
    transaction::Transaction, // Solana transaction
    instruction::Instruction, // Single operation in transaction
    message::Message,         // Transaction message (what gets signed)
    hash::Hash,              // 32-byte hash (blockhash)
    system_instruction,       // System program instructions (transfer, etc.)
};
```

**Example - Build Transfer Transaction**:
```rust
use solana_sdk::system_instruction;

let sender = Keypair::new();
let recipient = Pubkey::new_unique();

let instruction = system_instruction::transfer(
    &sender.pubkey(),
    &recipient,
    1_000_000,  // 0.001 SOL (1 million lamports)
);

let recent_blockhash = Hash::new_unique();  // Would normally fetch from RPC

let mut transaction = Transaction::new_with_payer(
    &[instruction],
    Some(&sender.pubkey()),  // Fee payer
);

transaction.message.recent_blockhash = recent_blockhash;
transaction.sign(&[&sender], recent_blockhash);

// Transaction is now ready to submit!
```

#### `solana-client` - RPC Client

**What it does**: Communicates with Solana blockchain over HTTP.

**Example - Fetch Account**:
```rust
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;

let client = RpcClient::new("https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6");

// Fetch account data from blockchain
let account = client
    .get_account_with_commitment(
        &nonce_pubkey,
        CommitmentConfig::confirmed(),
    )
    .await?
    .value
    .ok_or(Error::AccountNotFound)?;

// account.data contains the account's state
// account.lamports contains the SOL balance
```

**Example - Submit Transaction**:
```rust
let signature = client
    .send_and_confirm_transaction(&transaction)
    .await?;

println!("Transaction confirmed: {}", signature);
```

#### `spl-token` - SPL Token Instructions

**What it does**: Build SPL token transfer instructions (like ERC-20 on Ethereum).

**Example - SPL Transfer**:
```rust
use spl_token::instruction as spl_instruction;
use spl_associated_token_account::get_associated_token_address;

// Get token accounts (derived from wallet + mint)
let sender_token_account = get_associated_token_address(
    &sender_wallet,
    &mint_address,
);
let recipient_token_account = get_associated_token_address(
    &recipient_wallet,
    &mint_address,
);

// Build transfer instruction
let instruction = spl_instruction::transfer(
    &spl_token::id(),              // SPL Token program ID
    &sender_token_account,          // Source token account
    &recipient_token_account,       // Destination token account
    &sender_wallet,                 // Authority
    &[],                           // No additional signers
    1_000_000,                     // Amount (in token's smallest unit)
)?;
```

---

### 7.2 BLE Libraries

#### `btleplug` - Cross-Platform BLE

**What it does**: Provides unified BLE API for Windows, macOS, Linux, Android.

**Example - Scanning** (macOS):
```rust
use btleplug::api::{Central, Manager as _, ScanFilter};
use btleplug::platform::Manager;

let manager = Manager::new().await?;
let adapters = manager.adapters().await?;
let central = adapters.into_iter().next().unwrap();

// Start scanning
central.start_scan(ScanFilter::default()).await?;
tokio::time::sleep(Duration::from_secs(5)).await;

// Get discovered devices
let peripherals = central.peripherals().await?;
for peripheral in peripherals {
    let properties = peripheral.properties().await?;
    println!("Found device: {:?}", properties);
}
```

#### `bluer` - Linux BlueZ Bindings

**What it does**: Linux-specific BLE using BlueZ D-Bus API (more features than btleplug).

**Example - Advertise**:
```rust
use bluer::{Adapter, adv::Advertisement};

let session = bluer::Session::new().await?;
let adapter = session.default_adapter().await?;

let advertisement = Advertisement {
    service_uuids: vec![SERVICE_UUID].into_iter().collect(),
    local_name: Some("PolliNet".to_string()),
    ..Default::default()
};

let handle = adapter.advertise(advertisement).await?;
// Advertising until handle is dropped
```

---

### 7.3 Serialization Libraries

#### `serde` - Serialization Framework

**What it does**: Converts Rust structs to/from various formats (JSON, Bincode, etc.)

**Derive Macros**:
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct Person {
    name: String,
    age: u32,
    #[serde(rename = "emailAddress")]  // Rename field in JSON
    email: String,
    #[serde(skip_serializing_if = "Option::is_none")]  // Omit if None
    phone: Option<String>,
}

let person = Person {
    name: "Alice".to_string(),
    age: 30,
    email: "alice@example.com".to_string(),
    phone: None,
};

// To JSON
let json = serde_json::to_string(&person)?;
println!("{}", json);
// {"name":"Alice","age":30,"emailAddress":"alice@example.com"}

// From JSON
let parsed: Person = serde_json::from_str(&json)?;
```

#### `bincode` - Binary Serialization

**What it does**: Compact binary format (Solana's wire format).

**Example**:
```rust
use bincode;

let transaction = Transaction::new(...);

// Serialize to bytes
let tx_bytes = bincode::serialize(&transaction)?;
println!("Transaction size: {} bytes", tx_bytes.len());

// Deserialize from bytes
let decoded: Transaction = bincode::deserialize(&tx_bytes)?;
```

**Why two versions (`bincode` and `bincode1`)?**
- Solana uses bincode 1.x
- Modern Rust uses bincode 2.x
- We need both for compatibility

---

### 7.4 Cryptography Libraries

#### `sha2` - SHA-256 Hashing

**What it does**: Compute SHA-256 checksums for integrity verification.

**Example**:
```rust
use sha2::{Sha256, Digest};

fn sha256_checksum(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    
    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&result);
    checksum
}

let data = b"Hello, world!";
let checksum = sha256_checksum(data);
println!("Checksum: {}", hex::encode(checksum));
```

#### `openssl` - TLS/Crypto

**What it does**: Provides TLS for HTTPS connections (RPC client).

**Why `vendored` feature?**
- Android doesn't have system OpenSSL
- `vendored` compiles OpenSSL from source
- Bundled in final library (no system dependency)

---

### 7.5 Compression

#### `lz4` - Fast Compression

**What it does**: Compresses transactions for efficient BLE transmission.

**Example**:
```rust
use lz4::block::{compress, decompress};

// Compress
let tx_bytes = bincode::serialize(&transaction)?;
let compressed = compress(&tx_bytes, None, false)?;
println!("Compressed: {} → {} bytes ({:.1}% reduction)",
    tx_bytes.len(),
    compressed.len(),
    (1.0 - compressed.len() as f64 / tx_bytes.len() as f64) * 100.0
);

// Decompress
let decompressed = decompress(&compressed, Some(tx_bytes.len() as i32))?;
assert_eq!(tx_bytes, decompressed);
```

**Performance**:
- **Speed**: ~500 MB/s compression (very fast!)
- **Ratio**: Typically 40-60% reduction for transactions
- **Trade-off**: Balanced speed/compression (better than gzip for small data)

---

## Summary: Complete Picture

### From User Click to Blockchain

1. **User clicks button** (Kotlin UI - Jetpack Compose)
2. **UI calls SDK method** (Kotlin API - `PolliNetSDK.kt`)
3. **SDK serializes request to JSON** (Kotlin - `kotlinx.serialization`)
4. **SDK calls JNI function** (Kotlin - `PolliNetFFI.kt`)
5. **JNI loads native library** (`libpollinet.so`)
6. **Rust FFI receives call** (`src/ffi/android.rs`)
7. **FFI deserializes JSON** (Rust - `serde_json`)
8. **FFI calls business logic** (Rust - `src/lib.rs`, `src/transaction/`)
9. **Transaction service builds tx** (Rust - Solana SDK)
10. **RPC client submits to blockchain** (Rust - `solana-client`)
11. **Solana processes transaction** (External - Blockchain)
12. **Response flows back through layers**
13. **UI updates with result**

### Key Technologies

- **Kotlin**: Modern Android development, coroutines, Jetpack Compose
- **JNI**: Bridge between Java/Kotlin and native code
- **Rust**: Systems programming language (safe, fast, concurrent)
- **Tokio**: Async runtime for Rust
- **Solana SDK**: Blockchain transaction building
- **BLE**: Wireless mesh networking
- **JSON**: Cross-language data interchange
- **Bincode**: Efficient binary serialization

### Design Principles

1. **Layered Architecture**: Clear separation between UI, SDK, FFI, Core
2. **Type Safety**: Rust prevents entire classes of bugs at compile time
3. **Async/Await**: Efficient concurrent operations
4. **Error Handling**: Explicit `Result` types (no hidden exceptions)
5. **Zero-Copy**: Borrow references instead of copying large data
6. **Thread Safety**: `Arc<Mutex<T>>` for shared mutable state

---

## Learning Path

To become an expert at explaining PolliNet:

### 1. Understand Each Layer Independently
- **Kotlin**: Learn coroutines, suspend functions, Result types
- **JNI**: Understand how Java and native code interoperate
- **Rust**: Master ownership, borrowing, async/await, error handling
- **Solana**: Learn transaction structure, nonce accounts, instructions

### 2. Trace Real Examples
- Pick a feature (e.g., "Create Offline Transaction")
- Follow the code from UI button click to blockchain
- Use debugger to step through each layer
- Print/log data at each boundary

### 3. Experiment
- Modify a function and see what breaks
- Add logging to understand data flow
- Create simple test cases
- Build a minimal example (just Kotlin ↔ Rust)

### 4. Read Error Messages
- Rust compiler errors are incredibly helpful
- They tell you EXACTLY what's wrong and how to fix it
- Don't ignore warnings - they often catch real bugs

### 5. Use Tools
- **Android Studio**: Debug Kotlin code, view layout inspector
- **rust-analyzer**: IDE support for Rust (shows types, errors)
- **logcat**: View Android logs (Kotlin and Rust logs)
- **cargo doc**: Generate documentation from Rust code

---

This explanation covers the complete PolliNet architecture from top to bottom. You now have a comprehensive understanding of how Kotlin communicates with Rust, how data flows through the system, and how external libraries are used. With this knowledge, you can confidently explain any aspect of the project!

